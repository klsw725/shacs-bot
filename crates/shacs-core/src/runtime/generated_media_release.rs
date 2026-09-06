mod artifacts;
mod catalog;
mod model;
mod path_chain;
mod publication_result;
mod runner;
mod source;
mod source_descriptor;
mod source_git_config;
mod source_git_snapshot;
mod tools;

pub use model::{
    Spec034ReleaseArtifactError, Spec034ReleaseConfig, Spec034ReleaseManifest, Spec034ReleaseMode,
    Spec034StructuralAudit,
};
pub use publication_result::{CommittedPublicationIdentity, CommittedPublicationResult};
pub use runner::{
    audit_spec034_release_artifacts_against, audit_spec034_release_artifacts_against_expected,
    run_spec034_release_runner, run_spec034_release_runner_with_linker_image,
};

pub fn run_spec034_linker_wrapper() -> Result<(), Spec034ReleaseArtifactError> {
    tools::linker::run_wrapper()
}
