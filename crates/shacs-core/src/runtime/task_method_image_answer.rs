use super::task_method::{MethodId, MethodRequest};
use super::task_method_image::{CheckState, ImageState};

const WITHHELD: &str =
    "The model's own reply is not shown, since nothing checks what it says about the image.";

fn state(request: &MethodRequest) -> ImageState {
    let revision = request.selection.revision;
    request
        .image
        .verdict
        .as_ref()
        .filter(|verdict| verdict.selection_revision == revision)
        .map_or(ImageState::NotRun, |verdict| verdict.state)
}

/// An image request's answer is the runtime's own account whatever the
/// outcome: the model's reply may describe an image, or content, that nothing
/// checked. The checked facts follow in the summary.
pub(super) fn guarded_answer(request: &MethodRequest, stop_reason: &str) -> Option<String> {
    (request.selection.method == MethodId::ImageGenerationReview)
        .then(|| withheld(image_headline(request), stop_reason))
}

/// Ends a runtime answer that replaces the model's reply.
pub(super) fn withheld(mut text: String, stop_reason: &str) -> String {
    if stop_reason != "completed" {
        text.push_str(&format!(" The turn ended early ({stop_reason})."));
    }
    text.push(' ');
    text.push_str(WITHHELD);
    text
}

/// What the runtime saw of the image part of this revision.
pub(super) fn image_headline(request: &MethodRequest) -> String {
    let revision = request.selection.revision;
    let evidence = &request.image;
    let state = state(request);
    if state == ImageState::Generated {
        let saved = match evidence.result_artifacts().len() {
            1 => "Image saved".to_owned(),
            count => format!("{count} images saved"),
        };
        let unmet = evidence.verdict.as_ref().is_some_and(|verdict| {
            verdict
                .requirements
                .iter()
                .any(|requirement| requirement.state == CheckState::NotMet)
        });
        let checked = if unmet {
            ", but not every stated requirement is met (see below)"
        } else {
            " and checked on disk (see below)"
        };
        format!("{saved}{checked}. Content and look are not checked automatically; please review the image yourself.")
    } else if evidence
        .attempts
        .iter()
        .any(|attempt| evidence.counts(attempt.selection_revision, revision))
    {
        format!(
            "No new image was saved for this request (image: {}); /method status lists each image call.",
            state.label()
        )
    } else {
        format!("No image was generated for this request (image: {}): no image_generate call ran. Only new images can be generated here (editing an existing image is unsupported); ask again to generate one, or use /method use general first for a plain reply.", state.label())
    }
}

/// The runtime's line in place of an `[image]` line of a composed report: the
/// model's words on that line are never shown.
pub(super) fn image_line(request: &MethodRequest) -> String {
    let state = state(request);
    if state != ImageState::Generated {
        return format!("[Image: none — the image part is {}]", state.label());
    }
    let images: Vec<&str> = request
        .image
        .result_artifacts()
        .into_iter()
        .map(|artifact| artifact.reference.as_str())
        .collect();
    format!(
        "[Image: {} — saved and checked on disk; content and look not checked]",
        images.join(", ")
    )
}
