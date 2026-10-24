use super::task_method_image::{
    AttemptStatus, CheckState, ImageArtifact, ImageAttempt, ImageEvidence, ImageState,
    ImageVerdict, Requirement, RequirementKind,
};
use regex::Regex;
use std::sync::OnceLock;

#[cfg(test)]
#[path = "task_method_image_order_tests.rs"]
mod order_tests;
#[cfg(test)]
#[path = "task_method_image_tests.rs"]
mod tests;

/// Checkable requirements stated in the request texts; later texts override
/// earlier ones of the same kind (a revision overrides the original request).
pub(crate) fn requirements(texts: &[&str]) -> Vec<Requirement> {
    let mut found: Vec<Requirement> = Vec::new();
    for text in texts {
        for requirement in stated(text) {
            found.retain(|existing| existing.kind != requirement.kind);
            found.push(requirement);
        }
    }
    found.sort_by_key(|requirement| requirement.kind as u8);
    found
}

fn stated(text: &str) -> Vec<Requirement> {
    static SIZE: OnceLock<Option<Regex>> = OnceLock::new();
    static COUNT: OnceLock<Option<Regex>> = OnceLock::new();
    let text = text.to_lowercase();
    let size = SIZE.get_or_init(|| Regex::new(r"(?:^|\D)(\d{1,5})\s*[x×*]\s*(\d{1,5})").ok());
    let count =
        COUNT.get_or_init(|| Regex::new(r"(?:^|\D)(\d{1,2})\s*(?:images?\b|pictures?\b|장)").ok());
    let mut found = Vec::new();
    if let Some(captures) = size.as_ref().and_then(|size| size.captures(&text)) {
        found.push(requirement(
            RequirementKind::Size,
            format!("{}x{}", &captures[1], &captures[2]),
        ));
    }
    let format = text
        .split(|ch: char| !ch.is_ascii_alphanumeric())
        .find_map(|word| match word {
            "png" => Some("png"),
            "jpeg" | "jpg" => Some("jpeg"),
            "webp" => Some("webp"),
            _ => None,
        });
    if let Some(format) = format {
        found.push(requirement(RequirementKind::Format, format.to_owned()));
    }
    let number = count
        .as_ref()
        .and_then(|count| count.captures(&text))
        .and_then(|captures| captures[1].parse::<u32>().ok())
        .filter(|number| *number > 0);
    if let Some(number) = number {
        found.push(requirement(RequirementKind::Count, number.to_string()));
    }
    found
}

fn requirement(kind: RequirementKind, expected: String) -> Requirement {
    Requirement {
        kind,
        expected,
        state: CheckState::Unverified,
        observed: None,
    }
}

pub(crate) fn evaluate(evidence: &ImageEvidence, revision: u64, texts: &[&str]) -> ImageVerdict {
    let attempts: Vec<&ImageAttempt> = evidence
        .attempts
        .iter()
        .filter(|attempt| evidence.counts(attempt.selection_revision, revision))
        .collect();
    let mut requirements = requirements(texts);
    // The latest call decides: a later unsaved call withdraws an earlier image.
    let latest = attempts.last();
    let earlier_saved = evidence
        .last_saved_revision
        .is_some_and(|saved| evidence.counts(saved, revision))
        || attempts
            .iter()
            .any(|attempt| attempt.status == AttemptStatus::Generated);
    let (state, reason, result) = if let Some(attempt) =
        latest.filter(|attempt| attempt.status == AttemptStatus::Generated)
    {
        check(&mut requirements, &attempt.artifacts);
        let images = match attempt.artifacts.len() {
            1 => "1 image".to_owned(),
            count => format!("{count} images"),
        };
        (
            ImageState::Generated,
            format!("{images} saved and checked on disk; stated size, format and count are measured; content and look need your review"),
            Some(attempt.seq),
        )
    } else if latest.is_some_and(|attempt| attempt.status == AttemptStatus::CommitUncertain) {
        (
            ImageState::Uncertain,
            format!(
                "image_generate reported an image, but it could not be confirmed ({}), so no image is shown as generated{}",
                latest.and_then(|attempt| attempt.error.as_deref()).unwrap_or("no detail"),
                earlier_note(earlier_saved)
            ),
            None,
        )
    } else if let Some(last) = latest {
        let state = match last.status {
            AttemptStatus::Unavailable => ImageState::Unavailable,
            AttemptStatus::Unsupported => ImageState::Unsupported,
            AttemptStatus::Denied | AttemptStatus::Cancelled | AttemptStatus::Blocked => {
                ImageState::NotRun
            }
            _ => ImageState::Failed,
        };
        let error = last.error.as_deref().unwrap_or("no detail");
        (
            state,
            format!(
                "image_generate did not save an image ({}): {error}{}",
                last.status.label(),
                earlier_note(earlier_saved)
            ),
            None,
        )
    } else {
        (
            ImageState::NotRun,
            "no image_generate call ran for this method revision".to_owned(),
            None,
        )
    };
    ImageVerdict {
        selection_revision: revision,
        state,
        reason,
        result,
        requirements,
    }
}

fn earlier_note(earlier_saved: bool) -> &'static str {
    if earlier_saved {
        "; an earlier image from this request was saved, but it is not the current result"
    } else {
        ""
    }
}

fn check(requirements: &mut [Requirement], artifacts: &[ImageArtifact]) {
    for requirement in requirements {
        let observed: Vec<Option<String>> = match requirement.kind {
            RequirementKind::Size => artifacts
                .iter()
                .map(|artifact| {
                    let size = artifact.width.zip(artifact.height);
                    size.map(|(width, height)| format!("{width}x{height}"))
                })
                .collect(),
            RequirementKind::Format => artifacts
                .iter()
                .map(|artifact| artifact.format.clone())
                .collect(),
            RequirementKind::Count => vec![Some(artifacts.len().to_string())],
        };
        let expected = requirement.expected.as_str();
        requirement.state = if observed
            .iter()
            .flatten()
            .any(|value| value.as_str() != expected)
        {
            CheckState::NotMet
        } else if observed.iter().any(Option::is_none) {
            CheckState::Unverified
        } else {
            CheckState::Met
        };
        let mut seen: Vec<String> = observed
            .into_iter()
            .map(|value| value.unwrap_or_else(|| "unknown".to_owned()))
            .collect();
        seen.dedup();
        requirement.observed = Some(seen.join(", "));
    }
}
