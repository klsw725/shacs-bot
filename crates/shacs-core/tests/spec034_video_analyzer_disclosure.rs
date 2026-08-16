use shacs_core::runtime::{
    project_video_analyzer, VideoAnalysisPolicy, VideoAnalyzerCapability, VideoAnalyzerProjection,
    VideoAnalyzerProjectionInput,
};
use shacs_projection::{Spec031Freshness, TraceDestination, TracePreviewProjection};
use std::error::Error;

#[path = "spec034_video_analyzer_owner_facts/support.rs"]
mod support;
use support::OwnerFixture;

#[test]
fn analyzer_disclosure_omits_absolute_path_and_credential_query() -> Result<(), Box<dyn Error>> {
    let projection = project_with_preview(
        "/Users/private/exporters/trace",
        "https://trace.example/v1?token=secret&key=credential",
    )?;
    let serialized = serde_json::to_string(&projection)?;
    println!("{serialized}");

    assert!(!serialized.contains("/Users/private"));
    assert!(!serialized.contains("https://"));
    assert!(!serialized.contains("token=secret"));
    assert!(!serialized.contains("key=credential"));
    Ok(())
}

#[test]
fn analyzer_disclosure_redacts_secret_shaped_summaries() -> Result<(), Box<dyn Error>> {
    let projection = project_with_preview(
        "OPENAI_API_KEY=sk-live-analyzer-secret",
        "authorization=Bearer analyzer-secret-token",
    )?;
    let serialized = serde_json::to_string(&projection)?;

    assert!(!serialized.contains("sk-live-analyzer-secret"));
    assert!(!serialized.contains("analyzer-secret-token"));
    Ok(())
}

#[test]
fn analyzer_disclosure_bounds_exporter_endpoint_and_total_json() -> Result<(), Box<dyn Error>> {
    let projection = project_with_preview(&"e".repeat(8_000), &"p".repeat(8_000))?;
    let preview = projection
        .owner_facts
        .disclosure
        .as_ref()
        .and_then(|disclosure| disclosure.trace.preview.as_ref())
        .ok_or("missing bounded trace preview")?;
    let serialized = serde_json::to_string(&projection)?;

    assert!(preview.exporter.as_deref().unwrap_or_default().len() <= 120);
    assert!(
        preview
            .endpoint_summary
            .as_deref()
            .unwrap_or_default()
            .len()
            <= 120
    );
    assert!(serialized.len() <= 2_048);
    Ok(())
}

fn project_with_preview(
    exporter: &str,
    endpoint_summary: &str,
) -> Result<VideoAnalyzerProjection, Box<dyn Error>> {
    let fixture = OwnerFixture::new(
        "execution:spec034:disclosure",
        Some(TracePreviewProjection {
            record_count: 3,
            approximate_bytes: 512,
            destination: TraceDestination::ConfiguredRemote,
            exporter: Some(exporter.to_owned()),
            endpoint_summary: Some(endpoint_summary.to_owned()),
        }),
    )?;
    Ok(project_video_analyzer(VideoAnalyzerProjectionInput {
        capability: VideoAnalyzerCapability::Configured,
        duration_seconds: None,
        policy: VideoAnalysisPolicy::default(),
        outcome: None,
        owner_facts: fixture.input(Spec031Freshness::Current),
    })?)
}
