use super::*;

#[test]
fn command_stream_summary_never_publishes_raw_prose() -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let raw = b"Authorization: Basic dXNlcjpwYXNz\nCookie: session=opaque\n";

    super::write_summary(root.path(), "command.stdout", raw)?;

    let published = std::fs::read(root.path().join("command.stdout"))?;
    let text = std::str::from_utf8(&published)?;
    assert!(!text.contains("Authorization"));
    assert!(!text.contains("dXNlcjpwYXNz"));
    assert!(!text.contains("session=opaque"));
    let summary: CommandStreamSummary = serde_json::from_slice(&published)?;
    assert_eq!(summary.byte_count, raw.len() as u64);
    assert_eq!(summary.digest, super::super::super::artifacts::digest_bytes(raw));
    Ok(())
}

#[test]
fn portable_receipt_serializes_only_verifiable_cleanup_facts(
) -> Result<(), Box<dyn std::error::Error>> {
    let receipt = PortableProcessReceipt {
        reaped: true,
        temp_paths_published: true,
    };

    let value = serde_json::to_value(receipt)?;

    assert_eq!(value.as_object().ok_or("object")?.len(), 2);
    assert!(value.get("pid").is_none());
    assert!(value.get("duration_ms").is_none());
    assert!(value.get("stdout_temp_locator").is_none());
    assert!(value.get("stderr_temp_locator").is_none());
    assert!(!serde_json::to_string(&value)?.contains(".tmp."));
    Ok(())
}
