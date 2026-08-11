mod artifact_manifest;
mod bwrap_provenance;
mod bwrap_record;
mod bwrap_runner;
mod catalog;
mod cleanup;
mod command_contract;
#[cfg(test)]
mod command_contract_tests;
mod command_runner;
mod disk_validate;
mod fixture;
mod manual_qa;
mod model;
mod records;
mod runner;
mod semantic;
mod source_manifest;
mod surface_owner;
mod surface_owner_evidence;
#[cfg(test)]
mod surface_owner_evidence_tests;
mod surface_owner_http;
#[cfg(all(test, target_os = "linux"))]
mod surface_owner_linux_tests;
mod surface_owner_spawn;
#[cfg(test)]
mod surface_owner_tests;
mod surface_runner;
mod target_catalog;
mod validate;
mod writer;

pub use artifact_manifest::{
    build_spec030_artifact_manifest, Spec030ArtifactFile, Spec030ArtifactManifest,
    Spec030ArtifactManifestError, ARTIFACT_MANIFEST_PATH, SPEC030_ARTIFACT_MANIFEST_SCHEMA,
};
pub use bwrap_provenance::{
    validate_spec030_bwrap_record, Spec030BwrapPlatform, Spec030BwrapProducer, Spec030BwrapRecord,
    Spec030BwrapRecordError, SPEC030_BWRAP_RECORD_SCHEMA,
};
pub use cleanup::{
    validate_spec030_cleanup_receipt, Spec030CleanupReceipt, SPEC030_CLEANUP_SCHEMA,
};
pub use manual_qa::{
    parse_spec030_manual_qa, Spec030ManualCommand, Spec030ManualCommandStatus,
    Spec030ManualQaRecord, SPEC030_MANUAL_QA_SCHEMA,
};
pub use model::{
    Spec030CapturedFact, Spec030CoverageRow, Spec030ExternalEvidence, Spec030OwnerAudit,
    Spec030ReleaseArtifactError, Spec030ReleaseBlocker, Spec030ReleaseRunArtifacts,
    Spec030ReleaseRunId, Spec030ReleaseRunnerConfig, Spec030ReleaseRunnerMode,
    Spec030ReleaseVerdict, Spec030SurfaceArtifact, Spec030SurfaceOwnerEvidence,
    Spec030SurfaceOwnerReadiness, Spec030SurfaceOwnerShutdown, Spec030SurfaceOwnerSpawnSpec,
    SPEC030_RELEASE_RUNNER_SCHEMA,
};
pub use runner::run_spec030_release_runner;
pub use semantic::{parse_spec030_surface_assertions, Spec030SurfaceAssertions};
pub use source_manifest::{
    build_spec030_source_manifest, Spec030SourceFile, Spec030SourceFileKind, Spec030SourceManifest,
    Spec030SourceManifestError, SPEC030_SOURCE_MANIFEST_SCHEMA,
};
pub use target_catalog::{spec030_integration_targets, Spec030IntegrationTarget};
pub use validate::validate_spec030_release_artifacts;
