use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use shacs_session::durable_event::{
    DurableEventCompatibility, DurableEventError, DurableEventInput, DurableEventPayload,
    DurableEventStore, SESSION_TURN_ACCEPTED, SESSION_TURN_COMPLETED,
};
use std::error::Error;
use std::fs;
use std::path::Path;
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};

fn input(kind: &str, message: &str) -> DurableEventInput {
    let mut input = DurableEventInput::new(
        "session-1",
        kind,
        DurableEventPayload::inline("turn_fact", json!({"message": message})),
    );
    input.turn_id = Some("turn-1".to_owned());
    input.correlation_id = Some("correlation-1".to_owned());
    input
}

fn checksum(value: &Value) -> Result<String, Box<dyn Error>> {
    let bytes = serde_json::to_vec(value)?;
    Ok(format!("sha256:{:x}", Sha256::digest(bytes)))
}

fn rewrite_frame(
    path: &Path,
    index: usize,
    mutate: impl FnOnce(&mut Value),
    refresh_integrity: bool,
) -> Result<(), Box<dyn Error>> {
    let raw = fs::read_to_string(path)?;
    let mut frames = raw
        .lines()
        .map(serde_json::from_str::<Value>)
        .collect::<Result<Vec<_>, _>>()?;
    let frame = frames
        .get_mut(index)
        .ok_or_else(|| format!("missing frame {index}"))?;
    mutate(frame);
    if refresh_integrity {
        let record = frame
            .get("record")
            .ok_or("frame does not contain a record")?
            .clone();
        frame["record_length"] = json!(serde_json::to_vec(&record)?.len());
        frame["checksum"] = Value::String(checksum(&record)?);
    }
    let mut rewritten = frames
        .iter()
        .map(serde_json::to_string)
        .collect::<Result<Vec<_>, _>>()?
        .join("\n");
    rewritten.push('\n');
    fs::write(path, rewritten)?;
    Ok(())
}

#[test]
fn durable_event_store_appends_scans_and_reopens_monotonically() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let event_root = root.path().join("runtime").join("durable-events");
    let mut store = DurableEventStore::open(&event_root)?;
    assert!(store.is_writable());
    assert!(store.scan(10)?.records.is_empty());

    let first = store.append(input(SESSION_TURN_ACCEPTED, "accepted"))?;
    let second = store.append(input(SESSION_TURN_COMPLETED, "completed"))?;
    assert_eq!(first.sequence, 1);
    assert_eq!(first.event_id, "event-00000000000000000001");
    assert_eq!(second.sequence, 2);

    let scan = store.scan(10)?;
    assert_eq!(scan.records, vec![first, second]);
    assert_eq!(scan.last_sequence, Some(2));
    assert!(!scan.incomplete_tail);
    assert!(!scan.truncated);

    let mut reopened = DurableEventStore::open(&event_root)?;
    let third = reopened.append(input(SESSION_TURN_COMPLETED, "reopened"))?;
    assert_eq!(third.sequence, 3);
    assert_eq!(reopened.scan(10)?.last_sequence, Some(3));
    Ok(())
}

#[test]
fn durable_event_store_serializes_multiple_local_handles() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let mut first = DurableEventStore::open(root.path())?;
    let mut second = DurableEventStore::open(root.path())?;

    assert_eq!(
        first
            .append(input(SESSION_TURN_ACCEPTED, "first"))?
            .sequence,
        1
    );
    assert_eq!(
        second
            .append(input(SESSION_TURN_COMPLETED, "second"))?
            .sequence,
        2
    );
    assert_eq!(first.scan(10)?.last_sequence, Some(2));
    Ok(())
}

#[test]
fn durable_event_child_append() -> Result<(), Box<dyn Error>> {
    let Ok(root) = std::env::var("SHACS_DURABLE_EVENT_CHILD_ROOT") else {
        return Ok(());
    };
    let child_id = std::env::var("SHACS_DURABLE_EVENT_CHILD_ID")?;
    let mut store = DurableEventStore::open(&root)?;
    fs::write(Path::new(&root).join(format!("ready-{child_id}")), b"ready")?;
    let go = Path::new(&root).join("go");
    let deadline = Instant::now() + Duration::from_secs(10);
    while !go.exists() {
        if Instant::now() >= deadline {
            return Err("child timed out waiting for append signal".into());
        }
        thread::sleep(Duration::from_millis(10));
    }
    store.append(input(SESSION_TURN_ACCEPTED, &child_id))?;
    Ok(())
}

#[test]
fn durable_event_store_serializes_independent_processes() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let event_root = root.path().join("events");
    fs::create_dir_all(&event_root)?;
    let executable = std::env::current_exe()?;
    let mut children = Vec::new();
    for child_id in ["one", "two"] {
        children.push(
            Command::new(&executable)
                .arg("--exact")
                .arg("durable_event_child_append")
                .arg("--nocapture")
                .env("SHACS_DURABLE_EVENT_CHILD_ROOT", &event_root)
                .env("SHACS_DURABLE_EVENT_CHILD_ID", child_id)
                .spawn()?,
        );
    }
    let deadline = Instant::now() + Duration::from_secs(10);
    while ["one", "two"]
        .iter()
        .any(|child_id| !event_root.join(format!("ready-{child_id}")).exists())
    {
        if Instant::now() >= deadline {
            return Err("children timed out before concurrent append".into());
        }
        thread::sleep(Duration::from_millis(10));
    }
    fs::write(event_root.join("go"), b"go")?;
    for mut child in children {
        assert!(child.wait()?.success());
    }
    let scan = DurableEventStore::open(&event_root)?.scan(10)?;
    assert_eq!(scan.records.len(), 2);
    assert_eq!(scan.records[0].sequence, 1);
    assert_eq!(scan.records[1].sequence, 2);
    Ok(())
}

#[test]
fn durable_event_scan_is_memory_bounded_without_skipping_validation() -> Result<(), Box<dyn Error>>
{
    let root = tempfile::tempdir()?;
    let mut store = DurableEventStore::open(root.path())?;
    for message in ["one", "two", "three"] {
        store.append(input(SESSION_TURN_ACCEPTED, message))?;
    }

    let scan = store.scan(2)?;
    assert_eq!(scan.records.len(), 2);
    assert!(scan.truncated);
    assert_eq!(scan.last_sequence, Some(3));
    assert!(scan.bytes_scanned > 0);
    Ok(())
}

#[test]
fn durable_event_store_redacts_secrets_and_rejects_oversized_inline_payloads(
) -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let mut store = DurableEventStore::open(root.path())?;
    let secret = "sk-super-secret-value";
    store.append(DurableEventInput::new(
        "session-1",
        SESSION_TURN_ACCEPTED,
        DurableEventPayload::inline(
            "redacted_fact",
            json!({"api_key": secret, "nested": {"authorization": "Bearer token-value"}}),
        ),
    ))?;
    let raw = fs::read_to_string(store.path())?;
    assert!(!raw.contains(secret));
    assert!(!raw.contains("token-value"));
    assert!(raw.contains("[REDACTED]"));

    let error = store
        .append(DurableEventInput::new(
            "session-1",
            SESSION_TURN_ACCEPTED,
            DurableEventPayload::inline("oversized_fact", json!({"value": "x".repeat(70 * 1024)})),
        ))
        .expect_err("oversized inline payload must be rejected");
    assert!(matches!(error, DurableEventError::Validation(reason) if reason.contains("exceeds")));
    Ok(())
}

#[test]
fn durable_event_store_bounds_and_validates_the_complete_envelope() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let mut store = DurableEventStore::open(root.path())?;

    let payload_type_error = store
        .append(DurableEventInput::new(
            "session-1",
            SESSION_TURN_ACCEPTED,
            DurableEventPayload::inline("x".repeat(300), json!({})),
        ))
        .expect_err("oversized payload type must be rejected");
    assert!(matches!(
        payload_type_error,
        DurableEventError::Validation(_)
    ));

    let artifact_error = store
        .append(DurableEventInput::new(
            "session-1",
            SESSION_TURN_ACCEPTED,
            DurableEventPayload::artifact("tool_result", "sk-secret-artifact-ref"),
        ))
        .expect_err("secret-like artifact reference must be rejected");
    assert!(
        matches!(artifact_error, DurableEventError::Validation(reason) if reason.contains("secret-like"))
    );

    let absolute_artifact_error = store
        .append(DurableEventInput::new(
            "session-1",
            SESSION_TURN_ACCEPTED,
            DurableEventPayload::artifact("tool_result", "/tmp/raw-result"),
        ))
        .expect_err("absolute artifact reference must be rejected");
    assert!(
        matches!(absolute_artifact_error, DurableEventError::Validation(reason) if reason.contains("runtime-managed"))
    );

    let unmanaged_artifact_error = store
        .append(DurableEventInput::new(
            "session-1",
            SESSION_TURN_ACCEPTED,
            DurableEventPayload::artifact("tool_result", "custom/result.json"),
        ))
        .expect_err("unmanaged relative artifact reference must be rejected");
    assert!(
        matches!(unmanaged_artifact_error, DurableEventError::Validation(reason) if reason.contains("runtime-managed"))
    );

    for unsafe_ref in [
        ".nanobot/tool-results/../secret.json",
        ".nanobot\\tool-results\\result.json",
    ] {
        let error = store
            .append(DurableEventInput::new(
                "session-1",
                SESSION_TURN_ACCEPTED,
                DurableEventPayload::artifact("tool_result", unsafe_ref),
            ))
            .expect_err("unsafe artifact locator must be rejected");
        assert!(matches!(error, DurableEventError::Validation(_)));
    }

    store.append(DurableEventInput::new(
        "session-1",
        SESSION_TURN_ACCEPTED,
        DurableEventPayload::artifact("tool_result", ".nanobot/tool-results/result.json"),
    ))?;

    let mut provenance = shacs_session::durable_event::DurableEventProvenance::default();
    for index in 0..200 {
        provenance.skill_body_hashes.insert(
            format!("skill-{index}-{}", "x".repeat(800)),
            format!("sha256:{:064x}", index),
        );
    }
    let mut oversized_record = input(SESSION_TURN_ACCEPTED, "bounded");
    oversized_record.provenance = Some(provenance);
    let record_error = store
        .append(oversized_record)
        .expect_err("oversized complete event record must be rejected");
    assert!(
        matches!(record_error, DurableEventError::Validation(reason) if reason.contains("event record exceeds"))
    );
    assert_eq!(store.scan(10)?.records.len(), 1);
    Ok(())
}

#[cfg(unix)]
#[test]
fn durable_event_store_rejects_symlink_event_and_lock_files() -> Result<(), Box<dyn Error>> {
    use std::os::unix::fs::symlink;

    let event_root = tempfile::tempdir()?;
    let target = event_root.path().join("target");
    fs::write(&target, b"target")?;
    symlink(&target, event_root.path().join("events.log"))?;
    let event_error = DurableEventStore::open(event_root.path())
        .expect_err("symlink event file must be rejected");
    assert!(
        matches!(event_error, DurableEventError::Io(error) if error.kind() == std::io::ErrorKind::PermissionDenied)
    );

    let lock_root = tempfile::tempdir()?;
    let target = lock_root.path().join("target");
    fs::write(&target, b"target")?;
    symlink(&target, lock_root.path().join("events.lock"))?;
    let lock_error =
        DurableEventStore::open(lock_root.path()).expect_err("symlink lock file must be rejected");
    assert!(
        matches!(lock_error, DurableEventError::Io(error) if error.kind() == std::io::ErrorKind::PermissionDenied)
    );
    Ok(())
}

#[test]
fn durable_event_store_reports_checksum_corruption() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let mut store = DurableEventStore::open(root.path())?;
    store.append(input(SESSION_TURN_ACCEPTED, "safe"))?;
    let path = store.path().to_path_buf();
    rewrite_frame(
        &path,
        0,
        |frame| frame["record"]["payload"]["data"]["message"] = json!("evil"),
        false,
    )?;

    let error =
        DurableEventStore::open(root.path()).expect_err("checksum mismatch must reject the store");
    assert!(
        matches!(error, DurableEventError::Corruption { reason, .. } if reason.contains("checksum"))
    );
    Ok(())
}

#[test]
fn durable_event_store_detects_duplicate_gap_and_reordered_sequences() -> Result<(), Box<dyn Error>>
{
    for (label, replacement, expected) in [
        ("duplicate", 1_u64, "expected sequence 2, found 1"),
        ("gap", 3_u64, "expected sequence 2, found 3"),
        ("reordered", 0_u64, "expected sequence 2, found 0"),
    ] {
        let root = tempfile::tempdir()?;
        let event_root = root.path().join(label);
        let mut store = DurableEventStore::open(&event_root)?;
        store.append(input(SESSION_TURN_ACCEPTED, "one"))?;
        store.append(input(SESSION_TURN_COMPLETED, "two"))?;
        let path = store.path().to_path_buf();
        rewrite_frame(
            &path,
            1,
            |frame| frame["record"]["sequence"] = json!(replacement),
            true,
        )?;

        let error = DurableEventStore::open(&event_root)
            .expect_err("invalid sequence must reject the store");
        assert!(
            matches!(error, DurableEventError::Corruption { reason, .. } if reason.contains(expected))
        );
    }
    Ok(())
}

#[test]
fn durable_event_store_marks_truncated_final_frame_as_incomplete() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let mut store = DurableEventStore::open(root.path())?;
    store.append(input(SESSION_TURN_ACCEPTED, "accepted"))?;
    let path = store.path().to_path_buf();
    let mut bytes = fs::read(&path)?;
    bytes.truncate(bytes.len().saturating_sub(5));
    fs::write(&path, bytes)?;

    let mut reopened = DurableEventStore::open(root.path())?;
    let scan = reopened.scan(10)?;
    assert!(scan.incomplete_tail);
    assert!(scan.records.is_empty());
    assert!(!reopened.is_writable());
    assert!(matches!(
        reopened.append(input(SESSION_TURN_COMPLETED, "must not append")),
        Err(DurableEventError::IncompleteTail)
    ));
    Ok(())
}

#[test]
fn durable_event_store_returns_compatibility_for_unknown_schema_and_kind(
) -> Result<(), Box<dyn Error>> {
    let schema_root = tempfile::tempdir()?;
    let mut schema_store = DurableEventStore::open(schema_root.path())?;
    schema_store.append(input(SESSION_TURN_ACCEPTED, "schema"))?;
    let schema_path = schema_store.path().to_path_buf();
    rewrite_frame(
        &schema_path,
        0,
        |frame| frame["record"]["schema_version"] = json!(2),
        true,
    )?;
    let mut schema_store = DurableEventStore::open(schema_root.path())?;
    assert_eq!(
        schema_store.compatibility(),
        &DurableEventCompatibility::UnsupportedSchemaVersion { found: 2 }
    );
    assert!(!schema_store.is_writable());
    assert!(matches!(
        schema_store.append(input(SESSION_TURN_COMPLETED, "blocked")),
        Err(DurableEventError::ReadOnly(
            DurableEventCompatibility::UnsupportedSchemaVersion { found: 2 }
        ))
    ));

    let kind_root = tempfile::tempdir()?;
    let mut kind_store = DurableEventStore::open(kind_root.path())?;
    kind_store.append(input(SESSION_TURN_ACCEPTED, "kind"))?;
    let kind_path = kind_store.path().to_path_buf();
    rewrite_frame(
        &kind_path,
        0,
        |frame| frame["record"]["kind"] = json!("future.unknown"),
        true,
    )?;
    let kind_store = DurableEventStore::open(kind_root.path())?;
    assert_eq!(
        kind_store.compatibility(),
        &DurableEventCompatibility::UnsupportedKind {
            kind: "future.unknown".to_owned(),
            schema_version: 1,
        }
    );
    assert!(!kind_store.is_writable());

    let frame_root = tempfile::tempdir()?;
    let mut frame_store = DurableEventStore::open(frame_root.path())?;
    frame_store.append(input(SESSION_TURN_ACCEPTED, "frame"))?;
    let frame_path = frame_store.path().to_path_buf();
    rewrite_frame(
        &frame_path,
        0,
        |frame| frame["frame_version"] = json!(2),
        false,
    )?;
    let frame_store = DurableEventStore::open(frame_root.path())?;
    assert_eq!(
        frame_store.compatibility(),
        &DurableEventCompatibility::UnsupportedFrameVersion { found: 2 }
    );
    assert!(!frame_store.is_writable());
    Ok(())
}

#[test]
fn durable_event_store_rejects_unregistered_kind_before_append() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let mut store = DurableEventStore::open(root.path())?;
    let error = store
        .append(input("provider.delta", "transient"))
        .expect_err("transient provider kind must not become event truth");
    assert!(matches!(
        error,
        DurableEventError::ReadOnly(DurableEventCompatibility::UnsupportedKind { kind, .. })
            if kind == "provider.delta"
    ));
    assert_eq!(fs::metadata(store.path())?.len(), 0);
    Ok(())
}
