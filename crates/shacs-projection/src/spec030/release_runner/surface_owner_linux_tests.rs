use super::model::{Spec030ReleaseArtifactError, Spec030SurfaceOwnerShutdown};
use super::surface_owner::{ephemeral_port, ProductionOwner};
use super::surface_owner_tests::{runner_config, temp_path};
use super::surface_runner::{
    capture_tui_no_session, capture_tui_runtime, prepare, CONTROLLED_EXEC_CALL_ID,
};
use crate::release_evidence::EvidenceWriter;
use serde_json::Value;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::Path;
use std::process::Command;
use std::time::Duration;

const OWNER_TIMEOUT: Duration = Duration::from_secs(20);

#[cfg(target_os = "linux")]
#[test]
fn linux_bwrap_owner_lifecycle_reports_active_facts_and_reaps(
) -> Result<(), Box<dyn std::error::Error>> {
    if std::env::var_os("SHACS_REQUIRE_BWRAP").is_none() {
        return Ok(());
    }
    assert!(Command::new("bwrap").arg("--version").status()?.success());
    let evidence_root = temp_path("linux-lifecycle");
    let writer = EvidenceWriter::open_new_run(&evidence_root)?;
    let mut config = runner_config(evidence_root.clone())?;
    config.command_timeout = OWNER_TIMEOUT;
    let port = ephemeral_port()?;
    prepare(&config, &writer, port)?;

    let mut owner = ProductionOwner::start(&config, port)?;
    let owner_pid = owner.owner_pid();
    owner.wait_until_ready(OWNER_TIMEOUT)?;
    assert!(!evidence_root.join("surface/workspace/sessions").exists());
    let tui = capture_tui_no_session(&config, &writer)?;
    assert_eq!(tui.id, "surface-tui-no-session");
    assert!(
        std::fs::read_to_string(evidence_root.join("surface/tui-no-session.txt"))?
            .contains("status: no sessions")
    );
    owner.exercise_runtime(&config, OWNER_TIMEOUT)?;
    let runtime_tui = capture_tui_runtime(&config, &writer)?;
    assert_eq!(runtime_tui.id, "surface-tui-runtime");
    let runtime_tui = std::fs::read_to_string(evidence_root.join("surface/tui-runtime.txt"))?;
    assert!(!runtime_tui.contains("status: no sessions"));
    assert!(runtime_tui
        .contains("process: adapter=bash support=supported controlScope=controlledChild"));
    assert!(runtime_tui.contains("credential: availability=available status=resolved"));
    assert!(runtime_tui.contains("sandbox: availability=available status=active"));
    let session = std::fs::read_to_string(
        evidence_root.join("surface/workspace/sessions/api_default-7c9f551ea20b.jsonl"),
    )?;
    assert!(session.contains(CONTROLLED_EXEC_CALL_ID));
    assert!(session.contains("Exit code: 0"));
    assert!(session.contains(
        evidence_root
            .join("surface/workspace")
            .to_string_lossy()
            .as_ref()
    ));
    let projection = trusted_runtime_projection(port)?;

    assert_eq!(projection["profile"]["workspaceTrust"], "userAsserted");
    assert_eq!(
        projection["profile"]["resourceTrust"],
        "explicitOrTrustedWorkspace"
    );
    assert_eq!(projection["hooks"]["status"], "active");
    assert!(projection["hooks"]["registeredHandlers"]
        .as_u64()
        .is_some_and(|count| count > 0));
    let process_adapters = projection["processAdapters"]
        .as_array()
        .ok_or("process adapters must be an array")?;
    assert!(
        process_adapters.iter().any(|adapter| {
            adapter["adapter"] == "bash"
                && adapter["support"] == "supported"
                && adapter["capabilities"]["timeout"] == true
                && adapter["capabilities"]["abort"] == true
                && adapter["capabilities"]["cwd"] == true
                && adapter["capabilities"]["boundedOutput"] == true
                && adapter["capabilities"]["descendantCleanup"] == true
                && adapter["recentOutcomes"]
                    .as_array()
                    .is_some_and(|outcomes| {
                        outcomes.iter().any(|outcome| {
                            outcome["outcome"] == "succeeded"
                                && outcome["outputTruncated"] == false
                                && outcome["durationMs"].as_u64().is_some()
                        })
                    })
        }),
        "unexpected process adapters: {process_adapters:?}"
    );
    assert!(process_adapters.iter().any(|adapter| {
        adapter["support"] == "supported" && adapter["capabilities"]["startupReadiness"] == true
    }));
    assert_eq!(projection["credential"]["status"], "resolved");
    assert_eq!(projection["credential"]["source"], "environment");
    assert_eq!(projection["sandbox"]["status"], "active");
    assert_eq!(projection["sandbox"]["filesystemPolicy"], "applied");
    assert_eq!(projection["sandbox"]["networkPolicy"], "applied");
    assert!(projection["sandbox"]["appliedAdapters"]
        .as_array()
        .is_some_and(|adapters| adapters.iter().any(|adapter| adapter == "bash")));
    assert!(projection["resources"]
        .as_array()
        .is_some_and(|resources| !resources.is_empty()));
    assert!(projection["disclosure"]["surfaces"]
        .as_array()
        .is_some_and(|surfaces| surfaces.iter().any(|surface| surface == "trace")));
    assert_eq!(projection["disclosure"]["trace"]["status"], "enabled");
    assert_eq!(
        projection["disclosure"]["trace"]["preview"]["destination"],
        "localOnly"
    );

    let evidence = owner.stop(&config, &writer)?;

    assert_eq!(evidence.shutdown, Spec030SurfaceOwnerShutdown::Reaped);
    assert!(!process_exists(owner_pid));
    assert!(!evidence_root.join("surface/owner-tmp").exists());
    std::fs::remove_dir_all(evidence_root)?;
    Ok(())
}

#[cfg(target_os = "linux")]
#[test]
fn linux_bwrap_owner_lifecycle_exercise_failure_reaps_and_removes_temp(
) -> Result<(), Box<dyn std::error::Error>> {
    if std::env::var_os("SHACS_REQUIRE_BWRAP").is_none() {
        return Ok(());
    }
    let evidence_root = temp_path("linux-exercise-failure");
    let writer = EvidenceWriter::open_new_run(&evidence_root)?;
    let config = runner_config(evidence_root.clone())?;
    let port = ephemeral_port()?;
    prepare(&config, &writer, port)?;
    let mut owner = ProductionOwner::start(&config, port)?;
    let owner_pid = owner.owner_pid();
    owner.wait_until_ready(OWNER_TIMEOUT)?;
    std::fs::write(
        evidence_root.join("surface/provider-responses.json"),
        br#"[{"content":"done","finish_reason":"stop"}]"#,
    )?;

    let error = owner
        .exercise_runtime(&config, OWNER_TIMEOUT)
        .expect_err("HTTP 200 without controlled execution facts must fail");
    drop(owner);

    assert_eq!(error, Spec030ReleaseArtifactError::InvalidSurfaceEvidence);
    let session = std::fs::read_to_string(
        evidence_root.join("surface/workspace/sessions/api_default-7c9f551ea20b.jsonl"),
    )?;
    assert!(!session.contains("Exit code: 0"));
    assert!(!process_exists(owner_pid));
    assert!(!evidence_root.join("surface/owner-tmp").exists());
    std::fs::remove_dir_all(evidence_root)?;
    Ok(())
}

#[cfg(target_os = "linux")]
#[test]
fn linux_bwrap_owner_lifecycle_spawn_failure_removes_temp() -> Result<(), Box<dyn std::error::Error>>
{
    use std::os::unix::fs::PermissionsExt;

    if std::env::var_os("SHACS_REQUIRE_BWRAP").is_none() {
        return Ok(());
    }
    let evidence_root = temp_path("linux-spawn-failure");
    let writer = EvidenceWriter::open_new_run(&evidence_root)?;
    let mut config = runner_config(evidence_root.clone())?;
    let fake_repo = temp_path("linux-non-executable");
    let executable = fake_repo.join("crates/target/debug/shacs-bot");
    std::fs::create_dir_all(executable.parent().ok_or("missing executable parent")?)?;
    std::fs::write(&executable, b"not executable")?;
    std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o644))?;
    config.repo_root = fake_repo.clone();
    prepare(&config, &writer, ephemeral_port()?)?;

    let error = match ProductionOwner::start(&config, ephemeral_port()?) {
        Ok(owner) => {
            drop(owner);
            return Err("non-executable owner binary unexpectedly spawned".into());
        }
        Err(error) => error,
    };

    assert_eq!(error, Spec030ReleaseArtifactError::Io);
    assert!(!evidence_root.join("surface/owner-tmp").exists());
    std::fs::remove_dir_all(evidence_root)?;
    std::fs::remove_dir_all(fake_repo)?;
    Ok(())
}

#[cfg(target_os = "linux")]
fn trusted_runtime_projection(port: u16) -> Result<Value, Box<dyn std::error::Error>> {
    let address = SocketAddr::from(([127, 0, 0, 1], port));
    let mut stream = TcpStream::connect_timeout(&address, OWNER_TIMEOUT)?;
    stream.set_read_timeout(Some(OWNER_TIMEOUT))?;
    stream.write_all(
        format!(
            "GET /v1/trusted-runtime?schema_version=1 HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n"
        )
        .as_bytes(),
    )?;
    let mut response = Vec::new();
    stream.read_to_end(&mut response)?;
    let body = response
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .map(|index| &response[index + 4..])
        .ok_or("owner response omitted HTTP body")?;
    Ok(serde_json::from_slice(body)?)
}

#[cfg(target_os = "linux")]
fn process_exists(pid: u32) -> bool {
    Path::new(&format!("/proc/{pid}")).exists()
}
