use shacs_projection::{
    build_spec031_external_owner_projection, spec031_prd004_external_owner_artifacts,
    ExternalOwnerFact, ExternalOwnerFactInput, ExternalOwnerSpec, ExternalOwnerStatus,
    Spec031ArtifactError, Spec031ExternalCapability, Spec031ExternalOwnerReasonCode,
    Spec031ExternalOwnerReceiptRef, Spec031ExternalOwnerRef,
};
use std::error::Error;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_TEST_DIR_ID: AtomicU64 = AtomicU64::new(0);

fn app_fact(status: ExternalOwnerStatus) -> ExternalOwnerFact {
    ExternalOwnerFact::new(ExternalOwnerFactInput {
        owner: ExternalOwnerSpec::Spec032,
        capability: Spec031ExternalCapability::App,
        opaque_ref: Spec031ExternalOwnerRef::try_new("spec032://app/lifecycle/ref-1")
            .expect("safe app owner ref fixture"),
        status,
        reason_code: Spec031ExternalOwnerReasonCode::OwnerRecorded,
        receipt_ref: Some(
            Spec031ExternalOwnerReceiptRef::try_new("spec032://receipt/app-start-1")
                .expect("safe app receipt ref fixture"),
        ),
        stale: false,
    })
    .expect("consistent app owner fact fixture")
}

fn media_fact(status: ExternalOwnerStatus) -> ExternalOwnerFact {
    ExternalOwnerFact::new(ExternalOwnerFactInput {
        owner: ExternalOwnerSpec::Spec034,
        capability: Spec031ExternalCapability::Media,
        opaque_ref: Spec031ExternalOwnerRef::try_new("spec034://media/artifact/ref-1")
            .expect("safe media owner ref fixture"),
        status,
        reason_code: Spec031ExternalOwnerReasonCode::OwnerRecorded,
        receipt_ref: Some(
            Spec031ExternalOwnerReceiptRef::try_new("spec034://receipt/analyzer-1")
                .expect("safe media receipt ref fixture"),
        ),
        stale: false,
    })
    .expect("consistent media owner fact fixture")
}

#[test]
fn all_required_current_external_owner_facts_pass_read_audits() -> Result<(), Box<dyn Error>> {
    let evidence_root = test_output_dir()?;
    fs::create_dir_all(&evidence_root)?;
    let projection = build_spec031_external_owner_projection(
        [app_fact(ExternalOwnerStatus::Ready)],
        [media_fact(ExternalOwnerStatus::Included)],
    );
    let artifacts = spec031_prd004_external_owner_artifacts(&projection, &evidence_root)?;

    assert!(projection.closure_blockers.is_empty());
    assert!(artifacts
        .read_audits
        .iter()
        .all(|artifact| artifact.status == "PASS"));
    assert!(artifacts.closure_blockers.is_empty());
    fs::remove_dir_all(evidence_root)?;
    Ok(())
}

#[test]
fn prd004_external_owner_driver_writes_read_audits_and_blockers() -> Result<(), Box<dyn Error>> {
    let evidence_root = test_output_dir()?;
    fs::create_dir_all(&evidence_root)?;
    let artifacts = spec031_prd004_external_owner_artifacts(
        &build_spec031_external_owner_projection([app_fact(ExternalOwnerStatus::Degraded)], []),
        &evidence_root,
    )?;

    assert_eq!(artifacts.read_audits.len(), 2);
    assert!(artifacts
        .read_audits
        .iter()
        .any(
            |artifact| artifact.file_name == "spec032-read-audit.json" && artifact.status == "PASS"
        ));
    assert!(artifacts
        .read_audits
        .iter()
        .any(|artifact| artifact.file_name == "spec034-read-audit.json"
            && artifact.status == "BLOCKED"));
    assert!(artifacts
        .closure_blockers
        .iter()
        .any(|artifact| artifact.file_name == "spec034-media-closure-blocker.json"));
    assert!(evidence_root
        .join("available-partial-projection.json")
        .exists());
    fs::remove_dir_all(evidence_root)?;
    Ok(())
}

#[cfg(unix)]
#[test]
fn prd004_external_owner_writer_rejects_symlink_output_root() -> Result<(), Box<dyn Error>> {
    let outside = test_output_dir()?;
    fs::create_dir_all(&outside)?;
    let sentinel = outside.join("sentinel.txt");
    fs::write(&sentinel, "outside sentinel")?;
    let root = test_output_dir()?;
    std::os::unix::fs::symlink(&outside, &root)?;

    let error = spec031_prd004_external_owner_artifacts(
        &build_spec031_external_owner_projection([app_fact(ExternalOwnerStatus::Ready)], []),
        &root,
    )
    .expect_err("symlink root is rejected");

    assert!(matches!(error, Spec031ArtifactError::Io(_)));
    assert_eq!(fs::read_to_string(&sentinel)?, "outside sentinel");
    assert!(!outside.join("available-partial-projection.json").exists());
    Ok(())
}

#[cfg(unix)]
#[test]
fn prd004_external_owner_writer_rejects_final_artifact_symlink() -> Result<(), Box<dyn Error>> {
    let root = test_output_dir()?;
    fs::create_dir_all(&root)?;
    let sentinel = root.join("outside-projection-sentinel.json");
    fs::write(&sentinel, "outside sentinel")?;
    std::os::unix::fs::symlink(&sentinel, root.join("available-partial-projection.json"))?;

    let error = spec031_prd004_external_owner_artifacts(
        &build_spec031_external_owner_projection([app_fact(ExternalOwnerStatus::Ready)], []),
        &root,
    )
    .expect_err("final symlink is rejected");

    assert!(matches!(error, Spec031ArtifactError::Io(_)));
    assert_eq!(fs::read_to_string(&sentinel)?, "outside sentinel");
    Ok(())
}

fn test_output_dir() -> Result<PathBuf, Box<dyn Error>> {
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?;
    let unique = NEXT_TEST_DIR_ID.fetch_add(1, Ordering::Relaxed);
    Ok(temp_base().join(format!(
        "shacs-spec031-external-owner-{}-{}-{}",
        std::process::id(),
        now.as_nanos(),
        unique
    )))
}

fn temp_base() -> PathBuf {
    std::env::temp_dir()
        .canonicalize()
        .unwrap_or_else(|_| std::env::temp_dir())
}
