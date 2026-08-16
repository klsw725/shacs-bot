use serde_json::json;
use shacs_providers::{parse_codex_media_stream, ProviderEvent, ProviderMediaLifecycleStatus};
use std::error::Error;

fn main() -> Result<(), Box<dyn Error>> {
    let fixture = include_str!("../tests/fixtures/spec034_codex_media.sse");
    let mut lifecycle = Vec::new();
    let mut text_delta_count = 0usize;
    let response = parse_codex_media_stream(fixture, "gpt-5.6", &mut |event| match event {
        ProviderEvent::TextDelta { .. } => text_delta_count += 1,
        ProviderEvent::MediaLifecycle(observation) => {
            lifecycle.push(status_label(observation.status()))
        }
        ProviderEvent::ReasoningDelta { .. }
        | ProviderEvent::ToolCallStart { .. }
        | ProviderEvent::ToolCallDelta { .. }
        | ProviderEvent::ToolCallReady { .. }
        | ProviderEvent::Finish { .. } => {}
    })?;
    if text_delta_count != 0 || response.media_candidates.len() != 1 {
        return Err("Codex media fixture normalization failed".into());
    }
    println!(
        "{}",
        json!({
            "candidateCount": response.media_candidates.len(),
            "lifecycle": lifecycle,
            "textDeltaCount": text_delta_count,
        })
    );
    Ok(())
}

const fn status_label(status: ProviderMediaLifecycleStatus) -> &'static str {
    match status {
        ProviderMediaLifecycleStatus::Started => "started",
        ProviderMediaLifecycleStatus::Partial => "partial",
        ProviderMediaLifecycleStatus::Final => "final",
        ProviderMediaLifecycleStatus::Failed => "failed",
        ProviderMediaLifecycleStatus::Cancelled => "cancelled",
    }
}
