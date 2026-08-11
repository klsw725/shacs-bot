use super::{
    sandbox_argv, SandboxExecution, SandboxExecutionError, SandboxExecutionFact,
    SandboxFallbackPolicy, SandboxPlan, SandboxRuntimeStatus,
};
use crate::controlled_child::{
    run_bash, run_generic_argv, ControlledChildAbort, ControlledChildCommand, ControlledChildError,
    ControlledChildOutcome, ControlledChildReceipt,
};
use shacs_projection::{ProcessAdapterKind, SandboxFilesystemPolicy, SandboxNetworkPolicy};
use std::ffi::OsString;
use std::time::Duration;

pub fn execute_bash(
    plan: Option<&SandboxPlan>,
    command: &ControlledChildCommand,
    abort: &ControlledChildAbort,
) -> Result<SandboxExecution, SandboxExecutionError> {
    let Some(plan) = plan else {
        let receipt = run_bash(command, abort)?;
        return Ok(SandboxExecution {
            fact: inactive_fact(
                SandboxRuntimeStatus::Disabled,
                SandboxFallbackPolicy::TrustedNativeFallback,
                None,
            ),
            receipt,
            warning: Some("sandbox disabled; trusted native fallback used".to_owned()),
        });
    };
    if let Some(fact) = probe_failure(plan, probe_bwrap(command, abort)) {
        return fallback_or_reject(command, abort, fact);
    }
    let argv = match sandbox_argv(plan, command) {
        Ok(argv) => argv,
        Err(error) => {
            let fact = error
                .fact()
                .cloned()
                .unwrap_or_else(|| SandboxExecutionFact::failed(plan.fallback, error.to_string()));
            return fallback_or_reject(command, abort, fact);
        }
    };
    if let Some(fact) = probe_failure(plan, probe_policy(plan, command, abort)) {
        return fallback_or_reject(command, abort, fact);
    }
    let mut wrapped = command.clone();
    wrapped.argv = argv;
    match run_bash(&wrapped, abort) {
        Ok(receipt) => Ok(SandboxExecution {
            fact: SandboxExecutionFact {
                status: SandboxRuntimeStatus::Active,
                fallback: plan.fallback,
                applied_adapter: Some(ProcessAdapterKind::Bash),
                filesystem_policy: SandboxFilesystemPolicy::Applied,
                network_policy: SandboxNetworkPolicy::Applied,
                wrapped_execution: true,
                reason: None,
            },
            receipt,
            warning: None,
        }),
        Err(error) => fallback_or_reject(
            command,
            abort,
            inactive_fact(
                SandboxRuntimeStatus::Failed,
                plan.fallback,
                Some(error.to_string()),
            ),
        ),
    }
}

fn probe_policy(
    plan: &SandboxPlan,
    command: &ControlledChildCommand,
    abort: &ControlledChildAbort,
) -> Result<ControlledChildReceipt, ControlledChildError> {
    let mut probe =
        ControlledChildCommand::new(["/bin/sh", "-c", ":"], &command.cwd, Duration::from_secs(2));
    probe.env = command.env.clone();
    probe.inherit_env = command.inherit_env;
    probe.argv = sandbox_argv(plan, &probe).map_err(|error| {
        ControlledChildError::Spawn(format!("sandbox policy probe could not be built: {error}"))
    })?;
    run_generic_argv(&probe, abort)
}

fn probe_bwrap(
    command: &ControlledChildCommand,
    abort: &ControlledChildAbort,
) -> Result<ControlledChildReceipt, ControlledChildError> {
    let mut probe = ControlledChildCommand::new(
        [OsString::from("bwrap"), OsString::from("--version")],
        &command.cwd,
        Duration::from_secs(2),
    );
    probe.env = command.env.clone();
    probe.inherit_env = command.inherit_env;
    run_generic_argv(&probe, abort)
}

fn probe_failure(
    plan: &SandboxPlan,
    probe: Result<ControlledChildReceipt, ControlledChildError>,
) -> Option<SandboxExecutionFact> {
    match probe {
        Err(ControlledChildError::Spawn(reason)) => Some(inactive_fact(
            SandboxRuntimeStatus::Unsupported,
            plan.fallback,
            Some(reason),
        )),
        Err(error) => Some(inactive_fact(
            SandboxRuntimeStatus::Failed,
            plan.fallback,
            Some(error.to_string()),
        )),
        Ok(receipt) => match receipt.outcome {
            ControlledChildOutcome::Succeeded { .. } => None,
            ControlledChildOutcome::Failed { code } => Some(inactive_fact(
                SandboxRuntimeStatus::Failed,
                plan.fallback,
                Some(format!(
                    "bubblewrap probe failed (Exit code: {})",
                    code.unwrap_or(-1)
                )),
            )),
            ControlledChildOutcome::TimedOut
            | ControlledChildOutcome::Aborted
            | ControlledChildOutcome::InvalidCwd => Some(inactive_fact(
                SandboxRuntimeStatus::Failed,
                plan.fallback,
                Some("bubblewrap probe did not succeed".to_owned()),
            )),
        },
    }
}

fn fallback_or_reject(
    command: &ControlledChildCommand,
    abort: &ControlledChildAbort,
    fact: SandboxExecutionFact,
) -> Result<SandboxExecution, SandboxExecutionError> {
    match fact.fallback {
        SandboxFallbackPolicy::SandboxRequired => {
            Err(SandboxExecutionError::RequiredUnavailable(fact))
        }
        SandboxFallbackPolicy::TrustedNativeFallback => Ok(SandboxExecution {
            receipt: run_bash(command, abort)?,
            warning: Some("sandbox inactive; trusted native fallback used".to_owned()),
            fact,
        }),
    }
}

fn inactive_fact(
    status: SandboxRuntimeStatus,
    fallback: SandboxFallbackPolicy,
    reason: Option<String>,
) -> SandboxExecutionFact {
    SandboxExecutionFact {
        status,
        fallback,
        applied_adapter: None,
        filesystem_policy: SandboxFilesystemPolicy::NotApplied,
        network_policy: SandboxNetworkPolicy::NotApplied,
        wrapped_execution: false,
        reason,
    }
}
