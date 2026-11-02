use rusqlite::config::DbConfig;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use shacs_session::durable_event::{
    DurableEventCompatibility, DurableEventError, DurableEventFilter, DurableEventInput,
    DurableEventPayload, DurableEventRecord, DurableEventSchemaRegistry, DurableEventStore,
    DURABLE_EVENT_DATABASE_FILE, LEGACY_DURABLE_EVENT_LOG_FILE, RUNTIME_STOP_REQUESTED,
    SESSION_TURN_ACCEPTED, SESSION_TURN_COMPLETED, WORKFLOW_PLANNED, WORK_CANCELLED,
    WORK_CANCEL_REQUESTED, WORK_ENQUEUED, WORK_LEASED, WORK_TERMINAL,
};
use shacs_session::durable_work::{
    apply_persisted_durable_work_event, DurableWorkEnqueueError, DurableWorkEnqueueInput,
    DurableWorkEnqueuer, DurableWorkReplayState, RuntimeControlRequested, WorkCancellation,
    WorkEnqueued, WorkLeased, WorkPayloadRef, WorkTerminal, WorkTerminalKind,
};
use std::error::Error;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant, SystemTime};
use std::{fs, thread};

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

fn checksum(text: &str) -> String {
    format!("sha256:{:x}", Sha256::digest(text.as_bytes()))
}

fn sidecar(root: &Path, suffix: &str) -> PathBuf {
    root.join(format!("{DURABLE_EVENT_DATABASE_FILE}{suffix}"))
}

/// Changes the database outside the store, with its append-only triggers switched off, the way
/// a foreign writer would.
fn foreign_writer(root: &Path) -> Result<Connection, Box<dyn Error>> {
    let connection = Connection::open(root.join(DURABLE_EVENT_DATABASE_FILE))?;
    connection.set_db_config(DbConfig::SQLITE_DBCONFIG_ENABLE_TRIGGER, false)?;
    Ok(connection)
}

/// Rewrites one stored record with a matching checksum, kind and schema version columns, so
/// that only what `mutate` changed differs from what the store wrote.
fn rewrite_record(
    root: &Path,
    sequence: i64,
    mutate: impl FnOnce(&mut Value),
) -> Result<(), Box<dyn Error>> {
    let connection = foreign_writer(root)?;
    let text: String = connection.query_row(
        "SELECT record FROM events WHERE sequence = ?1",
        [sequence],
        |row| row.get(0),
    )?;
    let mut record: Value = serde_json::from_str(&text)?;
    mutate(&mut record);
    let text = serde_json::to_string(&record)?;
    connection.execute(
        "UPDATE events SET record = ?1, checksum = ?2, kind = ?3, schema_version = ?4 \
         WHERE sequence = ?5",
        params![
            text,
            checksum(&text),
            record["kind"].as_str(),
            record["schema_version"].as_i64(),
            sequence
        ],
    )?;
    Ok(())
}

/// Moves every committed event from the WAL into the database file and empties the WAL, so
/// that the database file alone holds the store.
fn fold_wal(root: &Path) -> Result<(), Box<dyn Error>> {
    Connection::open(root.join(DURABLE_EVENT_DATABASE_FILE))?.query_row(
        "PRAGMA wal_checkpoint(TRUNCATE)",
        [],
        |_| Ok(()),
    )?;
    let wal = fs::metadata(sidecar(root, "-wal")).map_or(0, |metadata| metadata.len());
    if wal > 0 {
        return Err("the WAL must be empty once it is folded into the database".into());
    }
    Ok(())
}

/// Copies a store, through SQLite, to a root this process has never opened, so that opening the
/// copy is a first open, as in a new process.
fn unopened_copy(root: &Path, copy: &Path) -> Result<(), Box<dyn Error>> {
    fs::create_dir_all(copy)?;
    let target = copy.join(DURABLE_EVENT_DATABASE_FILE);
    Connection::open(root.join(DURABLE_EVENT_DATABASE_FILE))?.execute(
        "VACUUM INTO ?1",
        [target.to_str().ok_or("copy path is not UTF-8")?],
    )?;
    Ok(())
}

fn patch_bytes(path: &Path, from: &[u8], to: &[u8]) -> Result<(), Box<dyn Error>> {
    let mut bytes = fs::read(path)?;
    let found: Vec<usize> = bytes
        .windows(from.len())
        .enumerate()
        .filter(|(_, window)| *window == from)
        .map(|(index, _)| index)
        .collect();
    let [start] = found[..] else {
        return Err(format!("expected one match, found {}", found.len()).into());
    };
    bytes[start..start + to.len()].copy_from_slice(to);
    fs::write(path, bytes)?;
    Ok(())
}

type Snapshot = Vec<(PathBuf, &'static str, Vec<u8>, SystemTime)>;

/// Names, kinds, contents and modification times of everything under `root`, without following
/// symbolic links.
fn snapshot(root: &Path) -> Result<Snapshot, Box<dyn Error>> {
    let mut entries = Vec::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(&directory)? {
            let path = entry?.path();
            let metadata = fs::symlink_metadata(&path)?;
            let (kind, content) = if metadata.file_type().is_symlink() {
                (
                    "symlink",
                    fs::read_link(&path)?.into_os_string().into_encoded_bytes(),
                )
            } else if metadata.is_dir() {
                pending.push(path.clone());
                ("directory", Vec::new())
            } else {
                ("file", fs::read(&path)?)
            };
            entries.push((
                path.strip_prefix(root)?.to_path_buf(),
                kind,
                content,
                metadata.modified()?,
            ));
        }
    }
    entries.sort();
    Ok(entries)
}

#[test]
fn durable_event_store_reports_a_lock_poisoned_by_a_panicking_visitor() -> Result<(), Box<dyn Error>>
{
    let root = tempfile::tempdir()?;
    let event_root = root.path().join("runtime").join("durable-events");
    let mut store = DurableEventStore::open(&event_root)?;
    store.append(input(SESSION_TURN_ACCEPTED, "accepted"))?;

    let visitor_root = event_root.clone();
    let visitor = thread::spawn(move || {
        let reader = DurableEventStore::open(&visitor_root).expect("a second handle opens");
        let _ = reader.visit_from_sequence(0, |_record| panic!("visitor bug"));
    });
    assert!(
        visitor.join().is_err(),
        "the visitor panic reaches the visiting thread"
    );

    assert!(
        matches!(store.scan(10), Err(DurableEventError::Io(_))),
        "a store lock poisoned by the visitor is reported as an error, not a panic"
    );
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
    assert!(serde_json::to_string(&store.scan(10)?.records)?.contains("[REDACTED]"));
    let database = store.path().to_path_buf();
    let wal = sidecar(store.root(), "-wal");
    drop(store);
    let mut bytes = fs::read(&database)?;
    if let Ok(wal) = fs::read(&wal) {
        bytes.extend(wal);
    }
    let raw = String::from_utf8_lossy(&bytes);
    assert!(!raw.contains(secret));
    assert!(!raw.contains("token-value"));

    let mut store = DurableEventStore::open(root.path())?;
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
            format!("sha256:{index:064x}"),
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
fn durable_event_store_rejects_symlinked_store_files() -> Result<(), Box<dyn Error>> {
    use std::os::unix::fs::symlink;

    let root = tempfile::tempdir()?;
    let target = root.path().join("target");
    fs::write(&target, b"target")?;
    let is_symlink_refusal = |result: Result<DurableEventStore, DurableEventError>| matches!(result, Err(DurableEventError::Io(error)) if error.kind() == std::io::ErrorKind::PermissionDenied);

    let database_root = root.path().join("database");
    fs::create_dir_all(&database_root)?;
    symlink(&target, database_root.join(DURABLE_EVENT_DATABASE_FILE))?;
    assert!(is_symlink_refusal(DurableEventStore::open(&database_root)));

    for suffix in ["-wal", "-shm"] {
        let event_root = root.path().join(format!("sidecar{suffix}"));
        DurableEventStore::open(&event_root)?.append(input(SESSION_TURN_ACCEPTED, "kept"))?;
        fold_wal(&event_root)?;
        symlink(&target, sidecar(&event_root, suffix))?;
        assert!(is_symlink_refusal(DurableEventStore::open(&event_root)));
    }

    let real_root = root.path().join("real");
    DurableEventStore::open(&real_root)?;
    let linked_root = root.path().join("linked");
    symlink(&real_root, &linked_root)?;
    assert!(is_symlink_refusal(DurableEventStore::open(&linked_root)));
    assert_eq!(fs::read(&target)?, b"target");
    Ok(())
}

#[test]
fn durable_event_store_reports_checksum_corruption() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let seed = root.path().join("seed");
    DurableEventStore::open(&seed)?.append(input(SESSION_TURN_ACCEPTED, "safe"))?;
    let damaged = root.path().join("damaged");
    unopened_copy(&seed, &damaged)?;
    patch_bytes(
        &damaged.join(DURABLE_EVENT_DATABASE_FILE),
        br#""message":"safe""#,
        br#""message":"evil""#,
    )?;

    let error =
        DurableEventStore::open(&damaged).expect_err("checksum mismatch must reject the store");
    assert!(
        matches!(error, DurableEventError::Corruption { offset: 1, reason } if reason.contains("checksum"))
    );
    Ok(())
}

#[test]
fn durable_event_store_rejects_columns_that_differ_from_the_record() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let seed = root.path().join("seed");
    let mut event = DurableEventInput::new(
        "session-1",
        SESSION_TURN_ACCEPTED,
        DurableEventPayload::inline("turn_fact", json!({"work_id": "work-1"})),
    );
    event.turn_id = Some("turn-1".to_owned());
    DurableEventStore::open(&seed)?.append(event)?;

    for (column, change) in [
        ("checksum", "UPDATE events SET checksum = 'sha256:00'"),
        ("event_id", "UPDATE events SET event_id = 'event-forged'"),
        ("session_id", "UPDATE events SET session_id = 'session-2'"),
        ("turn_id", "UPDATE events SET turn_id = NULL"),
        ("kind", "UPDATE events SET kind = 'session.turn_completed'"),
        ("work_id", "UPDATE events SET work_id = 'work-2'"),
        ("schema_version", "UPDATE events SET schema_version = 2"),
        (
            "recorded_at",
            "UPDATE events SET recorded_at = '1970-01-01T00:00:00.000Z'",
        ),
    ] {
        let damaged = root.path().join(column);
        unopened_copy(&seed, &damaged)?;
        foreign_writer(&damaged)?.execute(change, [])?;

        let error = DurableEventStore::open(&damaged)
            .expect_err("a column that differs from the record must reject the store");
        let expected = if column == "checksum" {
            "checksum mismatch"
        } else {
            "do not match"
        };
        assert!(
            matches!(&error, DurableEventError::Corruption { offset: 1, reason } if reason.contains(expected)),
            "{column}: {error:?}"
        );
    }
    Ok(())
}

#[test]
fn durable_event_store_detects_gaps_and_misnumbered_sequences() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let seed = root.path().join("seed");
    let mut store = DurableEventStore::open(&seed)?;
    for message in ["one", "two", "three"] {
        store.append(input(SESSION_TURN_ACCEPTED, message))?;
    }
    drop(store);

    type Damage = fn(&Path) -> Result<(), Box<dyn Error>>;
    let cases: [(&str, Damage, u64, &str); 4] = [
        (
            "gap",
            |root| {
                foreign_writer(root)?.execute("DELETE FROM events WHERE sequence = 2", [])?;
                Ok(())
            },
            3,
            "expected sequence 2, found 3",
        ),
        (
            "renumbered",
            |root| {
                rewrite_record(root, 3, |record| record["sequence"] = json!(4))?;
                foreign_writer(root)?
                    .execute("UPDATE events SET sequence = 4 WHERE sequence = 3", [])?;
                Ok(())
            },
            4,
            "expected sequence 3, found 4",
        ),
        (
            "duplicate",
            |root| rewrite_record(root, 2, |record| record["sequence"] = json!(1)),
            2,
            "do not match",
        ),
        (
            "below_one",
            |root| {
                rewrite_record(root, 1, |record| record["sequence"] = json!(0))?;
                foreign_writer(root)?
                    .execute("UPDATE events SET sequence = 0 WHERE sequence = 1", [])?;
                Ok(())
            },
            0,
            "not positive",
        ),
    ];
    for (label, damage, offset, expected) in cases {
        let damaged = root.path().join(label);
        unopened_copy(&seed, &damaged)?;
        damage(&damaged)?;

        let error =
            DurableEventStore::open(&damaged).expect_err("invalid sequence must reject the store");
        assert!(
            matches!(&error, DurableEventError::Corruption { offset: found, reason } if *found == offset && reason.contains(expected)),
            "{label}: {error:?}"
        );
    }
    Ok(())
}

#[test]
fn durable_event_store_rejects_sql_updates_and_deletes() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let mut store = DurableEventStore::open(root.path())?;
    store.append(input(SESSION_TURN_ACCEPTED, "one"))?;
    store.append(input(SESSION_TURN_COMPLETED, "two"))?;
    let before = store.scan(10)?.records;

    let connection = Connection::open(store.path())?;
    for statement in [
        "UPDATE events SET kind = 'session.turn_failed' WHERE sequence = 1",
        "DELETE FROM events WHERE sequence = 2",
        "INSERT OR REPLACE INTO events (sequence, event_id, session_id, kind, schema_version, \
         recorded_at, record, checksum) VALUES (1, 'event-replacement', 'session-1', \
         'session.turn_failed', 1, 'replaced', '{}', 'sha256:replaced')",
        "INSERT OR REPLACE INTO events (sequence, event_id, session_id, kind, schema_version, \
         recorded_at, record, checksum) VALUES (3, 'event-00000000000000000002', 'session-1', \
         'session.turn_failed', 1, 'replaced', '{}', 'sha256:replaced')",
    ] {
        let error = connection
            .execute(statement, [])
            .expect_err("the store's triggers must reject the statement");
        assert!(
            error.to_string().contains("durable events are append-only"),
            "{error}"
        );
    }
    drop(connection);
    assert_eq!(store.scan(10)?.records, before);
    Ok(())
}

#[test]
fn durable_event_store_visits_nothing_after_the_largest_sequence() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let mut store = DurableEventStore::open(root.path())?;
    store.append(input(SESSION_TURN_ACCEPTED, "one"))?;
    store.append(input(SESSION_TURN_COMPLETED, "two"))?;

    let mut visited = 0;
    let summary = store.visit_from_sequence(u64::MAX, |_| visited += 1)?;

    assert_eq!(visited, 0);
    assert_eq!(summary.visited, 0);
    assert_eq!(summary.last_sequence, Some(2));
    Ok(())
}

fn wal_bytes(root: &Path) -> u64 {
    fs::metadata(sidecar(root, "-wal")).map_or(0, |metadata| metadata.len())
}

#[test]
fn durable_event_store_last_handle_empties_the_wal() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let mut store = DurableEventStore::open(root.path())?;
    let first = store.append(input(SESSION_TURN_ACCEPTED, "one"))?;
    let second = store.append(input(SESSION_TURN_COMPLETED, "two"))?;
    assert!(wal_bytes(root.path()) > 0);

    drop(store);

    assert_eq!(wal_bytes(root.path()), 0);
    let reopened = DurableEventStore::open(root.path())?;
    assert_eq!(reopened.scan(10)?.records, vec![first, second]);
    assert!(reopened.is_writable());
    Ok(())
}

#[test]
fn durable_event_store_last_handle_does_not_wait_for_a_reader() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let mut store = DurableEventStore::open(root.path())?;
    let record = store.append(input(SESSION_TURN_ACCEPTED, "one"))?;
    let reader = Connection::open(root.path().join(DURABLE_EVENT_DATABASE_FILE))?;
    reader.execute_batch("BEGIN")?;
    reader.query_row("SELECT count(*) FROM events", [], |row| {
        row.get::<_, i64>(0)
    })?;

    let started = Instant::now();
    drop(store);

    assert!(started.elapsed() < Duration::from_secs(2));
    assert!(wal_bytes(root.path()) > 0);
    reader.execute_batch("COMMIT")?;
    drop(reader);
    let mut reopened = DurableEventStore::open(root.path())?;
    assert_eq!(reopened.scan(10)?.records, vec![record]);
    assert_eq!(
        reopened
            .append(input(SESSION_TURN_COMPLETED, "two"))?
            .sequence,
        2
    );
    Ok(())
}

#[test]
fn durable_event_store_last_handle_leaves_replaced_files_untouched() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let event_root = root.path().join("events");
    let mut store = DurableEventStore::open(&event_root)?;
    store.append(input(SESSION_TURN_ACCEPTED, "kept"))?;
    assert!(wal_bytes(&event_root) > 0);
    let replacement = root.path().join("replacement");
    let mut other = DurableEventStore::open(root.path().join("other"))?;
    other.append(input(SESSION_TURN_ACCEPTED, "other"))?;
    unopened_copy(other.root(), &replacement)?;
    drop(other);
    fs::write(sidecar(&replacement, "-wal"), b"replacement wal")?;
    fs::write(sidecar(&replacement, "-shm"), b"replacement shm")?;
    for suffix in ["", "-wal", "-shm"] {
        fs::rename(sidecar(&replacement, suffix), sidecar(&event_root, suffix))?;
    }
    let before = snapshot(&event_root)?;

    drop(store);

    assert_eq!(snapshot(&event_root)?, before);
    Ok(())
}

#[test]
fn durable_event_store_last_handle_does_not_checkpoint_an_unsupported_version(
) -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let mut store = DurableEventStore::open(root.path())?;
    store.append(input(SESSION_TURN_ACCEPTED, "one"))?;
    fold_wal(root.path())?;
    Connection::open(store.path())?.pragma_update(None, "user_version", 2_i64)?;
    assert!(wal_bytes(root.path()) > 0);

    drop(store);

    assert!(wal_bytes(root.path()) > 0);
    let header = fs::read(root.path().join(DURABLE_EVENT_DATABASE_FILE))?;
    assert_eq!(header.get(60..64), Some(&1_u32.to_be_bytes()[..]));
    Ok(())
}

#[test]
fn durable_event_store_rechecks_every_event_of_a_replaced_database() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let event_root = root.path().join("events");
    let mut store = DurableEventStore::open(&event_root)?;
    store.append(input(SESSION_TURN_ACCEPTED, "safe"))?;
    store.append(input(SESSION_TURN_COMPLETED, "last"))?;
    let replacement = root.path().join("replacement");
    unopened_copy(&event_root, &replacement)?;
    patch_bytes(
        &replacement.join(DURABLE_EVENT_DATABASE_FILE),
        br#""message":"safe""#,
        br#""message":"evil""#,
    )?;

    fold_wal(&event_root)?;
    fs::rename(
        replacement.join(DURABLE_EVENT_DATABASE_FILE),
        event_root.join(DURABLE_EVENT_DATABASE_FILE),
    )?;

    let error = DurableEventStore::open(&event_root)
        .expect_err("a replaced database with the same last event must be verified again");
    assert!(
        matches!(&error, DurableEventError::Corruption { offset: 1, reason } if reason.contains("checksum")),
        "{error:?}"
    );
    Ok(())
}

#[test]
fn durable_event_store_rechecks_every_event_after_a_schema_change() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;

    let recreated_root = root.path().join("recreated");
    let mut store = DurableEventStore::open(&recreated_root)?;
    store.append(input(SESSION_TURN_ACCEPTED, "safe"))?;
    store.append(input(SESSION_TURN_COMPLETED, "last"))?;
    let writer = foreign_writer(&recreated_root)?;
    writer.execute(
        "UPDATE events SET record = replace(record, '\"safe\"', '\"evil\"') WHERE sequence = 1",
        [],
    )?;
    let trigger: String = writer.query_row(
        "SELECT sql FROM sqlite_master WHERE name = 'events_reject_delete'",
        [],
        |row| row.get(0),
    )?;
    writer.execute_batch(&format!("DROP TRIGGER events_reject_delete; {trigger};"))?;
    drop(writer);
    let error = DurableEventStore::open(&recreated_root)
        .expect_err("a schema change must verify every event again");
    assert!(
        matches!(&error, DurableEventError::Corruption { offset: 1, reason } if reason.contains("checksum")),
        "{error:?}"
    );

    for (label, extra_object) in [
        (
            "index",
            "CREATE INDEX events_by_recorded_at ON events (recorded_at)",
        ),
        ("lookalike", "CREATE TABLE sqliteX (value TEXT)"),
    ] {
        let extended_root = root.path().join(label);
        let mut store = DurableEventStore::open(&extended_root)?;
        store.append(input(SESSION_TURN_ACCEPTED, "one"))?;
        foreign_writer(&extended_root)?.execute(extra_object, [])?;
        let error = DurableEventStore::open(&extended_root)
            .expect_err("a schema that differs from the store's must reject the store");
        assert!(
            matches!(&error, DurableEventError::Corruption { offset: 0, reason } if reason.contains("schema")),
            "{label}: {error:?}"
        );
    }
    Ok(())
}

#[test]
fn durable_event_store_finds_a_changed_verified_event_when_reading_it_again(
) -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let mut store = DurableEventStore::open(root.path())?;
    store.append(input(SESSION_TURN_ACCEPTED, "safe"))?;
    store.append(input(SESSION_TURN_COMPLETED, "last"))?;
    foreign_writer(root.path())?.execute(
        "UPDATE events SET record = replace(record, '\"safe\"', '\"evil\"') WHERE sequence = 1",
        [],
    )?;

    let visit = store.visit_from_sequence(0, |_| {});
    assert!(
        matches!(&visit, Err(DurableEventError::Corruption { offset: 1, reason }) if reason.contains("checksum")),
        "{visit:?}"
    );
    let scan = store.scan(10);
    assert!(
        matches!(&scan, Err(DurableEventError::Corruption { offset: 1, reason }) if reason.contains("checksum")),
        "{scan:?}"
    );
    Ok(())
}

fn work_input(kind: &str, payload: &impl Serialize) -> Result<DurableEventInput, Box<dyn Error>> {
    Ok(DurableEventInput::new(
        "session-1",
        kind,
        DurableEventPayload::inline("durable_work", serde_json::to_value(payload)?),
    ))
}

#[test]
fn durable_event_store_scan_keeps_no_work_state_of_a_rewritten_last_event(
) -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let mut store = DurableEventStore::open(root.path())?;
    store.append(work_input(
        WORK_ENQUEUED,
        &WorkEnqueued {
            work_id: "work-1".to_owned(),
            work_kind: "test.work".to_owned(),
            payload_ref: WorkPayloadRef::inline("test.work.v1", json!({"value": 1}))?,
            dedupe_hint: None,
            next_wake_at_ms: None,
            effect_id: None,
        },
    )?)?;
    store.append(work_input(
        WORK_LEASED,
        &WorkLeased {
            work_id: "work-1".to_owned(),
            lease_id: "lease-1".to_owned(),
            lease_owner_ref: "owner-1".to_owned(),
            attempt: 1,
            leased_at_ms: 100,
            lease_expires_at_ms: 200,
        },
    )?)?;
    rewrite_record(root.path(), 2, |record| {
        record["kind"] = json!(SESSION_TURN_ACCEPTED)
    })?;

    store.scan(10)?;
    let terminal = store.append(work_input(
        WORK_TERMINAL,
        &WorkTerminal {
            work_id: "work-1".to_owned(),
            terminal_kind: WorkTerminalKind::Succeeded,
            outcome_ref: "done".to_owned(),
            facts: None,
        },
    )?);

    assert!(
        matches!(&terminal, Err(DurableEventError::Validation(reason)) if reason.contains("is invalid for work")),
        "{terminal:?}"
    );
    Ok(())
}

#[test]
fn durable_event_child_interrupted_append() -> Result<(), Box<dyn Error>> {
    let Ok(root) = std::env::var("SHACS_DURABLE_EVENT_INTERRUPTED_ROOT") else {
        return Ok(());
    };
    let connection = Connection::open(Path::new(&root).join(DURABLE_EVENT_DATABASE_FILE))?;
    connection.pragma_update(None, "cache_size", 2_i64)?;
    connection.execute_batch("BEGIN IMMEDIATE")?;
    let record = "x".repeat(16 * 1024);
    for sequence in 2..64_i64 {
        connection.execute(
            "INSERT INTO events (sequence, event_id, session_id, kind, schema_version, \
             recorded_at, record, checksum) VALUES (?1, ?2, 'session-1', ?3, 1, 'never', ?4, ?5)",
            params![
                sequence,
                format!("event-{sequence:020}"),
                SESSION_TURN_ACCEPTED,
                record,
                checksum(&record)
            ],
        )?;
    }
    // Ends inside the transaction, as a crash would: nothing is rolled back or closed.
    std::process::exit(0)
}

#[test]
fn durable_event_store_hides_an_interrupted_append() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let mut store = DurableEventStore::open(root.path())?;
    let committed = store.append(input(SESSION_TURN_ACCEPTED, "committed"))?;
    fold_wal(root.path())?;

    let status = Command::new(std::env::current_exe()?)
        .arg("--exact")
        .arg("durable_event_child_interrupted_append")
        .arg("--nocapture")
        .env("SHACS_DURABLE_EVENT_INTERRUPTED_ROOT", root.path())
        .status()?;
    assert!(status.success());
    assert!(
        fs::metadata(sidecar(root.path(), "-wal"))?.len() > 0,
        "the interrupted append must have reached the WAL"
    );

    let reopened = DurableEventStore::open(root.path())?;
    let scan = reopened.scan(10)?;
    assert_eq!(scan.records, vec![committed]);
    assert_eq!(scan.last_sequence, Some(1));
    assert!(!scan.incomplete_tail);
    assert!(reopened.is_writable());
    assert_eq!(
        store
            .append(input(SESSION_TURN_COMPLETED, "after"))?
            .sequence,
        2
    );
    Ok(())
}

#[test]
fn durable_event_store_returns_compatibility_for_unknown_schema_and_kind(
) -> Result<(), Box<dyn Error>> {
    let schema_root = tempfile::tempdir()?;
    let mut schema_store = DurableEventStore::open(schema_root.path())?;
    schema_store.append(input(SESSION_TURN_ACCEPTED, "schema"))?;
    rewrite_record(schema_root.path(), 1, |record| {
        record["schema_version"] = json!(2)
    })?;
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
    rewrite_record(kind_root.path(), 1, |record| {
        record["kind"] = json!("future.unknown")
    })?;
    let kind_store = DurableEventStore::open(kind_root.path())?;
    assert_eq!(
        kind_store.compatibility(),
        &DurableEventCompatibility::UnsupportedKind {
            kind: "future.unknown".to_owned(),
            schema_version: 1,
        }
    );
    assert!(!kind_store.is_writable());

    let version_root = tempfile::tempdir()?;
    let mut version_store = DurableEventStore::open(version_root.path())?;
    version_store.append(input(SESSION_TURN_ACCEPTED, "version"))?;
    Connection::open(version_root.path().join(DURABLE_EVENT_DATABASE_FILE))?.pragma_update(
        None,
        "user_version",
        2_i64,
    )?;
    let mut version_store = DurableEventStore::open(version_root.path())?;
    assert_eq!(
        version_store.compatibility(),
        &DurableEventCompatibility::UnsupportedFrameVersion { found: 2 }
    );
    assert!(!version_store.is_writable());
    assert!(matches!(
        version_store.append(input(SESSION_TURN_COMPLETED, "blocked")),
        Err(DurableEventError::ReadOnly(
            DurableEventCompatibility::UnsupportedFrameVersion { found: 2 }
        ))
    ));
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
    assert_eq!(store.scan(10)?.last_sequence, None);
    Ok(())
}

enum LegacyEntry {
    File,
    BrokenSymlink,
    Directory,
}

#[test]
fn durable_event_store_refuses_a_legacy_event_log_of_any_kind() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    for (name, entry) in [
        ("file", LegacyEntry::File),
        ("broken-symlink", LegacyEntry::BrokenSymlink),
        ("directory", LegacyEntry::Directory),
    ] {
        let event_root = root.path().join(name);
        fs::create_dir_all(&event_root)?;
        let legacy = event_root.join(LEGACY_DURABLE_EVENT_LOG_FILE);
        match entry {
            LegacyEntry::File => fs::write(&legacy, b"{\"frame_version\":1}\n")?,
            LegacyEntry::BrokenSymlink => {
                #[cfg(unix)]
                std::os::unix::fs::symlink(event_root.join("missing"), &legacy)?;
                #[cfg(not(unix))]
                continue;
            }
            LegacyEntry::Directory => {
                fs::create_dir(&legacy)?;
                fs::write(legacy.join("kept"), b"kept")?;
            }
        }
        let before = snapshot(&event_root)?;

        assert!(matches!(
            DurableEventStore::open(&event_root),
            Err(DurableEventError::LegacyLogPresent)
        ));
        assert!(matches!(
            DurableEventStore::open_existing(&event_root),
            Err(DurableEventError::LegacyLogPresent)
        ));
        assert!(matches!(
            DurableEventStore::logical_digest_existing(&event_root),
            Err(DurableEventError::LegacyLogPresent)
        ));
        assert_eq!(snapshot(&event_root)?, before, "{name}");
    }

    let both_root = root.path().join("both");
    fs::create_dir_all(&both_root)?;
    fs::write(both_root.join(LEGACY_DURABLE_EVENT_LOG_FILE), b"legacy")?;
    fs::write(sidecar(&both_root, "-wal"), b"left behind")?;
    assert!(matches!(
        DurableEventStore::open(&both_root),
        Err(DurableEventError::LegacyLogPresent)
    ));

    let later_root = root.path().join("later");
    let mut store = DurableEventStore::open(&later_root)?;
    store.append(input(SESSION_TURN_ACCEPTED, "before"))?;
    fs::write(later_root.join(LEGACY_DURABLE_EVENT_LOG_FILE), b"legacy")?;
    assert!(matches!(
        store.append(input(SESSION_TURN_COMPLETED, "after")),
        Err(DurableEventError::LegacyLogPresent)
    ));
    assert!(matches!(
        store.scan(10),
        Err(DurableEventError::LegacyLogPresent)
    ));
    Ok(())
}

#[test]
fn durable_event_store_refuses_sidecars_without_a_database() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    for suffix in ["-wal", "-shm"] {
        let event_root = root.path().join(format!("orphan{suffix}"));
        fs::create_dir_all(&event_root)?;
        fs::write(sidecar(&event_root, suffix), b"left behind")?;
        let before = snapshot(&event_root)?;

        assert!(matches!(
            DurableEventStore::open(&event_root),
            Err(DurableEventError::Corruption { offset: 0, .. })
        ));
        assert!(matches!(
            DurableEventStore::open_existing(&event_root),
            Err(DurableEventError::Corruption { offset: 0, .. })
        ));
        assert_eq!(snapshot(&event_root)?, before, "{suffix}");
    }
    Ok(())
}

#[test]
fn durable_event_store_reads_a_database_it_did_not_create_as_unsupported(
) -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let zero_bytes = root.path().join("zero-bytes");
    fs::create_dir_all(&zero_bytes)?;
    fs::write(zero_bytes.join(DURABLE_EVENT_DATABASE_FILE), b"")?;
    let empty_sqlite = root.path().join("empty-sqlite");
    fs::create_dir_all(&empty_sqlite)?;
    Connection::open(empty_sqlite.join(DURABLE_EVENT_DATABASE_FILE))?
        .execute_batch("CREATE TABLE unrelated (value TEXT); DROP TABLE unrelated;")?;

    for event_root in [zero_bytes, empty_sqlite] {
        let database = event_root.join(DURABLE_EVENT_DATABASE_FILE);
        let before = fs::read(&database)?;

        let mut store = DurableEventStore::open(&event_root)?;

        assert_eq!(
            store.compatibility(),
            &DurableEventCompatibility::UnsupportedFrameVersion { found: 0 }
        );
        assert!(matches!(
            store.append(input(SESSION_TURN_ACCEPTED, "refused")),
            Err(DurableEventError::ReadOnly(_))
        ));
        drop(store);
        assert_eq!(fs::read(&database)?, before, "{}", event_root.display());
    }
    Ok(())
}

#[test]
fn durable_event_store_creates_its_database_without_leaving_staging_files(
) -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let event_root = root.path().join("events");

    let store = DurableEventStore::open(&event_root)?;

    assert!(store.is_writable());
    drop(store);
    let leftovers: Vec<_> = fs::read_dir(&event_root)?
        .map(|entry| entry.map(|entry| entry.file_name()))
        .collect::<Result<_, _>>()?;
    assert!(
        leftovers.iter().all(|name| name
            .to_string_lossy()
            .starts_with(DURABLE_EVENT_DATABASE_FILE)),
        "{leftovers:?}"
    );
    Ok(())
}

#[test]
fn durable_event_store_judges_shared_rows_with_each_handles_registry() -> Result<(), Box<dyn Error>>
{
    let root = tempfile::tempdir()?;
    let mut registry = DurableEventSchemaRegistry::default();
    registry.register("custom.fact", 1);
    let mut custom = DurableEventStore::open_with_registry(root.path(), registry)?;
    custom.append(input("custom.fact", "known to the custom registry"))?;
    assert!(custom.is_writable());

    let mut default = DurableEventStore::open(root.path())?;

    assert_eq!(
        default.compatibility(),
        &DurableEventCompatibility::UnsupportedKind {
            kind: "custom.fact".to_owned(),
            schema_version: 1,
        }
    );
    assert!(matches!(
        default.append(input(SESSION_TURN_ACCEPTED, "refused")),
        Err(DurableEventError::ReadOnly(_))
    ));
    assert_eq!(
        custom
            .append(input(SESSION_TURN_ACCEPTED, "still writable"))?
            .sequence,
        2
    );
    Ok(())
}

#[test]
fn durable_event_store_digest_orders_the_rows_of_every_table_by_their_key(
) -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    DurableEventStore::open(root.path())?.append(input(SESSION_TURN_ACCEPTED, "one"))?;
    let connection = Connection::open(root.path().join(DURABLE_EVENT_DATABASE_FILE))?;
    connection.execute_batch(
        r#"PRAGMA user_version = 2;
         CREATE TABLE "odd ""name""" (a TEXT, b TEXT, value TEXT, PRIMARY KEY (b, a))
             WITHOUT ROWID;
         INSERT INTO "odd ""name""" VALUES ('1', 'y', 'first'), ('2', 'x', 'second'),
             ('1', 'x', 'third');
         CREATE TABLE sqliteX (value TEXT);
         INSERT INTO sqliteX VALUES ('kept');"#,
    )?;
    let digest = || DurableEventStore::logical_digest_existing(root.path());
    let baseline = digest()?;

    connection.execute_batch(
        r#"DELETE FROM "odd ""name""";
         INSERT INTO "odd ""name""" VALUES ('1', 'x', 'third'), ('2', 'x', 'second'),
             ('1', 'y', 'first');
         VACUUM;"#,
    )?;
    assert_eq!(digest()?, baseline);
    let mut seen = vec![baseline];
    for change in [
        "UPDATE sqliteX SET value = 'changed'",
        r#"UPDATE "odd ""name""" SET value = 'changed' WHERE a = '1' AND b = 'x'"#,
    ] {
        connection.execute(change, [])?;
        let changed = digest()?;
        assert!(!seen.contains(&changed), "{change}");
        seen.push(changed);
    }

    connection.execute_batch(
        "CREATE TABLE hidden (rowid TEXT, _rowid_ TEXT, oid TEXT);
         INSERT INTO hidden VALUES ('x', 'y', 'z');",
    )?;
    let hidden = digest();
    assert!(
        matches!(&hidden, Err(DurableEventError::Corruption { offset: 0, reason }) if reason.contains("no key")),
        "{hidden:?}"
    );
    Ok(())
}

#[test]
fn durable_event_store_open_existing_creates_nothing() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let missing = root.path().join("missing");
    assert!(DurableEventStore::open_existing(&missing)?.is_none());
    assert!(DurableEventStore::logical_digest_existing(&missing)?.is_none());
    assert!(!missing.exists());

    let empty = root.path().join("empty");
    fs::create_dir(&empty)?;
    assert!(DurableEventStore::open_existing(&empty)?.is_none());
    assert!(DurableEventStore::logical_digest_existing(&empty)?.is_none());
    assert_eq!(fs::read_dir(&empty)?.count(), 0);
    Ok(())
}

#[cfg(unix)]
#[test]
fn durable_event_store_keeps_admitted_store_files_private() -> Result<(), Box<dyn Error>> {
    use std::os::unix::fs::PermissionsExt;

    let mode = |path: &Path| -> Result<u32, Box<dyn Error>> {
        Ok(fs::metadata(path)?.permissions().mode() & 0o777)
    };
    let widen = |path: &Path| fs::set_permissions(path, fs::Permissions::from_mode(0o644));
    let root = tempfile::tempdir()?;

    let created = root.path().join("created");
    let mut store = DurableEventStore::open(&created)?;
    store.append(input(SESSION_TURN_ACCEPTED, "created"))?;
    for file in [
        created.join(DURABLE_EVENT_DATABASE_FILE),
        sidecar(&created, "-wal"),
        sidecar(&created, "-shm"),
    ] {
        assert_eq!(mode(&file)?, 0o600, "{}", file.display());
    }
    drop(store);

    let admitted = root.path().join("admitted");
    DurableEventStore::open(&admitted)?.append(input(SESSION_TURN_ACCEPTED, "admitted"))?;
    let reader = Connection::open(admitted.join(DURABLE_EVENT_DATABASE_FILE))?;
    reader.query_row("SELECT count(*) FROM events", [], |row| {
        row.get::<_, i64>(0)
    })?;
    let files = [
        admitted.join(DURABLE_EVENT_DATABASE_FILE),
        sidecar(&admitted, "-wal"),
        sidecar(&admitted, "-shm"),
    ];
    for file in &files {
        widen(file)?;
    }
    DurableEventStore::open(&admitted)?;
    for file in &files {
        assert_eq!(mode(file)?, 0o600, "{}", file.display());
    }
    drop(reader);

    let unsupported = root.path().join("unsupported");
    DurableEventStore::open(&unsupported)?.append(input(SESSION_TURN_ACCEPTED, "unsupported"))?;
    Connection::open(unsupported.join(DURABLE_EVENT_DATABASE_FILE))?.pragma_update(
        None,
        "user_version",
        2_i64,
    )?;
    widen(&unsupported.join(DURABLE_EVENT_DATABASE_FILE))?;
    assert!(!DurableEventStore::open(&unsupported)?.is_writable());
    assert_eq!(mode(&unsupported.join(DURABLE_EVENT_DATABASE_FILE))?, 0o644);
    Ok(())
}

#[derive(Serialize, Deserialize)]
struct ForeignInput {
    session_id: String,
    kind: String,
    payload: DurableEventPayload,
}

#[test]
fn durable_event_child_appends_inputs() -> Result<(), Box<dyn Error>> {
    let Ok(inputs) = std::env::var("SHACS_DURABLE_EVENT_CHILD_INPUTS") else {
        return Ok(());
    };
    let root = std::env::var("SHACS_DURABLE_EVENT_CHILD_INPUTS_ROOT")?;
    let inputs: Vec<ForeignInput> = serde_json::from_slice(&fs::read(inputs)?)?;
    let mut store = DurableEventStore::open(root)?;
    for input in inputs {
        store.append(DurableEventInput::new(
            input.session_id,
            input.kind,
            input.payload,
        ))?;
    }
    Ok(())
}

/// Appends `inputs` from another process, so that no cache of this process sees them written.
fn append_in_another_process(
    root: &Path,
    inputs: &[DurableEventInput],
) -> Result<(), Box<dyn Error>> {
    let file = tempfile::NamedTempFile::new()?;
    let inputs: Vec<ForeignInput> = inputs
        .iter()
        .map(|input| ForeignInput {
            session_id: input.session_id.clone(),
            kind: input.kind.clone(),
            payload: input.payload.clone(),
        })
        .collect();
    fs::write(file.path(), serde_json::to_vec(&inputs)?)?;
    let status = Command::new(std::env::current_exe()?)
        .arg("--exact")
        .arg("durable_event_child_appends_inputs")
        .arg("--nocapture")
        .env("SHACS_DURABLE_EVENT_CHILD_INPUTS", file.path())
        .env("SHACS_DURABLE_EVENT_CHILD_INPUTS_ROOT", root)
        .status()?;
    if !status.success() {
        return Err("the other process could not append its events".into());
    }
    Ok(())
}

fn work_event_in(
    session_id: &str,
    kind: &str,
    payload: &impl Serialize,
) -> Result<DurableEventInput, Box<dyn Error>> {
    Ok(DurableEventInput::new(
        session_id,
        kind,
        DurableEventPayload::inline("durable_work", serde_json::to_value(payload)?),
    ))
}

fn enqueued_in(session_id: &str, work_id: &str) -> Result<DurableEventInput, Box<dyn Error>> {
    work_event_in(
        session_id,
        WORK_ENQUEUED,
        &WorkEnqueued {
            work_id: work_id.to_owned(),
            work_kind: "test.work".to_owned(),
            payload_ref: WorkPayloadRef::inline("test.work.v1", json!({"work": work_id}))?,
            dedupe_hint: None,
            next_wake_at_ms: None,
            effect_id: None,
        },
    )
}

fn leased_in(session_id: &str, work_id: &str) -> Result<DurableEventInput, Box<dyn Error>> {
    work_event_in(
        session_id,
        WORK_LEASED,
        &WorkLeased {
            work_id: work_id.to_owned(),
            lease_id: format!("lease-{work_id}"),
            lease_owner_ref: "owner-1".to_owned(),
            attempt: 1,
            leased_at_ms: 100,
            lease_expires_at_ms: 200,
        },
    )
}

fn succeeded_in(session_id: &str, work_id: &str) -> Result<DurableEventInput, Box<dyn Error>> {
    work_event_in(
        session_id,
        WORK_TERMINAL,
        &WorkTerminal {
            work_id: work_id.to_owned(),
            terminal_kind: WorkTerminalKind::Succeeded,
            outcome_ref: "done".to_owned(),
            facts: None,
        },
    )
}

fn stop_requested(requested_at_ms: u64) -> Result<DurableEventInput, Box<dyn Error>> {
    work_event_in(
        "runtime",
        RUNTIME_STOP_REQUESTED,
        &RuntimeControlRequested {
            requested_at_ms,
            request_id: Some(format!("stop-{requested_at_ms}")),
            target_owner_id: None,
        },
    )
}

fn fact_in(session_id: &str, kind: &str, data: Value) -> DurableEventInput {
    DurableEventInput::new(
        session_id,
        kind,
        DurableEventPayload::inline("test_fact", data),
    )
}

/// The work state of every current event, reduced here rather than by the store.
fn work_state_from_scratch(
    store: &DurableEventStore,
) -> Result<DurableWorkReplayState, Box<dyn Error>> {
    let mut state = DurableWorkReplayState::default();
    for record in store.scan(usize::MAX)?.records {
        apply_persisted_durable_work_event(&mut state, &record)?;
    }
    Ok(state)
}

fn selects(filter: &DurableEventFilter, record: &DurableEventRecord) -> bool {
    let work_id = match &record.payload {
        DurableEventPayload::Inline { data, .. } => data.get("work_id").and_then(Value::as_str),
        DurableEventPayload::Artifact { .. } => None,
    };
    filter
        .session_id
        .as_ref()
        .is_none_or(|session_id| *session_id == record.session_id)
        && filter
            .kinds
            .as_ref()
            .is_none_or(|kinds| kinds.contains(&record.kind))
        && filter.work_id_prefix.as_ref().is_none_or(|prefix| {
            work_id.is_some_and(|work_id| work_id.starts_with(prefix.as_str()))
        })
}

fn filter(
    session_id: Option<&str>,
    kinds: Option<&[&str]>,
    work_id_prefix: Option<&str>,
) -> DurableEventFilter {
    DurableEventFilter {
        session_id: session_id.map(str::to_owned),
        kinds: kinds.map(|kinds| kinds.iter().map(|kind| (*kind).to_owned()).collect()),
        work_id_prefix: work_id_prefix.map(str::to_owned),
    }
}

#[test]
fn durable_event_store_filtered_reads_visit_what_a_full_visit_selects() -> Result<(), Box<dyn Error>>
{
    let root = tempfile::tempdir()?;
    let mut store = DurableEventStore::open(root.path())?;
    let work_ids = [
        "web-watch:a:1",
        "web-watch:a:2",
        "web-watch:ab:1",
        "web-watch:a",
        "web-watch:b:1",
        "작업:가:1",
        "작업:가나:1",
        "작업:각",
        "z\u{10FFFF}\u{10FFFF}:1",
        "z\u{10FFFF}",
        "zz",
        "{",
        "\u{D7FF}x",
        "\u{E000}",
        "",
    ];
    for session_id in ["owner", "other"] {
        for kind in [SESSION_TURN_ACCEPTED, WORKFLOW_PLANNED] {
            for work_id in work_ids {
                store.append(fact_in(session_id, kind, json!({"work_id": work_id})))?;
            }
            store.append(fact_in(session_id, kind, json!({"work_id": 7})))?;
            store.append(fact_in(session_id, kind, json!({"other": "web-watch:a:1"})))?;
        }
        store.append(DurableEventInput::new(
            session_id,
            SESSION_TURN_COMPLETED,
            DurableEventPayload::artifact("tool_result", ".nanobot/tool-results/web-watch:a:1"),
        ))?;
    }
    store.append(enqueued_in("owner", "web-watch:a:3")?)?;
    store.append(leased_in("owner", "web-watch:a:3")?)?;
    store.append(succeeded_in("owner", "web-watch:a:3")?)?;
    let mut every = Vec::new();
    let full = store.visit_from_sequence(0, |record| every.push(record.clone()))?;

    let cases: Vec<Vec<DurableEventFilter>> = vec![
        vec![filter(Some("owner"), None, None)],
        vec![filter(None, Some(&[SESSION_TURN_ACCEPTED]), None)],
        vec![filter(None, None, Some("web-watch:a:"))],
        vec![filter(Some("owner"), None, Some("web-watch:a:"))],
        vec![filter(None, None, Some("작업:가"))],
        vec![filter(None, None, Some("z\u{10FFFF}"))],
        vec![filter(None, None, Some("\u{D7FF}"))],
        vec![filter(None, None, Some(""))],
        vec![filter(None, Some(&[]), None)],
        vec![],
        vec![filter(None, None, None)],
        vec![
            filter(Some("owner"), Some(&[SESSION_TURN_ACCEPTED]), None),
            filter(
                None,
                Some(&[SESSION_TURN_ACCEPTED, WORK_ENQUEUED, WORK_TERMINAL]),
                Some("web-watch:"),
            ),
        ],
    ];
    for filters in cases {
        let expected: Vec<DurableEventRecord> = every
            .iter()
            .filter(|record| filters.iter().any(|filter| selects(filter, record)))
            .cloned()
            .collect();
        let counted_before = store.decoded_event_count()?;
        let mut visited = Vec::new();

        let summary = store.visit_matching(&filters, |record| visited.push(record.clone()))?;

        assert_eq!(visited, expected, "{filters:?}");
        assert_eq!(summary.visited, expected.len(), "{filters:?}");
        assert_eq!(summary.decoded, expected.len() as u64, "{filters:?}");
        assert_eq!(
            store.decoded_event_count()? - counted_before,
            summary.decoded,
            "{filters:?}"
        );
        assert_eq!(summary.last_sequence, full.last_sequence, "{filters:?}");
        assert_eq!(summary.compatibility, DurableEventCompatibility::Current);
    }
    Ok(())
}

#[test]
fn durable_event_store_filtered_reads_report_damage_and_unsupported_rows_they_select(
) -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let mut registry = DurableEventSchemaRegistry::default();
    registry.register("custom.fact", 1);
    let mut custom = DurableEventStore::open_with_registry(root.path(), registry)?;
    custom.append(fact_in("owner", "custom.fact", json!({"work_id": "w:1"})))?;
    custom.append(fact_in(
        "owner",
        SESSION_TURN_ACCEPTED,
        json!({"message": "safe"}),
    ))?;
    custom.append(fact_in(
        "other",
        SESSION_TURN_ACCEPTED,
        json!({"work_id": "w:2"}),
    ))?;
    custom.append(fact_in(
        "other",
        SESSION_TURN_COMPLETED,
        json!({"work_id": "w:3"}),
    ))?;
    let store = DurableEventStore::open(root.path())?;

    let mut visited = Vec::new();
    let summary = store.visit_matching(&[filter(Some("owner"), None, None)], |record| {
        visited.push(record.sequence)
    })?;
    assert_eq!(visited, vec![2]);
    assert_eq!(
        summary.compatibility,
        DurableEventCompatibility::UnsupportedKind {
            kind: "custom.fact".to_owned(),
            schema_version: 1,
        }
    );

    foreign_writer(root.path())?.execute(
        "UPDATE events SET record = replace(record, '\"safe\"', '\"evil\"') WHERE sequence = 2",
        [],
    )?;
    let damaged = store.visit_matching(&[filter(Some("owner"), None, None)], |_| {});
    assert!(
        matches!(&damaged, Err(DurableEventError::Corruption { offset: 2, reason }) if reason.contains("checksum")),
        "{damaged:?}"
    );
    let other = DurableEventStore::open(root.path());
    assert!(
        matches!(&other, Err(DurableEventError::Corruption { offset: 2, .. })),
        "a failed read leaves no verification to reuse: {other:?}"
    );
    Ok(())
}

#[test]
fn durable_event_store_catches_up_work_another_process_wrote() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let mut store = DurableEventStore::open(root.path())?;
    for index in 0..20 {
        store.append(enqueued_in("session-1", &format!("work-{index}"))?)?;
        store.append(input(SESSION_TURN_ACCEPTED, "unrelated"))?;
    }
    store.work_state()?;
    let counted_before = store.decoded_event_count()?;

    append_in_another_process(
        root.path(),
        &[
            leased_in("session-1", "work-3")?,
            input(SESSION_TURN_COMPLETED, "unrelated"),
            succeeded_in("session-1", "work-3")?,
            stop_requested(500)?,
            enqueued_in("session-2", "work-new")?,
        ],
    )?;
    let caught_up = store.work_state()?;
    let decoded = store.decoded_event_count()? - counted_before;

    assert_eq!(decoded, 5 + 4, "the new events, then the new work events");
    assert_eq!(caught_up, work_state_from_scratch(&store)?);
    assert_eq!(caught_up.runtime_requests.len(), 1);
    assert!(caught_up.items.contains_key("work-new"));
    Ok(())
}

#[test]
fn durable_event_store_rebuilds_work_state_of_a_replaced_database() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let event_root = root.path().join("events");
    let mut store = DurableEventStore::open(&event_root)?;
    store.append(enqueued_in("session-1", "work-original")?)?;
    store.append(input(SESSION_TURN_ACCEPTED, "unrelated"))?;
    store.work_state()?;
    let replacement_source = root.path().join("source");
    let mut source = DurableEventStore::open(&replacement_source)?;
    source.append(enqueued_in("session-1", "work-replaced")?)?;
    source.append(leased_in("session-1", "work-replaced")?)?;
    source.append(input(SESSION_TURN_ACCEPTED, "unrelated"))?;
    drop(source);
    let replacement = root.path().join("replacement");
    unopened_copy(&replacement_source, &replacement)?;
    fold_wal(&event_root)?;
    fs::rename(
        replacement.join(DURABLE_EVENT_DATABASE_FILE),
        event_root.join(DURABLE_EVENT_DATABASE_FILE),
    )?;
    let counted_before = store.decoded_event_count()?;

    let rebuilt = store.work_state()?;

    assert_eq!(
        store.decoded_event_count()? - counted_before,
        3 + 2,
        "every event, then every work event"
    );
    assert!(rebuilt.items.contains_key("work-replaced"));
    assert!(!rebuilt.items.contains_key("work-original"));
    assert_eq!(rebuilt, work_state_from_scratch(&store)?);
    Ok(())
}

#[test]
fn durable_event_store_keeps_its_work_state_through_appends_of_other_kinds(
) -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let mut store = DurableEventStore::open(root.path())?;
    store.append(enqueued_in("session-1", "work-1")?)?;
    store.work_state()?;
    store.append(stop_requested(700)?)?;
    store.append(input(SESSION_TURN_ACCEPTED, "unrelated"))?;
    let counted_before = store.decoded_event_count()?;

    let state = store.work_state()?;

    assert_eq!(store.decoded_event_count()?, counted_before);
    assert_eq!(state.runtime_requests.len(), 1);
    assert_eq!(state, work_state_from_scratch(&store)?);
    Ok(())
}

#[test]
fn durable_event_store_normalizes_an_append_with_work_another_process_wrote(
) -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let mut store = DurableEventStore::open(root.path())?;
    store.append(enqueued_in("session-1", "work-1")?)?;
    store.append(leased_in("session-1", "work-1")?)?;
    store.work_state()?;
    append_in_another_process(
        root.path(),
        &[work_event_in(
            "session-1",
            WORK_CANCEL_REQUESTED,
            &WorkCancellation {
                work_id: "work-1".to_owned(),
                reason: "user_stop".to_owned(),
            },
        )?],
    )?;

    let recorded = store.append(succeeded_in("session-1", "work-1")?)?;

    assert_eq!(recorded.kind, WORK_CANCELLED);
    assert_eq!(store.work_state()?, work_state_from_scratch(&store)?);
    Ok(())
}

const WATCH_ROWS: usize = 50;
const NEW_ROWS: usize = 10;

/// A store with one watch of [`WATCH_ROWS`] events among `unrelated` events of other sessions
/// and kinds, each opened handle kept so that the process cache stays warm.
fn watched_store(root: &Path, unrelated: usize) -> Result<DurableEventStore, Box<dyn Error>> {
    let mut store = DurableEventStore::open(root)?;
    for index in 0..unrelated {
        store.append(fact_in(
            &format!("unrelated-{}", index % 7),
            if index % 2 == 0 {
                SESSION_TURN_ACCEPTED
            } else {
                WORKFLOW_PLANNED
            },
            json!({"work_id": format!("unrelated:{index}")}),
        ))?;
    }
    for index in 0..WATCH_ROWS / 2 {
        let work_id = format!("web-watch:watch-1:{index}");
        store.append(enqueued_in("owner", &work_id)?)?;
        store.append(leased_in("owner", &work_id)?)?;
    }
    store.work_state()?;
    Ok(store)
}

fn new_work_rows(batch: usize) -> Result<Vec<DurableEventInput>, Box<dyn Error>> {
    (0..NEW_ROWS)
        .map(|index| enqueued_in("worker", &format!("new-{batch}-{index}")))
        .collect()
}

fn decoded_after_new_rows(
    store: &DurableEventStore,
    root: &Path,
    batch: usize,
    operation: impl FnOnce() -> Result<(), Box<dyn Error>>,
) -> Result<u64, Box<dyn Error>> {
    append_in_another_process(root, &new_work_rows(batch)?)?;
    let counted_before = store.decoded_event_count()?;
    operation()?;
    Ok(store.decoded_event_count()? - counted_before)
}

#[test]
fn durable_event_store_decodes_the_same_rows_however_many_unrelated_rows_exist(
) -> Result<(), Box<dyn Error>> {
    let watch = [filter(Some("owner"), None, Some("web-watch:watch-1:"))];
    let mut decoded = Vec::new();
    for unrelated in [0, 300] {
        let root = tempfile::tempdir()?;
        let store = watched_store(root.path(), unrelated)?;
        let mut enqueuer = DurableWorkEnqueuer::open(root.path())?;

        let work_state = decoded_after_new_rows(&store, root.path(), 0, || {
            store.work_state()?;
            Ok(())
        })?;
        let watch_read = decoded_after_new_rows(&store, root.path(), 1, || {
            let mut visited = 0;
            store.visit_matching(&watch, |_| visited += 1)?;
            assert_eq!(visited, WATCH_ROWS);
            Ok(())
        })?;
        let enqueue = decoded_after_new_rows(&store, root.path(), 2, || {
            enqueuer.enqueue(DurableWorkEnqueueInput {
                work_id: "enqueued".to_owned(),
                work_kind: "test.work".to_owned(),
                session_key: "owner".to_owned(),
                turn_id: None,
                effect_id: None,
                payload_ref: WorkPayloadRef::inline("test.work.v1", json!({"value": 1}))?,
                dedupe_hint: None,
                next_wake_at_ms: None,
            })?;
            Ok(())
        })?;
        decoded.push((work_state, watch_read, enqueue));
    }

    let rows = NEW_ROWS as u64;
    assert_eq!(
        decoded[0],
        (2 * rows, rows + WATCH_ROWS as u64, 3 * rows),
        "each operation verifies the new events; the enqueue also catches up the work events \
         that the watch read verified but did not reduce"
    );
    assert_eq!(decoded[1], decoded[0]);
    Ok(())
}

#[test]
fn durable_event_store_forgets_its_verification_once_a_decided_append_finds_damage(
) -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let mut store = DurableEventStore::open(root.path())?;
    let mut enqueuer = DurableWorkEnqueuer::open(root.path())?;
    let work = |work_id: &str| -> Result<DurableWorkEnqueueInput, Box<dyn Error>> {
        Ok(DurableWorkEnqueueInput {
            work_id: work_id.to_owned(),
            work_kind: "test.work".to_owned(),
            session_key: "session-1".to_owned(),
            turn_id: None,
            effect_id: None,
            payload_ref: WorkPayloadRef::inline("test.work.v1", json!({"value": work_id}))?,
            dedupe_hint: None,
            next_wake_at_ms: None,
        })
    };
    enqueuer.enqueue(work("work-1")?)?;
    enqueuer.enqueue(work("work-2")?)?;
    store.append(input(SESSION_TURN_ACCEPTED, "after the work"))?;
    store.work_state()?;
    foreign_writer(root.path())?.execute(
        "UPDATE events SET record = replace(record, '\"work-1\"', '\"work-x\"') WHERE sequence = 1",
        [],
    )?;

    let duplicate = enqueuer.enqueue(work("work-1")?);
    let next = store.append(input(SESSION_TURN_COMPLETED, "after the damage"));
    let reopened = DurableEventStore::open(root.path());

    assert!(
        matches!(
            &duplicate,
            Err(DurableWorkEnqueueError::Event(
                DurableEventError::Corruption { offset: 1, .. }
            ))
        ),
        "{duplicate:?}"
    );
    assert!(
        matches!(&next, Err(DurableEventError::Corruption { offset: 1, .. })),
        "{next:?}"
    );
    assert!(
        matches!(
            &reopened,
            Err(DurableEventError::Corruption { offset: 1, .. })
        ),
        "{reopened:?}"
    );
    Ok(())
}
