macro_rules! declare_spec034_runtime_modules {
    () => {
        mod analyzer_turn_control;
        mod generated_media_release;
        mod runner_spec034;

        pub(crate) use analyzer_turn_control::{turn, MediaTurnControl, MediaTurnInput};
        pub use generated_media_release::{
            audit_spec034_release_artifacts_against,
            audit_spec034_release_artifacts_against_expected, run_spec034_linker_wrapper,
            run_spec034_release_runner, run_spec034_release_runner_with_linker_image,
            CommittedPublicationIdentity, CommittedPublicationResult, Spec034ReleaseArtifactError,
            Spec034ReleaseConfig, Spec034ReleaseManifest, Spec034ReleaseMode,
            Spec034StructuralAudit,
        };
        pub use runner_spec034::public_result::AgentRunResult;
    };
}

pub(super) use declare_spec034_runtime_modules;
