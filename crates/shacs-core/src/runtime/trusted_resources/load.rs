use super::{JavaScriptRuntime, ResourceDiagnostic, ResourceDiagnosticKind, ResourceLoadCheck};
use crate::controlled_child::{
    run_configured_load_check, run_configured_package_command, ControlledChildAbort,
    ControlledChildCommand, ControlledChildError, ControlledChildOutcome, ControlledChildReceipt,
};
use shacs_projection::ResourceLoadStatus;

pub(super) struct LoadOutcome {
    pub status: ResourceLoadStatus,
    pub receipt: Option<ControlledChildReceipt>,
    diagnostic: Option<(ResourceDiagnosticKind, String)>,
}

impl LoadOutcome {
    pub fn diagnostic(&self, resource_ref: &str, path: &str) -> Option<ResourceDiagnostic> {
        self.diagnostic
            .as_ref()
            .map(|(kind, reason)| ResourceDiagnostic {
                resource_ref: resource_ref.to_owned(),
                kind: *kind,
                path: Some(path.to_owned()),
                reason: reason.clone(),
            })
    }
}

pub(super) fn run(check: &ResourceLoadCheck, abort: &ControlledChildAbort) -> LoadOutcome {
    match check {
        ResourceLoadCheck::Content => loaded(None),
        ResourceLoadCheck::PackageCommand(command) => {
            from_run(run_configured_package_command(command, abort), false)
        }
        ResourceLoadCheck::PythonImport {
            interpreter,
            module,
            cwd,
            timeout,
        } => {
            let command = ControlledChildCommand::new(
                [
                    interpreter.clone(),
                    "-c".into(),
                    "import importlib,sys; importlib.import_module(sys.argv[1])".into(),
                    module.into(),
                ],
                cwd,
                *timeout,
            );
            from_run(run_configured_load_check(&command, abort), true)
        }
        ResourceLoadCheck::JavaScriptModule {
            runtime,
            program,
            module_path,
            cwd,
            timeout,
        } => {
            let script = match runtime {
                JavaScriptRuntime::Node => "import(process.argv[1])",
                JavaScriptRuntime::Bun => "await import(process.argv[1])",
            };
            let command = ControlledChildCommand::new(
                [
                    program.clone(),
                    "--eval".into(),
                    script.into(),
                    module_path.as_os_str().to_owned(),
                ],
                cwd,
                *timeout,
            );
            from_run(run_configured_load_check(&command, abort), true)
        }
        ResourceLoadCheck::EmbeddedJavaScriptHost => unsupported(
            ResourceDiagnosticKind::ScopedUnsupported,
            "embedded JavaScript host is unsupported in the scoped baseline",
        ),
        ResourceLoadCheck::DependencyResolution => unsupported(
            ResourceDiagnosticKind::ScopedUnsupported,
            "automatic dependency resolution is unsupported in the scoped baseline",
        ),
        ResourceLoadCheck::Unsupported { reason } => {
            unsupported(ResourceDiagnosticKind::RuntimeUnsupported, reason)
        }
    }
}

fn from_run(
    result: Result<ControlledChildReceipt, ControlledChildError>,
    missing_runtime_is_unsupported: bool,
) -> LoadOutcome {
    match result {
        Err(ControlledChildError::Spawn(reason)) if missing_runtime_is_unsupported => unsupported(
            ResourceDiagnosticKind::RuntimeUnsupported,
            &format!("configured runtime could not be started: {reason}"),
        ),
        Err(error) => failed(None, &error.to_string()),
        Ok(receipt) => match receipt.outcome {
            ControlledChildOutcome::Succeeded { .. } => loaded(Some(receipt)),
            ControlledChildOutcome::Failed { .. }
            | ControlledChildOutcome::TimedOut
            | ControlledChildOutcome::Aborted
            | ControlledChildOutcome::InvalidCwd => {
                failed(Some(receipt), "configured load check did not succeed")
            }
        },
    }
}

fn loaded(receipt: Option<ControlledChildReceipt>) -> LoadOutcome {
    LoadOutcome {
        status: ResourceLoadStatus::Loaded,
        receipt,
        diagnostic: None,
    }
}

fn failed(receipt: Option<ControlledChildReceipt>, reason: &str) -> LoadOutcome {
    LoadOutcome {
        status: ResourceLoadStatus::Rejected,
        receipt,
        diagnostic: Some((ResourceDiagnosticKind::LoadFailed, reason.to_owned())),
    }
}

fn unsupported(kind: ResourceDiagnosticKind, reason: &str) -> LoadOutcome {
    LoadOutcome {
        status: ResourceLoadStatus::Unsupported,
        receipt: None,
        diagnostic: Some((kind, reason.to_owned())),
    }
}
