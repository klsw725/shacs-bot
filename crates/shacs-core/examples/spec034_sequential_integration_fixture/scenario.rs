mod adversarial;
mod crash;
pub(super) mod docs_policy;
mod edit;
mod lifecycle;
mod receipt_inputs;
mod receipt_model;
mod receipts;
mod receipts_acceptance;
mod receipts_expanded;
mod remote;
mod remote_matrix;
mod secret_scan;

use self::adversarial::{AdversarialInputs, AdversarialMatrix};
use self::crash::CrashReport;
use self::edit::EditReport;
use self::receipt_model::{validate_catalog_after_observation, ObservedReceipt};
use self::secret_scan::SecretScanReport;
use serde::Serialize;
use shacs_core::generated_media::{
    ArtifactHandlingPolicy, ArtifactId, ArtifactStore, ArtifactWriteRequest,
    GeneratedArtifactDefinition, GeneratedArtifactMetadata, GeneratedMediaKind,
    GenerationOperation, ProjectionDisclosure, RetentionPolicy, Sha256Digest,
};
use shacs_projection::{spec034_current_requirements, SPEC034_RETIRED_REQUIREMENTS};
use std::error::Error;
use std::path::{Path, PathBuf};

#[derive(Debug, Serialize)]
pub struct SequentialReport {
    pub receipts: Vec<ObservedReceipt>,
    pub retired_requirements: Vec<&'static str>,
    pub mapped_count: usize,
    pub catalog_validated_after_observation: bool,
    pub lifecycle_states: Vec<&'static str>,
    pub artifact_record_relative_path: String,
    pub artifact_record_exists: bool,
    pub artifact_digest: String,
    pub artifact_hash_consistent: bool,
    pub edit: EditReport,
    pub remote: remote::RemoteResult,
    pub crash: CrashReport,
    pub documentation_policy: docs_policy::DocumentationPolicyReport,
    pub secret_scan: SecretScanReport,
    pub adversarial: AdversarialMatrix,
    pub cleanup: bool,
}

impl SequentialReport {
    pub fn is_complete(&self) -> bool {
        self.receipts.len() == spec034_current_requirements().count()
            && self.mapped_count == self.receipts.len()
            && self.retired_requirements == SPEC034_RETIRED_REQUIREMENTS
            && self.catalog_validated_after_observation
            && self.adversarial.all_observed()
            && self.secret_scan.matches.is_empty()
            && self.documentation_policy.is_complete()
            && self.cleanup
    }
}

pub fn run() -> Result<SequentialReport, Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let root_path = root.path().to_path_buf();
    let repo = repository_root()?;
    let store = ArtifactStore::open(&root_path)?;

    let mut lifecycle = lifecycle::run()?;
    let candidate = lifecycle
        .candidate
        .take()
        .ok_or("Codex lifecycle produced no final candidate")?;
    let committed = store.persist(ArtifactWriteRequest::new(candidate, generation_metadata()?))?;
    let payload = store.read_payload(&committed)?;
    let artifact_record_relative_path =
        format!("artifacts/{}/record.json", committed.artifact_id.as_str());
    let artifact_record_exists = root_path.join(&artifact_record_relative_path).is_file();
    let artifact_digest = committed.sha256.as_str().to_owned();
    let artifact_hash_consistent = Sha256Digest::from_bytes(&payload) == committed.sha256;

    let edit = edit::run(&store, &root_path)?;
    let edit_record = edit.record.as_ref().ok_or("edit record unavailable")?;
    let remote = remote::run(&store)?;
    let crash = crash::run(&root_path)?;
    let documentation_policy = docs_policy::run(&repo)?;
    let secret_scan = scan_outputs(committed.record(), edit_record, &remote)?;
    let adversarial = adversarial::run(
        &root_path,
        AdversarialInputs {
            untrusted_external_text: secret_scan.matches.is_empty()
                && remote.credential_headers_absent,
            replacement_revalidated: edit.replacement_revalidated,
            crash_recovered: crash.before_rename_hidden_and_clean && crash.after_rename_recovered,
        },
    )?;
    let receipts = receipts::build(receipt_inputs::ReceiptInputs {
        lifecycle: &lifecycle,
        artifact: committed.record(),
        artifact_record_exists,
        artifact_hash_consistent,
        edit: &edit,
        remote: &remote,
        scan: &secret_scan,
        documentation_policy: &documentation_policy,
    })?;
    let catalog_match_count = validate_catalog_after_observation(&receipts)?;
    let catalog_validated_after_observation = catalog_match_count == receipts.len();
    let lifecycle_states = lifecycle.states.clone();
    drop(store);
    root.close()?;
    let cleanup = !root_path.exists();
    Ok(SequentialReport {
        mapped_count: receipts.len(),
        receipts,
        retired_requirements: SPEC034_RETIRED_REQUIREMENTS.to_vec(),
        catalog_validated_after_observation,
        lifecycle_states,
        artifact_record_relative_path,
        artifact_record_exists,
        artifact_digest,
        artifact_hash_consistent,
        edit,
        remote,
        crash,
        documentation_policy,
        secret_scan,
        adversarial,
        cleanup,
    })
}

fn scan_outputs(
    artifact: &shacs_core::generated_media::GeneratedArtifactRecord,
    edit: &shacs_core::generated_media::GeneratedArtifactRecord,
    remote: &remote::RemoteResult,
) -> Result<SecretScanReport, Box<dyn Error>> {
    let artifact = serde_json::to_string(artifact)?;
    let edit = serde_json::to_string(edit)?;
    let outputs = [
        ("artifact", artifact.as_str()),
        ("edit", edit.as_str()),
        ("remote", remote.scan_output.as_str()),
    ];
    Ok(secret_scan::run(
        &outputs,
        &[
            ("remote_token", "untrusted-fixture-secret"),
            ("credential_query", "?token="),
            ("malformed_base64", "not-valid-base64-secret"),
            ("provider_failure", "provider raw secret"),
            ("absolute_analyzer_path", "/Users/private/bin/analyzer"),
            ("partial_base64", "cGFydGlhbA=="),
            ("data_url", "data:image"),
            ("authorization", "Bearer fixture-secret"),
        ],
    ))
}

fn generation_metadata() -> Result<GeneratedArtifactMetadata, Box<dyn Error>> {
    Ok(GeneratedArtifactMetadata::new(
        ArtifactId::new("codex-final")?,
        GeneratedArtifactDefinition::new(
            GeneratedMediaKind::Image,
            GenerationOperation::Generate,
            ArtifactHandlingPolicy::new(
                RetentionPolicy::UserManaged,
                ProjectionDisclosure::RawContentPossibleElsewhere,
            ),
        ),
        "2026-08-15T00:00:00Z",
    ))
}

fn repository_root() -> Result<PathBuf, Box<dyn Error>> {
    let current = std::env::current_dir()?;
    current
        .ancestors()
        .find(|path| path.join("crates/Cargo.toml").is_file())
        .map(Path::to_path_buf)
        .ok_or_else(|| "repository root not found".into())
}
