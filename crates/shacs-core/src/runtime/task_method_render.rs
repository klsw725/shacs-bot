use super::task_method::*;
use super::task_method_calc_render::{acceptance, summary as calc_summary};
use super::task_method_compose::{
    availability_note, method_name, parts_line, planned_checks, protocol,
};
use super::task_method_image_render::summary as image_summary;
use super::task_method_report_render::summary as report_summary;

#[cfg(test)]
#[path = "task_method_render_tests.rs"]
mod tests;

pub(super) fn availability_text(availability: &Availability) -> String {
    let support = match availability.support {
        Support::Supported => "supported",
        Support::Unsupported => "unsupported",
    };
    let readiness = match availability.readiness {
        Readiness::Available => "available",
        Readiness::RequiresRuntimeCheck => "requires_runtime_check",
        Readiness::Unavailable => "unavailable",
    };
    let mut text = format!("{support}, {readiness}");
    if !availability.missing_required_tools.is_empty() {
        text.push_str(&format!(
            "; missing required tools: {}",
            availability.missing_required_tools.join(", ")
        ));
    }
    if !availability.missing_optional_tools.is_empty() {
        text.push_str(&format!(
            "; web research unavailable (missing optional tools: {})",
            availability.missing_optional_tools.join(", ")
        ));
    }
    if availability.readiness == Readiness::RequiresRuntimeCheck {
        text.push_str("; credentials and provider support checked at invocation");
    }
    text
}

pub(super) fn selection_text(selection: &Selection) -> String {
    let source = match selection.source {
        SelectionSource::Automatic => "automatic",
        SelectionSource::Explicit => "explicit",
        SelectionSource::OneShot => "one_shot",
        SelectionSource::Auto => "auto",
    };
    let (label, checks) = if selection.validation_claims.is_empty() {
        ("Planned checks", planned_checks(selection))
    } else {
        (
            "Checks",
            selection
                .validation_claims
                .iter()
                .map(claim_text)
                .collect::<Vec<_>>(),
        )
    };
    let gate = match selection.gate {
        Gate::NotEvaluated => "not_evaluated",
        Gate::WaitingApproval => "waiting_approval",
        Gate::Blocked => "blocked",
    };
    format!(
        "Method: {} ({source}, revision {}){}\nReason: {} — {}\nAvailability: {}{}; gate: {gate}\n{label}: {}",
        method_name(selection),
        selection.revision,
        if selection.superseded {
            " [superseded]"
        } else {
            ""
        },
        selection.reason.code(),
        selection.reason.explanation(),
        availability_text(&selection.availability),
        availability_note(selection),
        if checks.is_empty() {
            "none".to_owned()
        } else {
            checks.join("; ")
        }
    )
}

fn claim_text(claim: &ValidationClaim) -> String {
    let state = match claim.state {
        ClaimState::NotVerified if claim.evidence_refs.is_empty() => "not verified",
        ClaimState::NotVerified => "recorded, not verified",
        ClaimState::Verified => "verified",
        ClaimState::Failed => "failed",
        ClaimState::SupersededNotInherited => "superseded_not_inherited",
    };
    format!("{}: {state}", claim.check)
}

fn method_summary(request: &MethodRequest) -> String {
    let mut text = format!(
        "{}{}{}{}",
        parts_line(request),
        calc_summary(request),
        report_summary(request),
        image_summary(request)
    );
    if request.selection.method == MethodId::Composed && !text.is_empty() {
        text.push_str(&format!(
            "\nUser acceptance: {}",
            acceptance(request.user_acceptance.as_ref())
        ));
    }
    text
}

pub(super) fn footer(request: &MethodRequest) -> String {
    if request.selection.visible() {
        format!(
            "\n\n---\n{}{}\nDetails: /method status",
            selection_text(&request.selection),
            method_summary(request)
        )
    } else {
        String::new()
    }
}

pub(super) fn blocked(selection: &Selection) -> String {
    let next = next_steps(selection.method);
    format!("{}\nRequest blocked; no provider or workflow was started.\n{next}\nAfter changing the method, send a message to continue the preserved request.\nDetails: /method status", selection_text(selection))
}

pub(super) fn next_steps(method: MethodId) -> &'static str {
    match method {
        MethodId::General => "Use /method use general.",
        MethodId::ReproducibleCalculation => "exec is not registered: side-effect tools are off (--no-side-effects, or run --allow-remote without --allow-side-effects) or tools.exec.enable is false. Restart with them enabled, then use /method auto to reassess; or use /method use general.",
        MethodId::SourceGroundedReport => "Register read_file for local sources; or use /method use general.",
        MethodId::ImageGenerationReview => "image_generate is not registered: side-effect tools are off, or tools.imageGeneration.enable with a supported image provider is not configured. Restart with it enabled, then use /method auto to reassess; or use /method use general.",
        MethodId::Composed => "None of the requested parts is available: register the missing tools listed above and use /method auto, or use /method use general, calculation, report or image for one part.",
    }
}

pub(super) fn request_text(request: &MethodRequest) -> String {
    let mut text = format!(
        "Request: {} ({:?})\n{}\nGoal: {}",
        request.request_id,
        request.state,
        selection_text(&request.selection),
        request.goal_preview
    );
    let calculation = method_summary(request);
    if calculation.is_empty() {
        if let Some(accepted) = &request.user_acceptance {
            text.push_str(&format!(
                "\nUser acceptance: {}",
                acceptance(Some(accepted))
            ));
        }
    } else {
        text.push_str(&calculation);
    }
    if request.omitted_observation_count > 0 {
        text.push_str(&format!(
            "\nOlder observed effects omitted by retention limit: {} (not reverted).",
            request.omitted_observation_count
        ));
    }
    // The model asked these and chose written paths; with an image part none
    // of its words are shown.
    let image = request
        .selection
        .parts()
        .contains(&MethodId::ImageGenerationReview);
    let asked = |question: &str| {
        if image {
            "(the model's question is withheld)".to_owned()
        } else {
            question.to_owned()
        }
    };
    for decision in &request.user_decisions {
        text.push_str(&format!(
            "\nUser decision: {} => {}",
            asked(&decision.question),
            decision.answer
        ));
    }
    for question in &request.remaining_questions {
        text.push_str(&format!("\nRemaining question: {}", asked(question)));
    }
    for evidence in &request.evidence_facts {
        match evidence {
            EvidenceFact::GeneratedArtifact { reference } => {
                text.push_str(&format!("\nObserved artifact: {reference}"))
            }
            EvidenceFact::WrittenFile { path } if image => text.push_str(&format!(
                "\nObserved written file: {}",
                super::quiet_tools::quiet_text(path)
            )),
            EvidenceFact::WrittenFile { path } => {
                text.push_str(&format!("\nObserved written file: {path}"))
            }
        }
    }
    for effect in &request.external_effect_facts {
        text.push_str(&format!(
            "\nObserved effect: {} {:?}, {}, revision {}, turn {}, at {} — not reverted",
            effect.tool_name,
            effect.outcome,
            effect.argument_digest,
            effect.selection_revision,
            effect.turn_id,
            effect.observed_at
        ));
    }
    text
}

pub(super) fn projection(request: &MethodRequest) -> String {
    if !request.selection.visible() {
        return String::new();
    }
    let protocol = protocol(request);
    let rules = format!("\nPrevious validation is not inherited. Checks are not verified unless the runtime shows them verified; do not claim verification yourself. Observed effects are not reverted. Do not automatically rerun them.{protocol}\n</task-method>");
    let budget = PROJECTION_LIMIT.saturating_sub(rules.chars().count() + "\n<task-method>\n".len());
    format!(
        "\n<task-method>\n{}{rules}",
        request_text(request)
            .chars()
            .take(budget)
            .collect::<String>()
    )
}

const PROJECTION_LIMIT: usize = 8192;
