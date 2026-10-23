use super::*;
use crate::runtime::task_method::{ClaimState, EffectOutcome};
use crate::runtime::task_method_image_claims::image_claims;
use crate::runtime::task_method_image_inspect::{attempt, image_call};
use crate::runtime::task_method_image_measure::measure;
use crate::runtime::task_method_observe::failure_outcome;
use serde_json::json;

pub(super) fn png_header(width: u32, height: u32) -> Vec<u8> {
    let mut bytes = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
    bytes.extend(width.to_be_bytes());
    bytes.extend(height.to_be_bytes());
    bytes.extend([8, 2, 0, 0, 0, 0, 0, 0, 0]);
    bytes
}

pub(super) fn saved(dir: &std::path::Path, name: &str, bytes: &[u8]) -> serde_json::Value {
    use sha2::{Digest, Sha256};
    let path = dir.join(name);
    std::fs::write(&path, bytes).unwrap();
    json!({"artifactId": name, "mediaRef": format!("media/image-generation/{name}"),
        "path": path, "sha256": format!("{:x}", Sha256::digest(bytes)),
        "providerId": "openai", "modelId": "image-model"})
}

#[test]
fn stated_size_format_and_count_are_read_and_later_text_overrides() {
    // Given
    let original = "Generate an image of a cat, 1024 x 1536 PNG, 2 images";
    let revision = "make it 512×512 instead";
    // When
    let found = requirements(&[original, revision]);
    let korean = requirements(&["고양이 이미지 생성해줘 1024x1024 jpg 3장"]);
    let plain = requirements(&["Generate an image of a 1024x1024 grid"]);
    // Then
    let expected: Vec<_> = found.iter().map(|r| r.expected.as_str()).collect();
    assert_eq!(expected, ["512x512", "png", "2"]);
    let korean: Vec<_> = korean.iter().map(|r| r.expected.as_str()).collect();
    assert_eq!(korean, ["1024x1024", "jpeg", "3"]);
    assert_eq!(plain.len(), 1);
}

#[test]
fn png_jpeg_and_webp_headers_give_format_and_pixel_size() {
    // Given
    let jpeg = [
        0xFF, 0xD8, 0xFF, 0xE0, 0, 4, 0, 0, 0xFF, 0xC0, 0, 17, 8, 0, 48, 0, 64, 3, 0, 0,
    ];
    let mut webp = b"RIFF\0\0\0\0WEBPVP8X\x0a\0\0\0\0\0\0\0".to_vec();
    webp.extend([63, 0, 0, 47, 0, 0]);
    // When / Then
    assert_eq!(measure(&png_header(64, 48)), Some(("png", 64, 48)));
    assert_eq!(measure(&jpeg), Some(("jpeg", 64, 48)));
    assert_eq!(measure(&webp), Some(("webp", 64, 48)));
    assert_eq!(measure(b"GIF89a"), None);
}

#[test]
fn only_files_inside_the_media_root_with_the_reported_digest_count_as_saved() {
    // Given
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let call = image_call("image_generate", &json!({"prompt": "cat", "size": "64x48"})).unwrap();
    let inside = saved(root.path(), "a.png", &png_header(64, 48));
    let mut tampered = saved(root.path(), "b.png", &png_header(64, 48));
    tampered["sha256"] = json!("0".repeat(64));
    let escaped = saved(outside.path(), "c.png", &png_header(64, 48));
    let roots = [root.path().to_path_buf()];
    // When
    let good = attempt(
        "1",
        &call,
        &json!({"artifacts": [inside]}).to_string(),
        None,
        &roots,
    );
    let bad = attempt(
        "2",
        &call,
        &json!({"artifacts": [tampered]}).to_string(),
        None,
        &roots,
    );
    let away = attempt(
        "3",
        &call,
        &json!({"artifacts": [escaped]}).to_string(),
        None,
        &roots,
    );
    // Then
    assert_eq!(good.status, AttemptStatus::Generated);
    assert_eq!(good.artifacts[0].width, Some(64));
    assert_eq!(good.options, ["size=64x48"]);
    assert_eq!(bad.status, AttemptStatus::CommitUncertain);
    assert_eq!(away.status, AttemptStatus::CommitUncertain);
    assert!(away.artifacts[0].width.is_none());
}

#[test]
fn provider_and_runtime_failures_map_to_their_own_states() {
    // Given
    let call = image_call("image_generate", &json!({"prompt": "cat"})).unwrap();
    let cases = [
        ("Error: Image generation unsupported/config failure: provider openai requires configured authentication", AttemptStatus::Unavailable),
        ("Error: Image generation unsupported/config failure: provider x does not support image_edit", AttemptStatus::Unsupported),
        ("Error: Image generation provider failure: code=image_generation_provider_error status=500", AttemptStatus::Failed),
        ("Error: Image generation media write failure: could not write image artifact: Permission denied (os error 13)", AttemptStatus::WriteFailed),
        ("Permission denied by user.", AttemptStatus::Denied),
        ("[Tool call skipped because the turn was cancelled]", AttemptStatus::Cancelled),
        ("{\"artifacts\": []}", AttemptStatus::Failed),
    ];
    for (output, expected) in cases {
        // When
        let recorded = attempt("1", &call, output, None, &[]);
        // Then
        assert_eq!(recorded.status, expected, "{output}");
        assert!(recorded.artifacts.is_empty());
    }
    let blocked = attempt("1", &call, "held back", Some("blocked before it ran"), &[]);
    assert_eq!(blocked.status, AttemptStatus::Blocked);
    let os_error = "Error: Image generation media write failure: could not write image artifact: Permission denied (os error 13)";
    let gate = "Error: Process launch blocked by permission gate (Denied)";
    assert_eq!(failure_outcome(os_error), EffectOutcome::Failed);
    assert_eq!(failure_outcome(gate), EffectOutcome::Denied);
    assert_eq!(
        failure_outcome("Permission denied by user."),
        EffectOutcome::Denied
    );
}

#[test]
fn requirements_are_measured_but_content_is_never_verified() {
    // Given
    let root = tempfile::tempdir().unwrap();
    let call = image_call("image_generate", &json!({"prompt": "cat"})).unwrap();
    let output = json!({"artifacts": [saved(root.path(), "a.png", &png_header(64, 48))]});
    let mut evidence = ImageEvidence::default();
    let roots = [root.path().to_path_buf()];
    evidence.push_attempt(attempt("1", &call, &output.to_string(), None, &roots), 1);
    // When
    let met = evaluate(&evidence, 1, &["Generate an image, 64x48 png"]);
    let missed = evaluate(&evidence, 1, &["Generate an image, 1024x1024 webp"]);
    evidence.verdict = Some(met.clone());
    let met_claims = image_claims(&evidence);
    evidence.verdict = Some(missed.clone());
    let missed_claims = image_claims(&evidence);
    // Then
    assert_eq!(met.state, ImageState::Generated);
    assert!(met.requirements.iter().all(|r| r.state == CheckState::Met));
    assert_eq!(met_claims[0].state, ClaimState::Verified);
    assert_eq!(met_claims[1].state, ClaimState::NotVerified);
    assert!(met_claims[1]
        .evidence_refs
        .iter()
        .any(|r| r.contains("needs your review")));
    assert!(missed
        .requirements
        .iter()
        .all(|r| r.state == CheckState::NotMet));
    assert_eq!(missed_claims[1].state, ClaimState::Failed);
    // A method change that keeps the image part keeps its calls; one that
    // drops or adds it starts over from its revision.
    assert_eq!(evaluate(&evidence, 2, &[]).state, ImageState::Generated);
    evidence.since = 2;
    assert_eq!(evaluate(&evidence, 2, &[]).state, ImageState::NotRun);
}

#[test]
fn a_revision_carries_the_goal_and_earlier_image_but_not_the_verdict() {
    // Given
    let root = tempfile::tempdir().unwrap();
    let call = image_call("image_generate", &json!({"prompt": "cat"})).unwrap();
    let output = json!({"artifacts": [saved(root.path(), "a.png", &png_header(64, 48))]});
    let mut first = ImageEvidence::default();
    let roots = [root.path().to_path_buf()];
    first.push_attempt(attempt("1", &call, &output.to_string(), None, &roots), 1);
    first.verdict = Some(evaluate(&first, 1, &["Generate a cat image"]));
    // When
    let second = ImageEvidence::next_version(&first, "method-request-1", "Generate a cat image");
    // Then
    assert_eq!(second.version, 2);
    assert_eq!(second.purpose.as_deref(), Some("Generate a cat image"));
    assert_eq!(second.previous[0].reference, "media/image-generation/a.png");
    assert!(second.attempts.is_empty() && second.verdict.is_none());
}
