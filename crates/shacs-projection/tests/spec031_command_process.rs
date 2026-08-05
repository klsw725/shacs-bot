use shacs_projection::{
    execute_spec031_release_command, Spec031ReleaseCommandSpec, Spec031ReleaseCommandStatus,
    Spec031ReleaseGateKind,
};
use std::fs;
use std::time::Duration;

#[test]
fn spec031_release_command_timeout_kills_term_ignoring_process_group(
) -> Result<(), Box<dyn std::error::Error>> {
    let output = temp_path("term-ignoring-descendant-timeout");
    fs::create_dir_all(&output)?;
    let marker = output.join("term-ignoring-child.pid");
    let record = execute_spec031_release_command(
        &Spec031ReleaseCommandSpec {
            id: "term_ignoring_descendant_timeout".to_owned(),
            gate: Spec031ReleaseGateKind::FailureInjection,
            package: None,
            filter: None,
            argv: vec![
                "sh".to_owned(),
                "-c".to_owned(),
                "trap '' TERM; sleep 5 & printf '%s' \"$!\" > \"$1\"; wait".to_owned(),
                "sh".to_owned(),
                marker.display().to_string(),
            ],
            cwd: std::env::temp_dir(),
            timeout: Duration::from_millis(50),
        },
        &output,
    )?;

    assert_eq!(record.status, Spec031ReleaseCommandStatus::TimedOut);
    let child_pid = fs::read_to_string(marker)?.parse::<u32>()?;
    std::thread::sleep(Duration::from_millis(100));
    assert_process_terminated(child_pid);
    Ok(())
}

fn assert_process_terminated(pid: u32) {
    if process_alive(pid) {
        cleanup_process(pid);
        panic!("process {pid} survived timeout cleanup");
    }
}

#[cfg(unix)]
fn cleanup_process(pid: u32) {
    let _ = std::process::Command::new("kill")
        .arg("-KILL")
        .arg(pid.to_string())
        .status();
}

#[cfg(not(unix))]
fn cleanup_process(_pid: u32) {}

#[cfg(unix)]
fn process_alive(pid: u32) -> bool {
    std::process::Command::new("kill")
        .arg("-0")
        .arg(pid.to_string())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

#[cfg(not(unix))]
fn process_alive(_pid: u32) -> bool {
    false
}

fn temp_path(label: &str) -> std::path::PathBuf {
    std::env::temp_dir()
        .canonicalize()
        .unwrap_or_else(|_| std::env::temp_dir())
        .join(format!(
            "shacs-spec031-command-process-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |duration| duration.as_nanos())
        ))
}
