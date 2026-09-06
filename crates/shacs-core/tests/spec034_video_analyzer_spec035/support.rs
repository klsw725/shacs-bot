#[path = "../spec034_video_analyzer_owner_facts/support.rs"]
mod owner_support;

use owner_support::OwnerFixture;
use shacs_core::runtime::{
    project_video_analyzer, VideoAnalysisPolicy, VideoAnalyzerCapability,
    VideoAnalyzerOutcomeInput, VideoAnalyzerOwnerFactsInput, VideoAnalyzerProjection,
    VideoAnalyzerProjectionInput, VideoAnalyzerStatus, VideoContextAnalysis,
};
use shacs_projection::{
    DataDisclosureProjection, DataSurface, Spec031ExternalOwnerRef, Spec031Freshness,
    TraceDisclosureProjection, TraceStatus,
};
use std::error::Error;

pub fn artifact_ref() -> Result<Spec031ExternalOwnerRef, Box<dyn Error>> {
    Ok(Spec031ExternalOwnerRef::try_new(
        "spec034://media/artifact/canonical-mapper",
    )?)
}

pub fn disclosure() -> DataDisclosureProjection {
    DataDisclosureProjection {
        raw_content_possible: true,
        surfaces: vec![DataSurface::Session, DataSurface::Log],
        trace: TraceDisclosureProjection {
            status: TraceStatus::Disabled,
            preview: None,
        },
    }
}

pub fn analyzer_projection(
    status: VideoAnalyzerStatus,
) -> Result<VideoAnalyzerProjection, Box<dyn Error>> {
    let policy = VideoAnalysisPolicy::default();
    let owner = OwnerFixture::new("snapshot:034:canonical", None)?;
    let analysis = analysis(status == VideoAnalyzerStatus::Truncated, policy);
    let (capability, duration_seconds, outcome, owner_facts) = match status {
        VideoAnalyzerStatus::Configured => (
            VideoAnalyzerCapability::Configured,
            None,
            None,
            owner.input(Spec031Freshness::Current),
        ),
        VideoAnalyzerStatus::AnalyzerMissing => (
            VideoAnalyzerCapability::Missing,
            None,
            None,
            VideoAnalyzerOwnerFactsInput::unavailable(Spec031Freshness::Current),
        ),
        VideoAnalyzerStatus::Unsupported => (
            VideoAnalyzerCapability::Configured,
            None,
            Some(VideoAnalyzerOutcomeInput::Unsupported("codec unavailable")),
            owner.input(Spec031Freshness::Current),
        ),
        VideoAnalyzerStatus::ExtractionFailed => (
            VideoAnalyzerCapability::Configured,
            None,
            Some(VideoAnalyzerOutcomeInput::Failed("extraction failed")),
            owner.input(Spec031Freshness::Current),
        ),
        VideoAnalyzerStatus::Included | VideoAnalyzerStatus::Truncated => (
            VideoAnalyzerCapability::Configured,
            None,
            Some(VideoAnalyzerOutcomeInput::Included(&analysis)),
            owner.input(Spec031Freshness::Current),
        ),
        VideoAnalyzerStatus::DurationCap => (
            VideoAnalyzerCapability::Configured,
            Some(policy.max_duration_seconds + 1),
            None,
            owner.input(Spec031Freshness::Current),
        ),
        VideoAnalyzerStatus::Cancelled => (
            VideoAnalyzerCapability::Configured,
            None,
            Some(VideoAnalyzerOutcomeInput::Cancelled),
            owner.input(Spec031Freshness::Current),
        ),
        VideoAnalyzerStatus::Timeout => (
            VideoAnalyzerCapability::Configured,
            None,
            Some(VideoAnalyzerOutcomeInput::TimedOut),
            owner.input(Spec031Freshness::Current),
        ),
    };
    Ok(project_video_analyzer(VideoAnalyzerProjectionInput {
        capability,
        duration_seconds,
        policy,
        outcome,
        owner_facts,
    })?)
}

pub fn stale_success_projection() -> Result<VideoAnalyzerProjection, Box<dyn Error>> {
    let policy = VideoAnalysisPolicy::default();
    let analysis = analysis(false, policy);
    let owner = OwnerFixture::new("snapshot:034:stale", None)?;
    Ok(project_video_analyzer(VideoAnalyzerProjectionInput {
        capability: VideoAnalyzerCapability::Configured,
        duration_seconds: None,
        policy,
        outcome: Some(VideoAnalyzerOutcomeInput::Included(&analysis)),
        owner_facts: owner.input(Spec031Freshness::Stale),
    })?)
}

fn analysis(truncated: bool, policy: VideoAnalysisPolicy) -> VideoContextAnalysis {
    VideoContextAnalysis {
        metadata: None,
        subtitles: Some(if truncated {
            "s".repeat(policy.max_subtitle_chars + 1)
        } else {
            "bounded analyzer evidence".to_owned()
        }),
        scene_summary: None,
        keyframe_summary: None,
        extracted_audio_path: None,
        extracted_audio_mime: None,
        extracted_audio_byte_length: None,
        extracted_audio_duration_seconds: None,
        component_failures: Vec::new(),
        truncated: false,
    }
}
