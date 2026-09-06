macro_rules! declare_spec034_runtime_modules {
    () => {
        mod analyzer_turn_control;
        mod generated_media_release;
        mod media_evidence_replay;
        mod runner_spec034;
        mod video_analyzer_disclosure;
        mod video_analyzer_evidence;
        mod video_analyzer_owner_facts;
        mod video_analyzer_runtime;
        mod video_analyzer_spec035;

        pub(crate) use analyzer_turn_control::{turn, MediaTurnControl, MediaTurnInput};
        pub use generated_media_release::{
            audit_spec034_release_artifacts_against,
            audit_spec034_release_artifacts_against_expected, run_spec034_linker_wrapper,
            run_spec034_release_runner, run_spec034_release_runner_with_linker_image,
            CommittedPublicationIdentity, CommittedPublicationResult, Spec034ReleaseArtifactError,
            Spec034ReleaseConfig, Spec034ReleaseManifest, Spec034ReleaseMode,
            Spec034StructuralAudit,
        };
        pub use media_evidence_replay::{
            project_media_evidence_diagnostics, replay_recorded_media_evidence,
            AnalyzerEvidenceSummary, ArtifactEvidenceSummary, MediaDisclosureSummary,
            MediaEvidenceAvailability, MediaEvidenceDiagnostics, MediaEvidenceDiagnosticsInput,
            MediaEvidenceProjectionError, MediaEvidenceReplayDependencies,
            MediaEvidenceReplayError, MediaEvidenceReplayReceipt, MediaEvidenceReplaySource,
            RecordedAnalyzerStatus, RecordedArtifactStatus,
        };
        pub use runner_spec034::public_result::AgentRunResult;
        pub use video_analyzer_disclosure::{
            VideoAnalyzerDisclosureProjection, VideoAnalyzerTraceDisclosureProjection,
            VideoAnalyzerTracePreviewProjection,
        };
        pub use video_analyzer_evidence::{
            project_video_analyzer, VideoAnalyzerCapability, VideoAnalyzerEvidenceProjection,
            VideoAnalyzerOutcomeInput, VideoAnalyzerProjection, VideoAnalyzerProjectionError,
            VideoAnalyzerProjectionInput, VideoAnalyzerStatus, VideoComponentFailureProjection,
        };
        pub use video_analyzer_owner_facts::{
            VideoAnalyzerOwnerFactsInput, VideoAnalyzerOwnerFactsProjection,
            VideoAnalyzerOwnerUnavailableReason, VideoAnalyzerSnapshotProjection,
            VideoAnalyzerSourceProjection,
        };
        pub use video_analyzer_runtime::{AnalyzerInvocation, AnalyzerMediaProvenance};
        pub use video_analyzer_spec035::{
            project_video_analyzer_spec035, Spec035MediaProjectionStore,
            Spec035MediaProjectionStoreError, Spec035MediaProjectionTransactionStage,
            VideoAnalyzerSpec035Error, VideoAnalyzerSpec035Input,
        };
    };
}

pub(super) use declare_spec034_runtime_modules;
