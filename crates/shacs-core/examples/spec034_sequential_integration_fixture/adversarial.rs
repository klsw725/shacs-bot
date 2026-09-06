use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use shacs_core::controlled_child::{
    run_generic_argv, ControlledChildAbort, ControlledChildCommand, ControlledChildOutcome,
};
use shacs_providers::{
    parse_codex_media_stream, ImageOperationLifecycle, ProviderEvent, ProviderMediaCandidateId,
    ProviderMediaLifecycleObservation, ProviderMediaLifecycleStatus,
};
use std::error::Error;
use std::process::{Command, Stdio};
use std::time::Duration;

#[derive(Debug, serde::Serialize)]
pub struct AdversarialMatrix {
    pub malformed_input: bool,
    pub untrusted_external_text: bool,
    pub cancel_resume: bool,
    pub stale_state: bool,
    pub dirty_worktree: bool,
    pub hung_commands: bool,
    pub flaky_tests: bool,
    pub misleading_success: bool,
    pub repeated_interruptions: bool,
}

pub struct AdversarialInputs {
    pub untrusted_external_text: bool,
    pub replacement_revalidated: bool,
    pub crash_recovered: bool,
}

impl AdversarialMatrix {
    pub const fn all_observed(&self) -> bool {
        self.malformed_input
            && self.untrusted_external_text
            && self.cancel_resume
            && self.stale_state
            && self.dirty_worktree
            && self.hung_commands
            && self.flaky_tests
            && self.misleading_success
            && self.repeated_interruptions
    }
}

pub fn run(
    root: &std::path::Path,
    inputs: AdversarialInputs,
) -> Result<AdversarialMatrix, Box<dyn Error>> {
    let raw = "not-valid-base64-secret";
    let malformed = format!(
        "event: response.output_item.done\ndata: {{\"type\":\"response.output_item.done\",\"item\":{{\"type\":\"image_generation_call\",\"id\":\"ig_bad\",\"status\":\"completed\",\"result\":\"{raw}\"}}}}\n\n"
    );
    let malformed_input = parse_codex_media_stream(&malformed, "gpt-5.6", &mut |_| {})
        .is_err_and(|error| !error.to_string().contains(raw));
    let misleading = concat!(
        "event: response.output_item.added\ndata: {\"type\":\"response.output_item.added\",\"item\":{\"type\":\"image_generation_call\",\"id\":\"ig_empty\"}}\n\n",
        "event: response.completed\ndata: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\",\"output\":[]}}\n\n",
    );
    let misleading_success = parse_codex_media_stream(misleading, "gpt-5.6", &mut |_| {}).is_err();
    let final_data = STANDARD.encode(b"stable-image");
    let stale_fixture = format!(
        concat!(
            "event: response.output_item.added\ndata: {{\"type\":\"response.output_item.added\",\"sequence_number\":10,\"item\":{{\"type\":\"image_generation_call\",\"id\":\"ig_stale\"}}}}\n\n",
            "event: response.output_item.done\ndata: {{\"type\":\"response.output_item.done\",\"sequence_number\":12,\"item\":{{\"type\":\"image_generation_call\",\"id\":\"ig_stale\",\"status\":\"completed\",\"result\":\"{final_data}\"}}}}\n\n",
            "event: response.output_item.done\ndata: {{\"type\":\"response.output_item.done\",\"sequence_number\":11,\"item\":{{\"type\":\"image_generation_call\",\"id\":\"ig_stale\",\"status\":\"completed\",\"result\":\"{final_data}\"}}}}\n\n"
        ),
        final_data = final_data,
    );
    let (first_count, first_statuses) = parse_observations(&stale_fixture)?;
    let (second_count, second_statuses) = parse_observations(&stale_fixture)?;
    let stale_state = first_count == 1
        && first_statuses
            .iter()
            .filter(|status| **status == ProviderMediaLifecycleStatus::Final)
            .count()
            == 1;

    let candidate = ProviderMediaCandidateId::new("interrupt-candidate")?;
    let mut cancelled = ImageOperationLifecycle::new();
    cancelled.apply(&ProviderMediaLifecycleObservation::started(
        candidate.clone(),
    ))?;
    cancelled.apply(&ProviderMediaLifecycleObservation::partial(
        candidate.clone(),
        1,
    ))?;
    cancelled.apply(&ProviderMediaLifecycleObservation::cancelled(
        candidate.clone(),
        Some(2),
    ))?;
    let repeated_interruptions = cancelled
        .apply(&ProviderMediaLifecycleObservation::cancelled(
            candidate.clone(),
            Some(3),
        ))
        .is_err()
        && cancelled
            .apply(&ProviderMediaLifecycleObservation::final_candidate(
                candidate.clone(),
                4,
            ))
            .is_err();
    let mut resumed = ImageOperationLifecycle::new();
    resumed.apply(&ProviderMediaLifecycleObservation::started(
        candidate.clone(),
    ))?;
    resumed.apply(&ProviderMediaLifecycleObservation::final_candidate(
        candidate, 1,
    ))?;

    let command = ControlledChildCommand::new(
        ["/bin/sh", "-c", "while :; do :; done"],
        root,
        Duration::from_millis(20),
    );
    let hung = run_generic_argv(&command, &ControlledChildAbort::new())?;
    let dirty_worktree = observe_dirty_worktree(root)?;
    Ok(AdversarialMatrix {
        malformed_input,
        untrusted_external_text: inputs.untrusted_external_text,
        cancel_resume: resumed.state() == shacs_providers::ImageOperationLifecycleState::Final,
        stale_state,
        dirty_worktree,
        hung_commands: matches!(hung.outcome, ControlledChildOutcome::TimedOut),
        flaky_tests: first_count == second_count && first_statuses == second_statuses,
        misleading_success: misleading_success
            && inputs.replacement_revalidated
            && inputs.crash_recovered,
        repeated_interruptions,
    })
}

fn parse_observations(
    input: &str,
) -> Result<(usize, Vec<ProviderMediaLifecycleStatus>), Box<dyn Error>> {
    let mut statuses = Vec::new();
    let response = parse_codex_media_stream(input, "gpt-5.6", &mut |event| {
        if let ProviderEvent::MediaLifecycle(observation) = event {
            statuses.push(observation.status());
        }
    })?;
    Ok((response.media_candidates.len(), statuses))
}

fn observe_dirty_worktree(root: &std::path::Path) -> Result<bool, Box<dyn Error>> {
    let repository = root.join("dirty-worktree");
    std::fs::create_dir(&repository)?;
    let initialized = Command::new("git")
        .args(["init", "--quiet"])
        .current_dir(&repository)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()?;
    if !initialized.success() {
        return Err("dirty worktree fixture could not initialize git".into());
    }
    std::fs::write(repository.join("untracked"), b"observation")?;
    let status = Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(&repository)
        .output()?;
    Ok(status.status.success() && !status.stdout.is_empty())
}
