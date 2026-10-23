use super::task_method::{MethodId, MethodRequest};
use super::task_method_calc_render::acceptance;
use super::task_method_compose_quote::withholds;
use super::task_method_image::{AttemptStatus, ImageArtifact, ImageState};
use super::task_method_image_claims::{requirement_text, short, UNSUPPORTED_EDIT};

pub(super) const IMAGE_PROTOCOL: &str = "Image protocol: create images only by calling image_generate in this request, and pass the user's requested size and format as its size and format arguments. The runtime records each call, checks the saved file, and measures size, format and count; content and look are for the user to review, so never claim that an image matches the request or call it verified. Editing an existing image (edit, mask, variation) is not available here: a requested change is a new generation, never an edit. The user sees the runtime's account of each call, not your reply.";

/// Context for the model when this request revises an earlier image.
pub(super) fn revision_context(request: &MethodRequest) -> String {
    let evidence = &request.image;
    let Some(previous) = &evidence.revision_of else {
        return String::new();
    };
    let earlier = if evidence.previous.is_empty() {
        "none was saved".to_owned()
    } else {
        references(&evidence.previous)
    };
    format!(
        "\nThis request is version {} revising {previous}. Original request: {}. Earlier image: {earlier}. Generate a new image for the requested change; {UNSUPPORTED_EDIT}.",
        evidence.version,
        evidence.purpose.as_deref().unwrap_or("not recorded")
    )
}

pub(super) fn summary(request: &MethodRequest) -> String {
    let evidence = &request.image;
    let Some(verdict) = evidence.verdict.as_ref() else {
        return String::new();
    };
    if !request
        .selection
        .parts()
        .contains(&MethodId::ImageGenerationReview)
        || verdict.selection_revision != request.selection.revision
    {
        return format!(
            "\nImage evidence from revision {}: superseded_not_inherited (was {}).",
            verdict.selection_revision,
            verdict.state.label()
        );
    }
    // In a composed request, a call's error (a tool's output, or a policy's
    // reason that may quote the prompt) stays in /method status.
    let withheld = withholds(request);
    let latest =
        evidence.attempts.iter().rev().find(|attempt| {
            evidence.counts(attempt.selection_revision, verdict.selection_revision)
        });
    let reason = match latest {
        Some(attempt) if withheld && verdict.state != ImageState::Generated => format!(
            "image_generate did not save an image ({}); the error is in /method status",
            attempt.status.label()
        ),
        _ => verdict.reason.clone(),
    };
    let mut text = format!("\nImage: {} — {reason}", verdict.state.label());
    let images = evidence.result_artifacts();
    if !images.is_empty() {
        let images: Vec<String> = images.into_iter().map(artifact_text).collect();
        text.push_str(&format!("\nImages: {}", images.join("; ")));
    } else if evidence
        .last_saved_revision
        .is_some_and(|saved| evidence.counts(saved, verdict.selection_revision))
        && !evidence.last_saved.is_empty()
    {
        let images: Vec<String> = evidence.last_saved.iter().map(artifact_text).collect();
        text.push_str(&format!(
            "\nEarlier saved image in this request (not the current result): {}",
            images.join("; ")
        ));
    }
    if verdict.state == ImageState::Generated {
        let mut checks: Vec<String> = verdict.requirements.iter().map(requirement_text).collect();
        checks.push(
            "content and look — not checked automatically; please review the image".to_owned(),
        );
        text.push_str(&format!("\nRequirements: {}", checks.join("; ")));
    } else if !verdict.requirements.is_empty() {
        text.push_str("\nRequirements: not checked, because no saved image was produced");
    }
    let failed: Vec<String> = evidence
        .attempts
        .iter()
        .filter(|attempt| {
            evidence.counts(attempt.selection_revision, verdict.selection_revision)
                && attempt.status != AttemptStatus::Generated
        })
        .take(5)
        .map(|attempt| match (withheld, attempt.error.as_deref()) {
            (true, _) => format!("#{} {}", attempt.seq, attempt.status.label()),
            (false, error) => format!(
                "#{} {} ({})",
                attempt.seq,
                attempt.status.label(),
                error.unwrap_or("no detail")
            ),
        })
        .collect();
    let repeats_reason = failed.len() == 1
        && !matches!(verdict.state, ImageState::Generated | ImageState::Uncertain);
    if !failed.is_empty() && !repeats_reason {
        text.push_str(&format!(
            "\nCalls without a saved image: {}",
            failed.join("; ")
        ));
    }
    if let Some(previous) = &evidence.revision_of {
        let earlier = if evidence.previous.is_empty() {
            "none was saved".to_owned()
        } else {
            references(&evidence.previous)
        };
        let produced = if verdict.state == ImageState::Generated {
            "This version is a new generation from your revised request"
        } else {
            "No new image was saved in this version"
        };
        text.push_str(&format!(
            "\nVersion {} revising {previous}: earlier image {earlier}. {produced}; {UNSUPPORTED_EDIT}. Earlier checks are re-run, not inherited.",
            evidence.version
        ));
    } else if verdict.state != ImageState::NotRun
        && request.selection.method == MethodId::ImageGenerationReview
    {
        text.push_str("\nTo revise this image: /method revise, then describe the change. The revision asks for a new image; editing the existing image is unsupported.");
    }
    if request.selection.method != MethodId::Composed {
        text.push_str(&format!(
            "\nUser acceptance: {}",
            acceptance(request.user_acceptance.as_ref())
        ));
    }
    text
}

pub(super) fn details(request: &MethodRequest) -> String {
    let evidence = &request.image;
    if evidence.attempts.is_empty() && evidence.previous.is_empty() {
        return String::new();
    }
    let mut text = "\nImage calls:".to_owned();
    for attempt in &evidence.attempts {
        text.push_str(&format!(
            "\n- #{} {} [{:?}] revision {} at {}",
            attempt.seq,
            attempt.call_id,
            attempt.status,
            attempt.selection_revision,
            attempt.observed_at
        ));
        if !attempt.options.is_empty() {
            text.push_str(&format!(" ({})", attempt.options.join(", ")));
        }
        if let Some(prompt) = &attempt.prompt {
            text.push_str(&format!("\n  prompt: {prompt}"));
        }
        for artifact in &attempt.artifacts {
            text.push_str(&format!(
                "\n  image: {} sha256:{} — {}",
                artifact.reference,
                artifact.sha256,
                if artifact.committed {
                    "saved file verified"
                } else {
                    "saved file not verified"
                }
            ));
        }
        if let Some(error) = &attempt.error {
            text.push_str(&format!("\n  error: {error}"));
        }
    }
    if evidence.omitted_attempts > 0 {
        text.push_str(&format!(
            "\nImage calls omitted by retention limit: {}",
            evidence.omitted_attempts
        ));
    }
    if !evidence.previous.is_empty() {
        let earlier: Vec<String> = evidence.previous.iter().map(artifact_text).collect();
        text.push_str(&format!("\nEarlier version images: {}", earlier.join("; ")));
    }
    text
}

fn artifact_text(artifact: &ImageArtifact) -> String {
    let mut facts = Vec::new();
    if let Some(format) = &artifact.format {
        facts.push(format.clone());
    }
    if let (Some(width), Some(height)) = (artifact.width, artifact.height) {
        facts.push(format!("{width}x{height}"));
    }
    if let Some(bytes) = artifact.byte_len {
        facts.push(format!("{bytes} bytes"));
    }
    facts.push(format!("sha256:{}…", short(&artifact.sha256)));
    if let Some(provider) = &artifact.provider {
        facts.push(format!("from {provider}"));
    }
    format!("{} ({})", artifact.reference, facts.join(", "))
}

fn references(artifacts: &[ImageArtifact]) -> String {
    artifacts
        .iter()
        .map(|artifact| artifact.reference.clone())
        .collect::<Vec<_>>()
        .join(", ")
}
