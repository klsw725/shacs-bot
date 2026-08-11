use super::model::Spec031ReleaseArtifactError;
use std::process::{Child, Command, ExitStatus};
use std::time::{Duration, Instant};

pub(super) fn wait_status_with_timeout(
    child: &mut Child,
    timeout: Duration,
) -> Result<(bool, ExitStatus), Spec031ReleaseArtifactError> {
    let start = Instant::now();
    while start.elapsed() < timeout {
        if let Some(status) = child
            .try_wait()
            .map_err(|_| Spec031ReleaseArtifactError::Io)?
        {
            return Ok((false, status));
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    terminate_process_tree(child)?;
    let status = child.wait().map_err(|_| Spec031ReleaseArtifactError::Io)?;
    Ok((true, status))
}

fn terminate_process_tree(child: &mut Child) -> Result<(), Spec031ReleaseArtifactError> {
    terminate_process_group(child.id())?;
    let start = Instant::now();
    while start.elapsed() < Duration::from_millis(250) {
        if child
            .try_wait()
            .map_err(|_| Spec031ReleaseArtifactError::Io)?
            .is_some()
        {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    kill_process_group(child.id())?;
    kill_direct_child(child)?;
    Ok(())
}

#[cfg(unix)]
pub(super) fn configure_process_group(command: &mut Command) {
    use std::os::unix::process::CommandExt;

    command.process_group(0);
}

#[cfg(unix)]
fn terminate_process_group(child_id: u32) -> Result<(), Spec031ReleaseArtifactError> {
    signal_process_group("-TERM", child_id)
}

#[cfg(unix)]
fn kill_process_group(child_id: u32) -> Result<(), Spec031ReleaseArtifactError> {
    signal_process_group("-KILL", child_id)
}

#[cfg(unix)]
fn signal_process_group(signal: &str, child_id: u32) -> Result<(), Spec031ReleaseArtifactError> {
    let group = format!("-{child_id}");
    Command::new("kill")
        .arg(signal)
        .arg("--")
        .arg(&group)
        .status()
        .map_err(|_| Spec031ReleaseArtifactError::Io)?;
    Ok(())
}

#[cfg(unix)]
fn kill_direct_child(_child: &mut Child) -> Result<(), Spec031ReleaseArtifactError> {
    Ok(())
}

#[cfg(not(unix))]
pub(super) fn configure_process_group(_command: &mut Command) {}

#[cfg(not(unix))]
fn terminate_process_group(_child_id: u32) -> Result<(), Spec031ReleaseArtifactError> {
    Ok(())
}

#[cfg(not(unix))]
fn kill_process_group(_child_id: u32) -> Result<(), Spec031ReleaseArtifactError> {
    Ok(())
}

#[cfg(not(unix))]
fn kill_direct_child(child: &mut Child) -> Result<(), Spec031ReleaseArtifactError> {
    child.kill().map_err(|_| Spec031ReleaseArtifactError::Io)
}
