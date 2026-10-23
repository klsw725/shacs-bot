use super::support::partial_frame;
use shacs_providers::{
    parse_codex_media_stream, CODEX_SSE_MAX_AGGREGATE_BYTES, CODEX_SSE_MAX_FRAME_BYTES,
    CODEX_SSE_MAX_LINE_BYTES, CODEX_SSE_MAX_PARTIAL_IMAGES,
};

#[test]
fn oversized_sse_line_is_rejected() {
    // Given
    let body = format!("data: {}\n\n", "x".repeat(CODEX_SSE_MAX_LINE_BYTES + 1));

    // When
    let error = parse_codex_media_stream(&body, "gpt-5.6", &mut |_| {})
        .expect_err("oversized line must fail");

    // Then
    assert!(error.to_string().contains("line limit"));
}

#[test]
fn oversized_sse_frame_is_rejected() {
    // Given
    let line = "x".repeat(CODEX_SSE_MAX_FRAME_BYTES / 2 + 1);
    let body = format!("data: {line}\ndata: {line}\n\n");
    assert!(body.len() > CODEX_SSE_MAX_FRAME_BYTES);

    // When
    let error = parse_codex_media_stream(&body, "gpt-5.6", &mut |_| {})
        .expect_err("oversized frame must fail");

    // Then
    assert!(error.to_string().contains("frame limit"));
}

#[test]
fn oversized_sse_aggregate_is_rejected() {
    // Given
    let payload = "x".repeat(CODEX_SSE_MAX_LINE_BYTES / 2);
    let mut body = String::new();
    while body.len() <= CODEX_SSE_MAX_AGGREGATE_BYTES {
        body.push_str(&format!("data: {payload}\n\n"));
    }

    // When
    let error = parse_codex_media_stream(&body, "gpt-5.6", &mut |_| {})
        .expect_err("oversized aggregate must fail");

    // Then
    assert!(error.to_string().contains("aggregate limit"));
}

#[test]
fn partial_image_count_is_bounded() {
    // Given
    let mut body = "event: response.output_item.added\ndata: {\"type\":\"response.output_item.added\",\"item\":{\"type\":\"image_generation_call\",\"id\":\"ig_many\"}}\n\n".to_owned();
    for index in 0..=CODEX_SSE_MAX_PARTIAL_IMAGES {
        body.push_str(&partial_frame("ig_many", index + 1, index));
    }

    // When
    let error = parse_codex_media_stream(&body, "gpt-5.6", &mut |_| {})
        .expect_err("too many partials must fail");

    // Then
    assert!(error.to_string().contains("partial image limit"));
}
