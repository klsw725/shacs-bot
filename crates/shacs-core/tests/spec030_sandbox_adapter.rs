#![cfg(unix)]

use shacs_core::controlled_child::{ControlledChildAbort, ControlledChildCommand};
use shacs_core::runtime::sandbox_adapter::{
    execute_bash, sandbox_argv, SandboxBackend, SandboxExecutionError, SandboxFallbackPolicy,
    SandboxMountPlan, SandboxNetworkPlan, SandboxPlan, SandboxRuntimeStatus,
};
use shacs_core::runtime::trusted_runtime::{
    LocalSpec030ProjectionProvider, SandboxInactiveFallback, SandboxInactiveStatus,
    SandboxObservation, Spec030FactStore, WorkspaceTrustObservation,
};
use shacs_projection::{
    ProcessAdapterKind, SandboxFallback, SandboxStatus, Spec030ProjectionProvider,
};
use std::error::Error;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::time::Duration;

fn command(cwd: &Path, script: &str) -> ControlledChildCommand {
    ControlledChildCommand::new(["/bin/sh", "-c", script], cwd, Duration::from_secs(3))
}

fn plan(root: &Path, fallback: SandboxFallbackPolicy) -> SandboxPlan {
    SandboxPlan {
        backend: SandboxBackend::Bubblewrap,
        fallback,
        mounts: SandboxMountPlan {
            deny_read: vec![root.join("private")],
            allow_write: vec![root.join("output")],
        },
        network: SandboxNetworkPlan::Deny,
    }
}

fn fake_bwrap(root: &Path, body: &str) -> Result<std::path::PathBuf, Box<dyn Error>> {
    let bin = root.join("bin");
    fs::create_dir(&bin)?;
    let bwrap = bin.join("bwrap");
    fs::write(&bwrap, body)?;
    fs::set_permissions(&bwrap, fs::Permissions::from_mode(0o755))?;
    Ok(bin)
}

#[test]
fn policy_argv_contains_deny_read_allow_write_and_network_deny() -> Result<(), Box<dyn Error>> {
    // Given
    let root = tempfile::tempdir()?;
    fs::create_dir(root.path().join("private"))?;
    fs::create_dir(root.path().join("output"))?;
    let command = command(root.path(), "printf wrapped");

    // When
    let argv = sandbox_argv(
        &plan(root.path(), SandboxFallbackPolicy::SandboxRequired),
        &command,
    )?;

    // Then
    let rendered = argv
        .iter()
        .map(|value| value.to_string_lossy())
        .collect::<Vec<_>>();
    let private = root.path().join("private").canonicalize()?;
    let output = root.path().join("output").canonicalize()?;
    assert!(rendered.iter().any(|value| value == "--unshare-net"));
    assert!(rendered
        .windows(2)
        .any(|pair| { pair[0] == "--tmpfs" && pair[1] == private.to_string_lossy() }));
    assert!(rendered.windows(3).any(|triple| {
        triple[0] == "--bind"
            && triple[1] == output.to_string_lossy()
            && triple[2] == output.to_string_lossy()
    }));
    Ok(())
}

#[test]
fn native_fallback_reports_unsupported_without_claiming_applied_adapter(
) -> Result<(), Box<dyn Error>> {
    // Given
    let root = tempfile::tempdir()?;
    fs::create_dir(root.path().join("private"))?;
    fs::create_dir(root.path().join("output"))?;
    let mut command = command(root.path(), "printf native");
    command.inherit_env = false;
    command.env.insert("PATH".into(), root.path().into());
    let facts = Spec030FactStore::new(WorkspaceTrustObservation::Trusted);

    // When
    let execution = execute_bash(
        Some(&plan(
            root.path(),
            SandboxFallbackPolicy::TrustedNativeFallback,
        )),
        &command,
        &ControlledChildAbort::new(),
    )?;
    facts.record_sandbox_execution(&execution.fact)?;

    // Then
    assert_eq!(execution.fact.status, SandboxRuntimeStatus::Unsupported);
    assert_eq!(execution.fact.applied_adapter, None);
    assert!(!execution.fact.wrapped_execution);
    assert_eq!(
        execution.fact.observation(),
        SandboxObservation::Inactive {
            status: SandboxInactiveStatus::Unsupported,
            fallback: SandboxInactiveFallback::TrustedNativeFallback,
        }
    );
    assert!(execution.warning.is_some());
    assert_eq!(execution.receipt.stdout.captured, b"native");
    let projection = LocalSpec030ProjectionProvider::new(facts).projection();
    assert_eq!(projection.sandbox().status, SandboxStatus::Unsupported);
    assert_eq!(
        projection.sandbox().fallback,
        SandboxFallback::TrustedNativeFallback
    );
    Ok(())
}

#[test]
fn required_sandbox_rejects_failed_probe_before_native_execution() -> Result<(), Box<dyn Error>> {
    // Given
    let root = tempfile::tempdir()?;
    fs::create_dir(root.path().join("private"))?;
    fs::create_dir(root.path().join("output"))?;
    let marker = root.path().join("must-not-exist");
    let bin = fake_bwrap(root.path(), "#!/bin/sh\nexit 9\n")?;
    let mut command = command(root.path(), "touch must-not-exist");
    command.inherit_env = false;
    command.env.insert("PATH".into(), bin.into());

    // When
    let error = execute_bash(
        Some(&plan(root.path(), SandboxFallbackPolicy::SandboxRequired)),
        &command,
        &ControlledChildAbort::new(),
    )
    .expect_err("required sandbox rejects a failed probe");

    // Then
    let SandboxExecutionError::RequiredUnavailable(fact) = error else {
        return Err("unexpected sandbox error".into());
    };
    assert_eq!(fact.status, SandboxRuntimeStatus::Failed);
    assert_eq!(
        fact.reason.as_deref(),
        Some("bubblewrap probe failed (Exit code: 9)")
    );
    assert_eq!(
        fact.observation(),
        SandboxObservation::Inactive {
            status: SandboxInactiveStatus::Failed,
            fallback: SandboxInactiveFallback::ExecutionDenied,
        }
    );
    assert!(!marker.exists());
    Ok(())
}

#[test]
fn required_sandbox_rejects_policy_setup_failure() -> Result<(), Box<dyn Error>> {
    // Given
    let root = tempfile::tempdir()?;
    fs::create_dir(root.path().join("private"))?;
    fs::create_dir(root.path().join("output"))?;
    let bin = fake_bwrap(
        root.path(),
        "#!/bin/sh\n[ \"$1\" = --version ] && exit 0\nexit 8\n",
    )?;
    let mut command = command(root.path(), "printf native");
    command.inherit_env = false;
    command.env.insert("PATH".into(), bin.into());

    // When
    let error = execute_bash(
        Some(&plan(root.path(), SandboxFallbackPolicy::SandboxRequired)),
        &command,
        &ControlledChildAbort::new(),
    )
    .expect_err("required sandbox rejects policy setup failure");

    // Then
    let SandboxExecutionError::RequiredUnavailable(fact) = error else {
        return Err("unexpected sandbox error".into());
    };
    assert_eq!(fact.status, SandboxRuntimeStatus::Failed);
    assert!(!fact.wrapped_execution);
    Ok(())
}

#[test]
fn invalid_plan_records_failed_execution_denied_fact() -> Result<(), Box<dyn Error>> {
    // Given
    let root = tempfile::tempdir()?;
    let missing = root.path().join("missing");
    let plan = SandboxPlan {
        backend: SandboxBackend::Bubblewrap,
        fallback: SandboxFallbackPolicy::SandboxRequired,
        mounts: SandboxMountPlan {
            deny_read: vec![missing],
            allow_write: Vec::new(),
        },
        network: SandboxNetworkPlan::Allow,
    };
    let facts = Spec030FactStore::new(WorkspaceTrustObservation::Trusted);

    // When
    let error = sandbox_argv(&plan, &command(root.path(), ":"))
        .expect_err("missing mount path makes the sandbox plan invalid");
    let fact = error.fact().ok_or("invalid plan fact missing")?;
    facts.record_sandbox_execution(fact)?;

    // Then
    assert_eq!(fact.status, SandboxRuntimeStatus::Failed);
    assert_eq!(
        fact.observation(),
        SandboxObservation::Inactive {
            status: SandboxInactiveStatus::Failed,
            fallback: SandboxInactiveFallback::ExecutionDenied,
        }
    );
    let projection = LocalSpec030ProjectionProvider::new(facts).projection();
    assert_eq!(projection.sandbox().status, SandboxStatus::Failed);
    assert_eq!(
        projection.sandbox().fallback,
        SandboxFallback::ExecutionDenied
    );
    Ok(())
}

#[test]
fn active_fact_requires_probe_and_wrapped_execution() -> Result<(), Box<dyn Error>> {
    // Given
    let root = tempfile::tempdir()?;
    fs::create_dir(root.path().join("private"))?;
    fs::create_dir(root.path().join("output"))?;
    let log = root.path().join("calls");
    let bin = fake_bwrap(
        root.path(),
        &format!(
            "#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{}'\n[ \"$1\" = --version ] && exit 0\nexit 0\n",
            log.display()
        ),
    )?;
    let mut command = command(root.path(), "printf wrapped");
    command.inherit_env = false;
    command.env.insert("PATH".into(), bin.into());

    // When
    let execution = execute_bash(
        Some(&plan(root.path(), SandboxFallbackPolicy::SandboxRequired)),
        &command,
        &ControlledChildAbort::new(),
    )?;

    // Then
    assert_eq!(execution.fact.status, SandboxRuntimeStatus::Active);
    assert_eq!(
        execution.fact.applied_adapter,
        Some(ProcessAdapterKind::Bash)
    );
    assert!(execution.fact.wrapped_execution);
    assert_eq!(
        execution.fact.observation(),
        SandboxObservation::Active {
            applied_adapters: vec![ProcessAdapterKind::Bash],
            filesystem_policy: shacs_projection::SandboxFilesystemPolicy::Applied,
            network_policy: shacs_projection::SandboxNetworkPolicy::Applied,
        }
    );
    assert_eq!(fs::read_to_string(log)?.lines().count(), 3);
    Ok(())
}

#[test]
fn real_bwrap_lane_runs_only_when_required() -> Result<(), Box<dyn Error>> {
    if std::env::var_os("SHACS_REQUIRE_BWRAP").is_none() {
        return Ok(());
    }
    let root = tempfile::tempdir()?;
    fs::create_dir(root.path().join("private"))?;
    fs::create_dir(root.path().join("output"))?;
    let private = root.path().join("private");
    let output = root.path().join("output");
    fs::write(private.join("secret"), "secret")?;
    let script = format!(
        "if cat '{}/secret' >/dev/null 2>&1; then exit 41; fi; printf allowed > '{}/written'; interfaces=$(grep -c ':' /proc/net/dev); routes=$(grep -c '^' /proc/net/route); [ \"$interfaces\" -eq 1 ] && [ \"$routes\" -eq 1 ]",
        private.display(),
        output.display()
    );
    let execution = execute_bash(
        Some(&plan(root.path(), SandboxFallbackPolicy::SandboxRequired)),
        &command(root.path(), &script),
        &ControlledChildAbort::new(),
    )?;
    assert_eq!(execution.fact.status, SandboxRuntimeStatus::Active);
    assert_eq!(fs::read_to_string(output.join("written"))?, "allowed");
    Ok(())
}
