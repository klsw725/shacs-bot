use shacs_core::generated_media::{
    ArtifactId, CandidateId, GeneratedArtifactRecord, GeneratedMediaKind, GeneratedProvenance,
    GeneratedProvenanceKind, GenerationOperation, GenerationOptionsSummary, MediaRootRelativePath,
    ProjectionDisclosure, RetentionPolicy, SafeModelId, SafeProviderId, Sha256Digest,
};
use shacs_core::runtime::{
    project_media_evidence_diagnostics, project_video_analyzer, MediaEvidenceDiagnostics,
    MediaEvidenceDiagnosticsInput, MediaEvidenceReplayDependencies, VideoAnalysisPolicy,
    VideoAnalyzerCapability, VideoAnalyzerOutcomeInput, VideoAnalyzerOwnerFactsInput,
    VideoAnalyzerProjection, VideoAnalyzerProjectionInput, VideoContextAnalysis,
};
use shacs_projection::{
    DataDisclosureProjection, DataSurface, Spec031Freshness, TraceDisclosureProjection, TraceStatus,
};
use std::cell::Cell;
use std::error::Error;

#[path = "../spec034_video_analyzer_owner_facts/support.rs"]
mod owner_support;
use owner_support::OwnerFixture;

#[derive(Default)]
pub struct DependencySpies {
    network: Cell<u64>,
    credential: Cell<u64>,
    analyzer: Cell<u64>,
    resource: Cell<u64>,
}

impl DependencySpies {
    pub fn counts(&self) -> [u64; 4] {
        [
            self.network.get(),
            self.credential.get(),
            self.analyzer.get(),
            self.resource.get(),
        ]
    }
}

impl MediaEvidenceReplayDependencies for DependencySpies {
    fn request_network(&self) {
        self.network.set(self.network.get() + 1);
    }

    fn resolve_credential(&self) {
        self.credential.set(self.credential.get() + 1);
    }

    fn invoke_analyzer(&self) {
        self.analyzer.set(self.analyzer.get() + 1);
    }

    fn resolve_resource(&self) {
        self.resource.set(self.resource.get() + 1);
    }
}

pub fn artifact_record() -> Result<GeneratedArtifactRecord, Box<dyn Error>> {
    Ok(GeneratedArtifactRecord {
        schema: "shacs.generated-artifact.v1".to_owned(),
        artifact_id: ArtifactId::new("artifact-034")?,
        candidate_id: CandidateId::new("candidate-034")?,
        kind: GeneratedMediaKind::Image,
        media_root_relative_path: MediaRootRelativePath::new("artifacts/artifact-034/image.png")?,
        mime_type: "image/png".to_owned(),
        byte_len: 12,
        sha256: Sha256Digest::new("a".repeat(64))?,
        provenance: GeneratedProvenance {
            kind: GeneratedProvenanceKind::Generated,
            provider_id: SafeProviderId::new("provider-safe")?,
            model_id: SafeModelId::new("model-safe")?,
            operation: GenerationOperation::Generate,
            source_artifact_ids: Vec::new(),
        },
        generation_options_summary: GenerationOptionsSummary::default(),
        created_at: "2026-08-15T00:00:00Z".to_owned(),
        retention: RetentionPolicy::UserManaged,
        disclosure: ProjectionDisclosure::RawContentPossibleElsewhere,
    })
}

pub fn analyzer_projection() -> Result<VideoAnalyzerProjection, Box<dyn Error>> {
    let owner = OwnerFixture::new("snapshot:034", None)?;
    let analysis = VideoContextAnalysis {
        metadata: None,
        subtitles: Some("subtitle text".to_owned()),
        scene_summary: Some("scene text".to_owned()),
        keyframe_summary: Some("keyframe text".to_owned()),
        extracted_audio_path: None,
        extracted_audio_mime: None,
        extracted_audio_byte_length: None,
        extracted_audio_duration_seconds: None,
        component_failures: Vec::new(),
        truncated: false,
    };
    Ok(project_video_analyzer(VideoAnalyzerProjectionInput {
        capability: VideoAnalyzerCapability::Configured,
        duration_seconds: None,
        policy: VideoAnalysisPolicy::default(),
        outcome: Some(VideoAnalyzerOutcomeInput::Included(&analysis)),
        owner_facts: owner.input(Spec031Freshness::Current),
    })?)
}

pub fn ownerless_analyzer_projection() -> Result<VideoAnalyzerProjection, Box<dyn Error>> {
    Ok(project_video_analyzer(VideoAnalyzerProjectionInput {
        capability: VideoAnalyzerCapability::Configured,
        duration_seconds: None,
        policy: VideoAnalysisPolicy::default(),
        outcome: None,
        owner_facts: VideoAnalyzerOwnerFactsInput::unavailable(Spec031Freshness::Current),
    })?)
}

pub fn disclosure() -> DataDisclosureProjection {
    DataDisclosureProjection {
        raw_content_possible: true,
        surfaces: vec![DataSurface::Session, DataSurface::Log],
        trace: TraceDisclosureProjection {
            status: TraceStatus::Unavailable,
            preview: None,
        },
    }
}

pub fn recorded_with_analyzer_mutation(
    mutate: impl FnOnce(&mut serde_json::Value),
) -> Result<String, Box<dyn Error>> {
    let diagnostics = project_media_evidence_diagnostics(MediaEvidenceDiagnosticsInput {
        artifacts: &[artifact_record()?],
        analyzer: &analyzer_projection()?,
        disclosure: &disclosure(),
    })?;
    let mut recorded = serde_json::to_value(diagnostics)?;
    mutate(&mut recorded["analyzer"]);
    recorded["facts_digest"] = serde_json::json!(recomputed_digest(&recorded)?);
    Ok(serde_json::to_string(&recorded)?)
}

fn recomputed_digest(value: &serde_json::Value) -> Result<String, Box<dyn Error>> {
    use sha2::{Digest, Sha256};

    let mut digestible: MediaEvidenceDiagnostics = serde_json::from_value(value.clone())?;
    digestible.facts_digest.clear();
    Ok(format!(
        "sha256:{:x}",
        Sha256::digest(serde_json::to_vec(&digestible)?)
    ))
}
