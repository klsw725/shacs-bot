use super::*;

#[cfg(target_vendor = "apple")]
#[test]
fn output_mutate_restore_is_detected() -> Result<(), Spec034ReleaseArtifactError> {
    let root = tempfile::tempdir().map_err(Spec034ReleaseArtifactError::Io)?;
    let output = root.path().join("output");
    std::fs::write(&output, b"A").map_err(Spec034ReleaseArtifactError::Io)?;
    let ledger = ExecutionLedger::arm(std::slice::from_ref(&output))?;

    std::fs::write(&output, b"B").map_err(Spec034ReleaseArtifactError::Io)?;
    std::fs::write(&output, b"A").map_err(Spec034ReleaseArtifactError::Io)?;

    assert!(matches!(
        ledger.verify(),
        Err(Spec034ReleaseArtifactError::DigestMismatch)
    ));
    Ok(())
}
