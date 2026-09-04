use super::*;

#[cfg(target_vendor = "apple")]
#[test]
fn inherited_socket_cannot_forge_receipt_through_wrapper_image(
) -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let wrapper = spawn::capture_process_identity(i32::try_from(std::process::id())?)?;
    let compiler = spawn::capture_static_identity(Path::new("/usr/bin/clang"))?;
    let receipts = LinkerReceipts::prepare(root.path(), wrapper, compiler)?;
    let ready = root.path().join("ready");
    let mut command = std::process::Command::new(std::env::current_exe()?);
    command
        .arg("forged_receipt_helper")
        .arg("--nocapture")
        .env("SHACS_SPEC034_FORGERY_READY", &ready);
    receipts.configure(&mut command);
    let mut child = command.spawn()?;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while !ready.exists() {
        if std::time::Instant::now() >= deadline {
            child.kill()?;
            return Err("forgery helper did not connect".into());
        }
        std::thread::yield_now();
    }

    assert!(receipts.verify().is_err());
    child.kill()?;
    child.wait()?;
    let current = spawn::capture_process_identity(i32::try_from(std::process::id())?)?;
    receipts.verify_identity(&current)?;
    Ok(())
}

#[cfg(target_vendor = "apple")]
#[test]
fn forged_receipt_helper() -> Result<(), Box<dyn std::error::Error>> {
    let Some(ready) = std::env::var_os("SHACS_SPEC034_FORGERY_READY") else {
        return Ok(());
    };
    let socket = std::env::var_os(SOCKET_ENV).ok_or("missing socket")?;
    let mut stream = UnixStream::connect(socket)?;
    stream.write_all(b"{}\n")?;
    std::fs::write(ready, b"ready")?;
    std::thread::sleep(std::time::Duration::from_secs(5));
    Ok(())
}

#[cfg(target_vendor = "apple")]
#[test]
fn unknown_target_executable_receipt_is_rejected() -> Result<(), Spec034ReleaseArtifactError> {
    let root = tempfile::tempdir().map_err(Spec034ReleaseArtifactError::Io)?;
    let identity = spawn::capture_process_identity(
        i32::try_from(std::process::id()).map_err(|_| Spec034ReleaseArtifactError::InvalidConfig)?,
    )?;
    let receipts = LinkerReceipts::prepare(root.path(), identity.clone(), identity)?;
    let receipt = LinkerReceipt {
        nonce: receipts.nonce.clone(),
        path: "unknown-executable".to_owned(),
        device: 0,
        inode: 0,
        size: 0,
        digest: digest_bytes(&[]),
        cdhash: Vec::new(),
    };

    assert!(matches!(
        receipts.verify_receipt(receipt),
        Err(Spec034ReleaseArtifactError::DigestMismatch)
    ));
    Ok(())
}

#[cfg(target_vendor = "apple")]
#[test]
fn captured_identity_verifies_after_short_lived_target_exit(
) -> Result<(), Spec034ReleaseArtifactError> {
    use std::os::unix::fs::MetadataExt;
    let root = tempfile::tempdir().map_err(Spec034ReleaseArtifactError::Io)?;
    let source = std::env::current_exe().map_err(Spec034ReleaseArtifactError::Io)?;
    let output = root.path().join("short-lived-target");
    std::fs::copy(&source, &output).map_err(Spec034ReleaseArtifactError::Io)?;
    let identity = spawn::capture_static_identity(&output)?;
    let wrapper = spawn::capture_process_identity(
        i32::try_from(std::process::id()).map_err(|_| Spec034ReleaseArtifactError::InvalidConfig)?,
    )?;
    let receipts = LinkerReceipts::prepare(root.path(), wrapper.clone(), wrapper)?;
    let metadata = std::fs::metadata(&output).map_err(Spec034ReleaseArtifactError::Io)?;
    let receipt = LinkerReceipt {
        nonce: receipts.nonce.clone(),
        path: "short-lived-target".to_owned(),
        device: metadata.dev(),
        inode: metadata.ino(),
        size: metadata.len(),
        digest: identity.digest.clone(),
        cdhash: identity.cdhash.clone(),
    };
    let monitor = ExecutionLedger::arm(std::slice::from_ref(&output))?;
    receipts
        .state
        .lock()
        .map_err(|_| Spec034ReleaseArtifactError::InvalidEvidence)?
        .insert(receipt.path.clone(), VerifiedReceipt { receipt, monitor });

    receipts.verify_identity(&identity)
}
