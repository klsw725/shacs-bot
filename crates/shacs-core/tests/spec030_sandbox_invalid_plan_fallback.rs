#![cfg(unix)]

use shacs_core::controlled_child::{ControlledChildAbort, ControlledChildCommand};
use shacs_core::runtime::sandbox_adapter::{
    execute_bash, SandboxBackend, SandboxExecutionError, SandboxFallbackPolicy, SandboxMountPlan,
    SandboxNetworkPlan, SandboxPlan, SandboxRuntimeStatus,
};
use shacs_core::runtime::trusted_runtime::{
    LocalSpec030ProjectionProvider, Spec030FactStore, WorkspaceTrustObservation,
};
use shacs_projection::{SandboxFallback, SandboxStatus, Spec030ProjectionProvider};
use std::error::Error;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::time::Duration;

#[derive(Clone, Copy)]
enum InvalidMount {
    DenyRead,
    AllowWrite,
}

fn plan(root: &Path, fallback: SandboxFallbackPolicy, invalid: InvalidMount) -> SandboxPlan {
    let missing = root.join("missing");
    let mounts = match invalid {
        InvalidMount::DenyRead => SandboxMountPlan {
            deny_read: vec![missing],
            allow_write: Vec::new(),
        },
        InvalidMount::AllowWrite => SandboxMountPlan {
            deny_read: Vec::new(),
            allow_write: vec![missing],
        },
    };
    SandboxPlan {
        backend: SandboxBackend::Bubblewrap,
        fallback,
        mounts,
        network: SandboxNetworkPlan::Deny,
    }
}

fn command(root: &Path, script: &str) -> Result<ControlledChildCommand, Box<dyn Error>> {
    let bin = root.join("bin");
    fs::create_dir(&bin)?;
    let calls = root.join("bwrap-calls");
    let bwrap = bin.join("bwrap");
    fs::write(
        &bwrap,
        format!(
            "#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{}'\n[ \"$1\" = --version ] && exit 0\nexit 91\n",
            calls.display()
        ),
    )?;
    fs::set_permissions(&bwrap, fs::Permissions::from_mode(0o755))?;
    let mut command =
        ControlledChildCommand::new(["/bin/sh", "-c", script], root, Duration::from_secs(3));
    command.inherit_env = false;
    command.env.insert("PATH".into(), bin.into());
    Ok(command)
}

#[test]
fn invalid_deny_read_uses_configured_native_fallback_with_warning_and_fact(
) -> Result<(), Box<dyn Error>> {
    // Given
    let root = tempfile::tempdir()?;
    let command = command(root.path(), "printf deny-read-native")?;
    let facts = Spec030FactStore::new(WorkspaceTrustObservation::Trusted);

    // When
    let execution = execute_bash(
        Some(&plan(
            root.path(),
            SandboxFallbackPolicy::TrustedNativeFallback,
            InvalidMount::DenyRead,
        )),
        &command,
        &ControlledChildAbort::new(),
    )?;
    facts.record_sandbox_execution(&execution.fact)?;

    // Then
    assert_eq!(execution.receipt.stdout.captured, b"deny-read-native");
    assert!(execution.warning.is_some());
    assert_eq!(execution.fact.status, SandboxRuntimeStatus::Failed);
    assert_eq!(
        execution.fact.fallback,
        SandboxFallbackPolicy::TrustedNativeFallback
    );
    assert_eq!(execution.fact.applied_adapter, None);
    assert!(!execution.fact.wrapped_execution);
    let projection = LocalSpec030ProjectionProvider::new(facts).projection();
    assert_eq!(projection.sandbox().status, SandboxStatus::Failed);
    assert_eq!(
        projection.sandbox().fallback,
        SandboxFallback::TrustedNativeFallback
    );
    Ok(())
}

#[test]
fn invalid_allow_write_uses_configured_native_fallback_with_visible_surface_result(
) -> Result<(), Box<dyn Error>> {
    // Given
    let root = tempfile::tempdir()?;
    let command = command(root.path(), "printf allow-write-native")?;

    // When
    let execution = execute_bash(
        Some(&plan(
            root.path(),
            SandboxFallbackPolicy::TrustedNativeFallback,
            InvalidMount::AllowWrite,
        )),
        &command,
        &ControlledChildAbort::new(),
    )?;

    // Then
    assert_eq!(execution.receipt.stdout.captured, b"allow-write-native");
    assert!(execution.warning.is_some());
    assert_eq!(execution.fact.status, SandboxRuntimeStatus::Failed);
    assert_eq!(
        execution.fact.fallback,
        SandboxFallbackPolicy::TrustedNativeFallback
    );
    assert_eq!(execution.fact.applied_adapter, None);
    Ok(())
}

#[test]
fn invalid_plan_with_required_fallback_blocks_without_running_command_or_adapter(
) -> Result<(), Box<dyn Error>> {
    // Given
    let root = tempfile::tempdir()?;
    let marker = root.path().join("must-not-exist");
    let command = command(
        root.path(),
        &format!("printf blocked > '{}'", marker.display()),
    )?;
    let facts = Spec030FactStore::new(WorkspaceTrustObservation::Untrusted);

    // When
    let error = execute_bash(
        Some(&plan(
            root.path(),
            SandboxFallbackPolicy::SandboxRequired,
            InvalidMount::AllowWrite,
        )),
        &command,
        &ControlledChildAbort::new(),
    )
    .expect_err("required sandbox rejects an invalid plan");
    let SandboxExecutionError::RequiredUnavailable(fact) = error else {
        return Err("invalid plan did not use the shared rejection path".into());
    };
    facts.record_sandbox_execution(&fact)?;

    // Then
    assert!(!marker.exists());
    assert_eq!(fact.status, SandboxRuntimeStatus::Failed);
    assert_eq!(fact.fallback, SandboxFallbackPolicy::SandboxRequired);
    assert_eq!(fact.applied_adapter, None);
    assert!(!fact.wrapped_execution);
    let projection = LocalSpec030ProjectionProvider::new(facts).projection();
    assert_eq!(projection.sandbox().status, SandboxStatus::Failed);
    assert_eq!(
        projection.sandbox().fallback,
        SandboxFallback::ExecutionDenied
    );
    Ok(())
}
