mod adversarial;
mod analyzer;
mod analyzer_runtime_probe;
mod crash;
pub(super) mod docs_policy;
mod edit;
mod lifecycle;
mod owner_fixture;
mod receipt_inputs;
mod receipt_model;
mod receipts;
mod receipts_acceptance;
mod receipts_expanded;
mod remote;
mod remote_matrix;
mod replay;
mod secret_scan;
mod surface_process;
mod surfaces;

use self::adversarial::{AdversarialInputs, AdversarialMatrix};
use self::analyzer::AnalyzerReport;
use self::crash::CrashReport;
use self::edit::EditReport;
use self::receipt_model::{validate_catalog_after_observation, ObservedReceipt};
use self::replay::ReplayReport;
use self::secret_scan::SecretScanReport;
use self::surfaces::SurfaceReport;
use serde::Serialize;
use shacs_core::generated_media::{
    ArtifactHandlingPolicy, ArtifactId, ArtifactStore, ArtifactWriteRequest,
    GeneratedArtifactDefinition, GeneratedArtifactMetadata, GeneratedMediaKind,
    GenerationOperation, ProjectionDisclosure, RetentionPolicy, Sha256Digest,
};
use shacs_core::runtime::{
    project_media_evidence_diagnostics, project_video_analyzer_spec035,
    MediaEvidenceDiagnosticsInput, VideoAnalyzerSpec035Input,
};
use shacs_projection::Spec031ExternalOwnerRef;
use std::error::Error;
use std::path::{Path, PathBuf};

#[derive(Debug, Serialize)]
pub struct SequentialReport {
    pub receipts: Vec<ObservedReceipt>,
    pub mapped_count: usize,
    pub catalog_validated_after_observation: bool,
    pub lifecycle_states: Vec<&'static str>,
    pub artifact_record_relative_path: String,
    pub artifact_record_exists: bool,
    pub artifact_digest: String,
    pub artifact_hash_consistent: bool,
    pub edit: EditReport,
    pub remote: remote::RemoteResult,
    pub analyzer: AnalyzerReport,
    pub crash: CrashReport,
    pub documentation_policy: docs_policy::DocumentationPolicyReport,
    pub replay: ReplayReport,
    pub surfaces: SurfaceReport,
    pub secret_scan: SecretScanReport,
    pub adversarial: AdversarialMatrix,
    pub cleanup: bool,
}

impl SequentialReport {
    pub fn is_complete(&self) -> bool {
        self.receipts.len() == 22
            && self.mapped_count == self.receipts.len()
            && self.catalog_validated_after_observation
            && self.adversarial.all_observed()
            && self.secret_scan.matches.is_empty()
            && self.surfaces.semantic_parity
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
    let analyzer = analyzer::run()?;
    let included = analyzer
        .included
        .as_ref()
        .ok_or("included analyzer unavailable")?;
    let disclosure = owner_fixture::disclosure();
    let diagnostics = project_media_evidence_diagnostics(MediaEvidenceDiagnosticsInput {
        artifacts: &[committed.record().clone(), edit_record.clone()],
        analyzer: included,
        disclosure: &disclosure,
    })?;
    let recorded = serde_json::to_string(&diagnostics)?;
    let replay = replay::run(&recorded)?;
    let artifact_ref = Spec031ExternalOwnerRef::try_new(&format!(
        "spec034://media/artifact/{}",
        committed.artifact_id.as_str()
    ))?;
    let canonical = project_video_analyzer_spec035(VideoAnalyzerSpec035Input {
        artifact_ref: &artifact_ref,
        analyzer: included,
    })?;
    let surfaces = surfaces::run(&repo, &root_path)?;
    let crash = crash::run(&root_path)?;
    let documentation_policy = docs_policy::run(&repo)?;
    let secret_scan = scan_outputs(
        &diagnostics,
        committed.record(),
        edit_record,
        &canonical,
        &remote,
        &analyzer,
        &surfaces,
    )?;
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
        analyzer: &analyzer,
        diagnostics: &diagnostics,
        replay: &replay,
        surfaces: &surfaces,
        scan: &secret_scan,
        crash: &crash,
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
        catalog_validated_after_observation,
        lifecycle_states,
        artifact_record_relative_path,
        artifact_record_exists,
        artifact_digest,
        artifact_hash_consistent,
        edit,
        remote,
        analyzer,
        crash,
        documentation_policy,
        replay,
        surfaces,
        secret_scan,
        adversarial,
        cleanup,
    })
}

fn scan_outputs(
    diagnostics: &shacs_core::runtime::MediaEvidenceDiagnostics,
    artifact: &shacs_core::generated_media::GeneratedArtifactRecord,
    edit: &shacs_core::generated_media::GeneratedArtifactRecord,
    canonical: &shacs_projection::Spec035MediaProjection,
    remote: &remote::RemoteResult,
    analyzer: &AnalyzerReport,
    surfaces: &SurfaceReport,
) -> Result<SecretScanReport, Box<dyn Error>> {
    let diagnostics = serde_json::to_string(diagnostics)?;
    let artifact = serde_json::to_string(artifact)?;
    let edit = serde_json::to_string(edit)?;
    let canonical = serde_json::to_string(canonical)?;
    let analyzer = serde_json::to_string(analyzer)?;
    let mut owned = vec![
        ("diagnostics", diagnostics),
        ("artifact", artifact),
        ("edit", edit),
        ("canonical", canonical),
        ("remote", remote.scan_output.clone()),
        ("analyzer", analyzer),
    ];
    owned.extend(surfaces.raw_outputs.iter().cloned());
    let outputs = owned
        .iter()
        .map(|(name, value)| (*name, value.as_str()))
        .collect::<Vec<_>>();
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
