use serde::Serialize;
use shacs_providers::{
    parse_codex_media_stream, ImageOperationLifecycle, ImageOperationLifecycleState, ProviderEvent,
    ProviderMediaCandidate, ProviderMediaCandidateId, ProviderMediaLifecycleObservation,
    ProviderMediaLifecycleStatus,
};
use std::error::Error;

#[derive(Debug, Serialize)]
pub struct LifecycleReport {
    pub states: Vec<&'static str>,
    pub text_delta_count: usize,
    pub final_candidate_count: usize,
    pub failed_error_redacted: bool,
    #[serde(skip)]
    pub candidate: Option<ProviderMediaCandidate>,
}

pub fn run() -> Result<LifecycleReport, Box<dyn Error>> {
    let mut statuses = Vec::new();
    let mut text_delta_count = 0usize;
    let response = parse_codex_media_stream(
        include_str!("../../../shacs-providers/tests/fixtures/spec034_codex_media.sse"),
        "gpt-5.6",
        &mut |event| match event {
            ProviderEvent::MediaLifecycle(observation) => statuses.push(observation.status()),
            ProviderEvent::TextDelta { .. } => text_delta_count += 1,
            ProviderEvent::ReasoningDelta { .. }
            | ProviderEvent::ToolCallStart { .. }
            | ProviderEvent::ToolCallDelta { .. }
            | ProviderEvent::ToolCallReady { .. }
            | ProviderEvent::Finish { .. } => {}
        },
    )?;
    let failed_raw = "provider raw secret";
    let failed_body = format!(
        "event: response.output_item.added\ndata: {{\"type\":\"response.output_item.added\",\"item\":{{\"type\":\"image_generation_call\",\"id\":\"ig_failed\"}}}}\n\nevent: response.failed\ndata: {{\"type\":\"response.failed\",\"response\":{{\"status\":\"failed\",\"error\":{{\"message\":\"{failed_raw}\"}}}}}}\n\n"
    );
    let mut failed_observed = false;
    let failed = parse_codex_media_stream(&failed_body, "gpt-5.6", &mut |event| {
        if matches!(
            event,
            ProviderEvent::MediaLifecycle(observation)
                if observation.status() == ProviderMediaLifecycleStatus::Failed
        ) {
            failed_observed = true;
        }
    });
    let failed_error_redacted =
        failed.is_err_and(|error| !error.to_string().contains(failed_raw)) && failed_observed;
    let candidate_id = ProviderMediaCandidateId::new("cancelled-lifecycle")?;
    let mut cancelled = ImageOperationLifecycle::new();
    cancelled.apply(&ProviderMediaLifecycleObservation::started(
        candidate_id.clone(),
    ))?;
    cancelled.apply(&ProviderMediaLifecycleObservation::cancelled(
        candidate_id,
        Some(1),
    ))?;
    if statuses
        != [
            ProviderMediaLifecycleStatus::Started,
            ProviderMediaLifecycleStatus::Partial,
            ProviderMediaLifecycleStatus::Final,
        ]
        || cancelled.state() != ImageOperationLifecycleState::Cancelled
        || !failed_error_redacted
    {
        return Err("media lifecycle receipt was incomplete".into());
    }
    let final_candidate_count = response.media_candidates.len();
    Ok(LifecycleReport {
        states: vec!["started", "partial", "final", "failed", "cancelled"],
        text_delta_count,
        final_candidate_count,
        failed_error_redacted,
        candidate: response.media_candidates.into_iter().next(),
    })
}
