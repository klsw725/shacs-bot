use super::owner_fixture;
use serde::Serialize;
use shacs_core::runtime::{
    project_video_analyzer, VideoAnalysisPolicy, VideoAnalyzerCapability,
    VideoAnalyzerOutcomeInput, VideoAnalyzerOwnerFactsInput, VideoAnalyzerProjection,
    VideoAnalyzerProjectionInput, VideoAnalyzerStatus, VideoContextAnalysis,
};
use shacs_projection::{CredentialStatus, SandboxStatus, Spec031Freshness, TrustedCodeDisclosure};
use std::error::Error;

#[derive(Debug, Serialize)]
pub struct AnalyzerReport {
    pub states: Vec<&'static str>,
    pub codec_unsupported: bool,
    pub duration_capped: bool,
    pub typed_disclosure_recorded: bool,
    pub sandbox_not_universal: bool,
    pub credential_not_exposed: bool,
    pub trusted_source_disclosed: bool,
    pub runtime: super::analyzer_runtime_probe::AnalyzerRuntimeProbe,
    pub missing_explicit: bool,
    pub codec_reason_recorded: bool,
    pub duration_reason_recorded: bool,
    #[serde(skip)]
    pub included: Option<VideoAnalyzerProjection>,
}

pub fn run() -> Result<AnalyzerReport, Box<dyn Error>> {
    let policy = VideoAnalysisPolicy::default();
    let included_analysis = analysis("bounded fixture subtitle");
    let truncated_analysis = analysis(&"s".repeat(policy.max_subtitle_chars + 1));
    let owner_backed_included = owner_fixture::analyzer_projection()?;
    let configured = owner_fixture::ownerless_projection()?;
    let included = project(
        VideoAnalyzerCapability::Configured,
        Some(VideoAnalyzerOutcomeInput::Included(&included_analysis)),
        policy,
    )?;
    let missing = project(VideoAnalyzerCapability::Missing, None, policy)?;
    let unsupported = project(
        VideoAnalyzerCapability::Configured,
        Some(VideoAnalyzerOutcomeInput::Unsupported("codec unavailable")),
        policy,
    )?;
    let failed = project(
        VideoAnalyzerCapability::Configured,
        Some(VideoAnalyzerOutcomeInput::Failed("analyzer failed")),
        policy,
    )?;
    let truncated = project(
        VideoAnalyzerCapability::Configured,
        Some(VideoAnalyzerOutcomeInput::Included(&truncated_analysis)),
        policy,
    )?;
    let cancelled = project(
        VideoAnalyzerCapability::Configured,
        Some(VideoAnalyzerOutcomeInput::Cancelled),
        policy,
    )?;
    let timeout = project(
        VideoAnalyzerCapability::Configured,
        Some(VideoAnalyzerOutcomeInput::TimedOut),
        policy,
    )?;
    let duration = project_video_analyzer(VideoAnalyzerProjectionInput {
        capability: VideoAnalyzerCapability::Configured,
        duration_seconds: Some(policy.max_duration_seconds + 1),
        policy,
        outcome: None,
        owner_facts: VideoAnalyzerOwnerFactsInput::unavailable(Spec031Freshness::Unavailable),
    })?;
    let statuses = [
        configured.status,
        included.status,
        missing.status,
        unsupported.status,
        failed.status,
        truncated.status,
        cancelled.status,
        timeout.status,
        duration.status,
    ];
    let expected = [
        VideoAnalyzerStatus::Configured,
        VideoAnalyzerStatus::Included,
        VideoAnalyzerStatus::AnalyzerMissing,
        VideoAnalyzerStatus::Unsupported,
        VideoAnalyzerStatus::ExtractionFailed,
        VideoAnalyzerStatus::Truncated,
        VideoAnalyzerStatus::Cancelled,
        VideoAnalyzerStatus::Timeout,
        VideoAnalyzerStatus::DurationCap,
    ];
    if statuses != expected {
        return Err("analyzer states collapsed".into());
    }
    let owner = &owner_backed_included.owner_facts;
    let typed_disclosure_recorded = owner
        .disclosure
        .as_ref()
        .is_some_and(|disclosure| disclosure.raw_content_possible);
    let sandbox_not_universal = owner
        .sandbox
        .as_ref()
        .is_some_and(|sandbox| sandbox.status == SandboxStatus::Unknown);
    let credential_not_exposed = owner
        .credential
        .as_ref()
        .is_some_and(|credential| credential.status == CredentialStatus::Missing);
    let trusted_source_disclosed = owner
        .source
        .as_ref()
        .is_some_and(|source| source.trusted_code_disclosure == TrustedCodeDisclosure::Shown);
    Ok(AnalyzerReport {
        states: vec![
            "configured",
            "included",
            "analyzer_missing",
            "unsupported",
            "extraction_failed",
            "truncated",
            "cancelled",
            "timeout",
            "duration_cap",
        ],
        codec_unsupported: unsupported.status == VideoAnalyzerStatus::Unsupported,
        duration_capped: duration.status == VideoAnalyzerStatus::DurationCap,
        typed_disclosure_recorded,
        sandbox_not_universal,
        credential_not_exposed,
        trusted_source_disclosed,
        runtime: super::analyzer_runtime_probe::run()?,
        missing_explicit: missing.status == VideoAnalyzerStatus::AnalyzerMissing
            && missing.reason.is_some(),
        codec_reason_recorded: unsupported.status == VideoAnalyzerStatus::Unsupported
            && unsupported.reason.is_some(),
        duration_reason_recorded: duration.status == VideoAnalyzerStatus::DurationCap
            && duration.reason.is_some(),
        included: Some(owner_backed_included),
    })
}

fn analysis(subtitles: &str) -> VideoContextAnalysis {
    VideoContextAnalysis {
        metadata: None,
        subtitles: Some(subtitles.to_owned()),
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

fn project(
    capability: VideoAnalyzerCapability,
    outcome: Option<VideoAnalyzerOutcomeInput<'_>>,
    policy: VideoAnalysisPolicy,
) -> Result<VideoAnalyzerProjection, Box<dyn Error>> {
    Ok(project_video_analyzer(VideoAnalyzerProjectionInput {
        capability,
        duration_seconds: None,
        policy,
        outcome,
        owner_facts: VideoAnalyzerOwnerFactsInput::unavailable(Spec031Freshness::Unavailable),
    })?)
}
