use super::{SandboxBackend, SandboxExecutionError, SandboxNetworkPlan, SandboxPlan};
use crate::controlled_child::ControlledChildCommand;
use std::ffi::OsString;
use std::path::Path;

pub fn sandbox_argv(
    plan: &SandboxPlan,
    command: &ControlledChildCommand,
) -> Result<Vec<OsString>, SandboxExecutionError> {
    match plan.backend {
        SandboxBackend::Bubblewrap => bubblewrap_argv(plan, command),
    }
}

fn bubblewrap_argv(
    plan: &SandboxPlan,
    command: &ControlledChildCommand,
) -> Result<Vec<OsString>, SandboxExecutionError> {
    let cwd = canonical(&command.cwd, plan.fallback)?;
    let mut argv = vec![
        "bwrap".into(),
        "--new-session".into(),
        "--die-with-parent".into(),
        "--ro-bind".into(),
        "/".into(),
        "/".into(),
        "--proc".into(),
        "/proc".into(),
        "--dev".into(),
        "/dev".into(),
    ];
    match plan.network {
        SandboxNetworkPlan::Allow => {}
        SandboxNetworkPlan::Deny => argv.push("--unshare-net".into()),
    }
    for path in &plan.mounts.allow_write {
        let path = canonical(path, plan.fallback)?;
        argv.extend(["--bind".into(), path.clone().into(), path.into()]);
    }
    for path in &plan.mounts.deny_read {
        let path = canonical(path, plan.fallback)?;
        if path.is_dir() {
            argv.extend(["--tmpfs".into(), path.into()]);
        } else {
            argv.extend(["--bind".into(), "/dev/null".into(), path.into()]);
        }
    }
    argv.extend(["--chdir".into(), cwd.into(), "--".into()]);
    argv.extend(command.argv.iter().cloned());
    Ok(argv)
}

fn canonical(
    path: &Path,
    fallback: super::SandboxFallbackPolicy,
) -> Result<std::path::PathBuf, SandboxExecutionError> {
    path.canonicalize().map_err(|error| {
        SandboxExecutionError::InvalidPlan(super::SandboxExecutionFact::failed(
            fallback,
            format!("{}: {error}", path.display()),
        ))
    })
}
