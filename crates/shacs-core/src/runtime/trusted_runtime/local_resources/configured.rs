use crate::controlled_child::ControlledChildCommand;
use crate::runtime::trusted_resources::{JavaScriptRuntime, ResourceCandidate, ResourceLoadCheck};
use shacs_config::{
    ConfigBundle, TrustedJavaScriptRuntime, TrustedResourceConfig, TrustedResourceKind,
};
use shacs_projection::{
    ResourceActivation, ResourceKind, ResourcePrecedence, ResourceSource, TrustedCodeDisclosure,
};
use std::ffi::OsString;
use std::path::PathBuf;
use std::time::Duration;

const LOAD_TIMEOUT: Duration = Duration::from_secs(10);

pub fn candidates(bundle: &ConfigBundle) -> Vec<ResourceCandidate> {
    bundle
        .config
        .trusted_runtime
        .resources
        .iter()
        .map(|config| candidate(config, &bundle.context.workspace))
        .collect()
}

fn candidate(config: &TrustedResourceConfig, workspace: &std::path::Path) -> ResourceCandidate {
    let path = resolve(workspace, &config.path);
    let (kind, disclosure) = match config.kind {
        TrustedResourceKind::Prompt => (ResourceKind::Prompt, TrustedCodeDisclosure::NotExecutable),
        TrustedResourceKind::Context => {
            (ResourceKind::Context, TrustedCodeDisclosure::NotExecutable)
        }
        TrustedResourceKind::Package => (ResourceKind::Package, TrustedCodeDisclosure::Shown),
        TrustedResourceKind::Python => (ResourceKind::Skill, TrustedCodeDisclosure::Shown),
        TrustedResourceKind::JavaScript => (ResourceKind::Extension, TrustedCodeDisclosure::Shown),
    };
    ResourceCandidate {
        resource_ref: config.resource_ref.clone(),
        kind,
        source: ResourceSource::Explicit,
        precedence: ResourcePrecedence::Explicit,
        path: path.clone(),
        activation: ResourceActivation::Explicit,
        trusted_code_disclosure: disclosure,
        load_check: load_check(config, path, workspace),
        diagnostics: Vec::new(),
    }
}

fn load_check(
    config: &TrustedResourceConfig,
    path: PathBuf,
    workspace: &std::path::Path,
) -> ResourceLoadCheck {
    match config.kind {
        TrustedResourceKind::Prompt | TrustedResourceKind::Context => ResourceLoadCheck::Content,
        TrustedResourceKind::Package => configured_program(config).map_or_else(
            || unsupported("configured package command is missing a program"),
            |argv| {
                ResourceLoadCheck::PackageCommand(ControlledChildCommand::new(
                    argv,
                    workspace,
                    LOAD_TIMEOUT,
                ))
            },
        ),
        TrustedResourceKind::Python => match (&config.program, &config.module) {
            (Some(program), Some(module)) => ResourceLoadCheck::PythonImport {
                interpreter: program.into(),
                module: module.clone(),
                cwd: workspace.to_path_buf(),
                timeout: LOAD_TIMEOUT,
            },
            (None, _) | (_, None) => {
                unsupported("configured Python resource requires program and module")
            }
        },
        TrustedResourceKind::JavaScript => match (&config.program, config.runtime) {
            (Some(program), Some(runtime)) => ResourceLoadCheck::JavaScriptModule {
                runtime: match runtime {
                    TrustedJavaScriptRuntime::Node => JavaScriptRuntime::Node,
                    TrustedJavaScriptRuntime::Bun => JavaScriptRuntime::Bun,
                },
                program: program.into(),
                module_path: path,
                cwd: workspace.to_path_buf(),
                timeout: LOAD_TIMEOUT,
            },
            (None, _) | (_, None) => {
                unsupported("configured JavaScript resource requires program and runtime")
            }
        },
    }
}

fn configured_program(config: &TrustedResourceConfig) -> Option<Vec<OsString>> {
    let mut argv = vec![OsString::from(config.program.as_ref()?)];
    argv.extend(config.args.iter().map(OsString::from));
    Some(argv)
}

fn unsupported(reason: &str) -> ResourceLoadCheck {
    ResourceLoadCheck::Unsupported {
        reason: reason.to_owned(),
    }
}

fn resolve(workspace: &std::path::Path, path: &str) -> PathBuf {
    let path = PathBuf::from(path);
    if path.is_absolute() {
        path
    } else {
        workspace.join(path)
    }
}
