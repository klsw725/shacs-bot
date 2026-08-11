use super::*;
use crate::{Spec031CommandProcessReceipt, Spec031ReleaseCommandStatus, Spec031ReleaseGateKind};
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn trusted_producer_passes_and_bound_hash_tampering_fails() -> Result<(), Box<dyn std::error::Error>>
{
    let stdout = b"test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s\n";
    let command = command();
    let record = build_spec030_bwrap_record(TrustedBwrapExecution {
        source_digest: &format!("sha256:{}", "1".repeat(64)),
        image_digest: &format!("sha256:{}", "2".repeat(64)),
        command: &command,
        stdout,
        stderr: b"",
    })?;
    let path = temp_path();
    write_bundle(&path, &record, stdout)?;
    assert!(validate_spec030_bwrap_record(&path).is_ok());

    for (pointer, value) in [
        ("/source_digest", serde_json::json!("sha256:bad")),
        ("/producer/image_digest", serde_json::json!("sha256:bad")),
        ("/producer/producer_id", serde_json::json!("forged")),
        (
            "/producer/command_sha256",
            serde_json::json!(format!("sha256:{}", "3".repeat(64))),
        ),
        (
            "/producer/transcript_sha256",
            serde_json::json!(format!("sha256:{}", "4".repeat(64))),
        ),
    ] {
        let mut forged = serde_json::to_value(&record)?;
        *forged
            .pointer_mut(pointer)
            .ok_or("missing tamper pointer")? = value;
        std::fs::write(&path, serde_json::to_vec(&forged)?)?;
        assert_eq!(
            validate_spec030_bwrap_record(&path),
            Err(Spec030BwrapRecordError::Malformed)
        );
    }
    Ok(())
}

fn command() -> Spec031ReleaseCommandRecord {
    Spec031ReleaseCommandRecord {
        id: "spec030-bwrap-active".to_owned(),
        gate: Spec031ReleaseGateKind::FocusedCargoTest,
        package: Some("shacs-core".to_owned()),
        filter: Some(BWRAP_TEST_NAME.to_owned()),
        argv: producer_argv(),
        cwd: ".".to_owned(),
        status: Spec031ReleaseCommandStatus::Passed,
        exit_code: Some(0),
        duration_ms: 1,
        stdout_path: "commands/spec030-bwrap-active.stdout".to_owned(),
        stderr_path: "commands/spec030-bwrap-active.stderr".to_owned(),
        tests: None,
        process_receipt: Some(Spec031CommandProcessReceipt {
            pid: u32::MAX,
            reaped: true,
            stdout_temp_path: "commands/.spec030-bwrap-active.stdout.tmp.1.1".to_owned(),
            stderr_temp_path: "commands/.spec030-bwrap-active.stderr.tmp.1.2".to_owned(),
            temp_paths_published: true,
        }),
    }
}

fn write_bundle(
    path: &Path,
    record: &Spec030BwrapRecord,
    stdout: &[u8],
) -> Result<(), Box<dyn std::error::Error>> {
    std::fs::write(path, serde_json::to_vec(record)?)?;
    std::fs::write(stdout_path(path), stdout)?;
    std::fs::write(stderr_path(path), b"")?;
    Ok(())
}

fn temp_path() -> PathBuf {
    std::env::temp_dir().join(format!(
        "spec030-producer-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |value| value.as_nanos())
    ))
}
