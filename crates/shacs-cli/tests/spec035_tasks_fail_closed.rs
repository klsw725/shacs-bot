use serde_json::json;
use shacs_session::durable_event::{
    DurableEventInput, DurableEventPayload, DurableEventStore, SESSION_TURN_ACCEPTED,
};
use shacs_session::durable_replay::{evaluate_durable_recovery, DurableCheckpointStore};
use shacs_session::{Session, SessionManager};
use std::path::Path;
use std::process::Command;

#[test]
fn healthy_recovery_without_advertised_action_exits_nonzero(
) -> Result<(), Box<dyn std::error::Error>> {
    // Given
    let root = task_fixture(false)?;
    let before = owner_bytes(root.path())?;

    // When
    let output = recover_command_with_supported_hello(root.path()).output()?;

    // Then
    assert!(!output.status.success());
    assert!(String::from_utf8(output.stderr)?.contains("tasks action is not advertised"));
    assert_eq!(owner_bytes(root.path())?, before);
    Ok(())
}

#[test]
fn recoverable_recovery_without_advertised_action_exits_nonzero(
) -> Result<(), Box<dyn std::error::Error>> {
    // Given
    let root = task_fixture(true)?;
    let before = owner_bytes(root.path())?;

    // When
    let output = recover_command_with_supported_hello(root.path()).output()?;

    // Then
    assert!(!output.status.success());
    assert!(String::from_utf8(output.stderr)?.contains("tasks action is not advertised"));
    assert_eq!(owner_bytes(root.path())?, before);
    Ok(())
}

#[test]
fn recovery_without_transport_hello_fails_before_owner_validation(
) -> Result<(), Box<dyn std::error::Error>> {
    // Given
    let root = task_fixture(false)?;
    let before = owner_bytes(root.path())?;

    // When
    let output = recover_command(root.path()).output()?;

    // Then
    assert!(!output.status.success());
    assert_eq!(
        String::from_utf8(output.stderr)?,
        "unsupported: capability_unavailable\n"
    );
    assert_eq!(owner_bytes(root.path())?, before);
    Ok(())
}

fn task_fixture(recoverable: bool) -> Result<tempfile::TempDir, Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let workspace = root.path().join("workspace");
    let mut manager = SessionManager::new(&workspace)?;
    manager.save(&Session::new("cli:direct"))?;
    let event_root = root.path().join("runtime/durable-events");
    let checkpoint_root = root.path().join("runtime/durable-checkpoints");
    let mut events = DurableEventStore::open(&event_root)?;
    if recoverable {
        let mut input = DurableEventInput::new(
            "cli:direct",
            SESSION_TURN_ACCEPTED,
            DurableEventPayload::inline(
                "orchestrator_fact",
                json!({"content_hash":"sha256:one","media_count":0}),
            ),
        );
        input.turn_id = Some("turn-1".to_owned());
        events.append(input)?;
        let state = evaluate_durable_recovery(&event_root, &checkpoint_root)
            .state
            .ok_or("missing replay state")?;
        let checkpoints = DurableCheckpointStore::open(&checkpoint_root)?;
        checkpoints.write(&state)?;
        let checkpoint = checkpoints
            .candidate_paths()?
            .into_iter()
            .next()
            .ok_or("missing checkpoint")?;
        std::fs::write(checkpoint, b"{malformed")?;
    }
    Ok(root)
}

fn recover_command(root: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_shacs-bot"));
    command.args([
        "tasks",
        "--json",
        "--workspace",
        &root.join("workspace").display().to_string(),
        "--data-dir",
        &root.display().to_string(),
        "--owner",
        "recovery",
        "--action",
        "recover",
        "--locator",
        "runtime:recovery",
    ]);
    command
}

fn recover_command_with_supported_hello(root: &Path) -> Command {
    let mut command = recover_command(root);
    command.args([
        "--transport-hello",
        r#"{"client_id":"client:cli-fail-closed","schema_versions":[1],"mutation_capabilities":["task_recover"]}"#,
    ]);
    command
}

fn owner_bytes(root: &Path) -> Result<Vec<Vec<u8>>, Box<dyn std::error::Error>> {
    let events = std::fs::read(root.join("runtime/durable-events/events.log"))?;
    let checkpoints = DurableCheckpointStore::open(root.join("runtime/durable-checkpoints"))?;
    let mut bytes = vec![events];
    for path in checkpoints.candidate_paths()? {
        bytes.push(std::fs::read(path)?);
    }
    Ok(bytes)
}
