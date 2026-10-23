use super::tests::{png_header, saved};
use super::*;
use crate::runtime::task_method_image::ATTEMPT_LIMIT;
use crate::runtime::task_method_image_inspect::{attempt, image_call};
use serde_json::json;

#[test]
fn a_later_unsaved_attempt_withdraws_an_earlier_generated_result_until_one_succeeds() {
    // Given
    let root = tempfile::tempdir().unwrap();
    let call = image_call("image_generate", &json!({"prompt": "cat"})).unwrap();
    let roots = [root.path().to_path_buf()];
    let saved_first = json!({"artifacts": [saved(root.path(), "a.png", &png_header(64, 48))]});
    let saved_again = json!({"artifacts": [saved(root.path(), "b.png", &png_header(64, 48))]});
    let failure =
        "Error: Image generation provider failure: code=image_generation_provider_error status=500";
    let mut evidence = ImageEvidence::default();
    evidence.push_attempt(
        attempt("1", &call, &saved_first.to_string(), None, &roots),
        1,
    );
    evidence.push_attempt(attempt("2", &call, failure, None, &roots), 1);
    // When
    let after_failure = evaluate(&evidence, 1, &["Generate an image, 64x48"]);
    evidence.push_attempt(
        attempt("3", &call, &saved_again.to_string(), None, &roots),
        1,
    );
    let recovered = evaluate(&evidence, 1, &["Generate an image, 64x48"]);
    // Then
    assert_eq!(after_failure.state, ImageState::Failed);
    assert!(after_failure.result.is_none());
    assert!(after_failure
        .reason
        .contains("an earlier image from this request was saved"));
    assert_eq!(recovered.state, ImageState::Generated);
    assert_eq!(recovered.result, Some(3));
}

#[test]
fn more_images_than_can_be_checked_are_never_reported_as_generated() {
    // Given
    let root = tempfile::tempdir().unwrap();
    let call = image_call("image_generate", &json!({"prompt": "cat", "count": 9})).unwrap();
    let items: Vec<_> = (0..9)
        .map(|index| saved(root.path(), &format!("{index}.png"), &png_header(64, 48)))
        .collect();
    let output = json!({ "artifacts": items }).to_string();
    // When
    let recorded = attempt("1", &call, &output, None, &[root.path().to_path_buf()]);
    // Then
    assert_eq!(recorded.status, AttemptStatus::CommitUncertain);
    assert!(recorded
        .error
        .as_deref()
        .unwrap_or("")
        .contains("9 images were reported"));
}

#[test]
fn every_kind_of_later_unsaved_call_withdraws_the_earlier_image() {
    // Given
    let root = tempfile::tempdir().unwrap();
    let call = image_call("image_generate", &json!({"prompt": "cat"})).unwrap();
    let roots = [root.path().to_path_buf()];
    let saved_first = json!({"artifacts": [saved(root.path(), "a.png", &png_header(64, 48))]});
    let mut unconfirmed = saved(root.path(), "b.png", &png_header(64, 48));
    unconfirmed["sha256"] = json!("0".repeat(64));
    let cases = [
        (json!({"artifacts": [unconfirmed]}).to_string(), ImageState::Uncertain),
        ("Permission denied by user.".to_owned(), ImageState::NotRun),
        ("Error: Image generation unsupported/config failure: provider openai requires configured authentication".to_owned(), ImageState::Unavailable),
    ];
    for (later, expected) in cases {
        let mut evidence = ImageEvidence::default();
        evidence.push_attempt(
            attempt("1", &call, &saved_first.to_string(), None, &roots),
            1,
        );
        evidence.push_attempt(attempt("2", &call, &later, None, &roots), 1);
        // When
        let verdict = evaluate(&evidence, 1, &[]);
        // Then
        assert_eq!(verdict.state, expected, "{later}");
        assert!(verdict.result.is_none());
        assert!(
            verdict.reason.contains("not the current result"),
            "{}",
            verdict.reason
        );
        assert_eq!(
            evidence.saved_artifacts()[0].reference,
            "media/image-generation/a.png"
        );
    }
}

#[test]
fn the_last_saved_image_survives_the_attempt_retention_limit() {
    // Given
    let root = tempfile::tempdir().unwrap();
    let call = image_call("image_generate", &json!({"prompt": "cat"})).unwrap();
    let roots = [root.path().to_path_buf()];
    let saved_first = json!({"artifacts": [saved(root.path(), "a.png", &png_header(64, 48))]});
    let failure =
        "Error: Image generation provider failure: code=image_generation_provider_error status=500";
    let mut evidence = ImageEvidence::default();
    evidence.push_attempt(
        attempt("1", &call, &saved_first.to_string(), None, &roots),
        1,
    );
    for index in 0..ATTEMPT_LIMIT {
        let id = format!("f{index}");
        evidence.push_attempt(attempt(&id, &call, failure, None, &roots), 1);
    }
    evidence.verdict = Some(evaluate(&evidence, 1, &[]));
    // When
    let next = ImageEvidence::next_version(&evidence, "method-request-1", "Generate a cat image");
    // Then
    assert_eq!(evidence.attempts.len(), ATTEMPT_LIMIT);
    assert!(evidence
        .verdict
        .as_ref()
        .is_some_and(|v| v.reason.contains("not the current result")));
    assert_eq!(
        evidence.saved_artifacts()[0].reference,
        "media/image-generation/a.png"
    );
    assert_eq!(next.previous[0].reference, "media/image-generation/a.png");
}
