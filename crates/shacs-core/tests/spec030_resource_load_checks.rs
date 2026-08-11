#![cfg(unix)]

use shacs_core::controlled_child::{ControlledChildAbort, ControlledChildCommand};
use shacs_core::runtime::trusted_resources::{
    inspect_resources, JavaScriptRuntime, ResourceCandidate, ResourceDiagnosticKind,
    ResourceLoadCheck, WorkspaceResourceTrust,
};
use shacs_core::runtime::trusted_runtime::{
    LocalSpec030ProjectionProvider, Spec030FactStore, WorkspaceTrustObservation,
};
use shacs_projection::{
    ProcessAdapterKind, ProcessAdapterSupport, ProcessControlReason, ProcessControlScope,
    ResourceActivation, ResourceKind, ResourceLoadStatus, ResourcePrecedence, ResourceSource,
    Spec030ProjectionProvider, TrustedCodeDisclosure,
};
use std::error::Error;
use std::path::Path;
use std::time::Duration;

fn executable_candidate(path: &Path, load_check: ResourceLoadCheck) -> ResourceCandidate {
    ResourceCandidate {
        resource_ref: "package:local".to_owned(),
        kind: ResourceKind::Package,
        source: ResourceSource::Explicit,
        precedence: ResourcePrecedence::Explicit,
        path: path.to_path_buf(),
        activation: ResourceActivation::Explicit,
        trusted_code_disclosure: TrustedCodeDisclosure::Shown,
        load_check,
        diagnostics: Vec::new(),
    }
}

fn command(cwd: &Path, script: &str) -> ControlledChildCommand {
    ControlledChildCommand::new(["/bin/sh", "-c", script], cwd, Duration::from_secs(3))
}

fn status(candidate: ResourceCandidate) -> Result<ResourceLoadStatus, Box<dyn Error>> {
    let inspection = inspect_resources(
        vec![candidate],
        WorkspaceResourceTrust::Trusted,
        &ControlledChildAbort::new(),
    );
    Ok(inspection
        .resources
        .into_iter()
        .next()
        .ok_or("resource fact missing")?
        .projection
        .load_status)
}

#[test]
fn configured_local_package_command_reports_success_and_failure() -> Result<(), Box<dyn Error>> {
    // Given
    let root = tempfile::tempdir()?;
    let manifest = root.path().join("package.toml");
    std::fs::write(&manifest, "name='local'")?;

    // When
    let success = status(executable_candidate(
        &manifest,
        ResourceLoadCheck::PackageCommand(command(root.path(), "exit 0")),
    ))?;
    let failure = status(executable_candidate(
        &manifest,
        ResourceLoadCheck::PackageCommand(command(root.path(), "exit 7")),
    ))?;

    // Then
    assert_eq!(success, ResourceLoadStatus::Loaded);
    assert_eq!(failure, ResourceLoadStatus::Rejected);
    Ok(())
}

#[test]
fn configured_package_fact_becomes_supported_only_after_used_inspection(
) -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let manifest = root.path().join("package.toml");
    std::fs::write(&manifest, "name='local'")?;
    let facts = Spec030FactStore::new(WorkspaceTrustObservation::Trusted);
    let before = LocalSpec030ProjectionProvider::new(facts.clone()).projection();
    assert_eq!(
        before
            .process_adapters()
            .iter()
            .find(|row| row.adapter == ProcessAdapterKind::PackageOperation)
            .ok_or("package row missing")?
            .support,
        ProcessAdapterSupport::Unsupported
    );

    let inspection = inspect_resources(
        vec![executable_candidate(
            &manifest,
            ResourceLoadCheck::PackageCommand(command(root.path(), "exit 0")),
        )],
        WorkspaceResourceTrust::Trusted,
        &ControlledChildAbort::new(),
    );
    facts.record_resource_inspection(&inspection)?;

    let after = LocalSpec030ProjectionProvider::new(facts).projection();
    let package = after
        .process_adapters()
        .iter()
        .find(|row| row.adapter == ProcessAdapterKind::PackageOperation)
        .ok_or("package row missing")?;
    assert_eq!(package.support, ProcessAdapterSupport::Supported);
    assert_eq!(package.control_scope, ProcessControlScope::ControlledChild);
    assert_eq!(
        package.reason,
        ProcessControlReason::ControlledChildObservedNoRollback
    );
    assert_eq!(package.recent_outcomes.len(), 1);
    Ok(())
}

#[test]
fn configured_python_import_reports_success_and_failure() -> Result<(), Box<dyn Error>> {
    // Given
    let root = tempfile::tempdir()?;
    let module = root.path().join("loadable.py");
    std::fs::write(&module, "VALUE = 1\n")?;
    let interpreter = std::env::var_os("PYTHON").unwrap_or_else(|| "python3".into());

    // When
    let success = status(executable_candidate(
        &module,
        ResourceLoadCheck::PythonImport {
            interpreter: interpreter.clone(),
            module: "loadable".to_owned(),
            cwd: root.path().to_path_buf(),
            timeout: Duration::from_secs(3),
        },
    ))?;
    let failure = status(executable_candidate(
        &module,
        ResourceLoadCheck::PythonImport {
            interpreter,
            module: "missing_shacs_module".to_owned(),
            cwd: root.path().to_path_buf(),
            timeout: Duration::from_secs(3),
        },
    ))?;

    // Then
    assert_eq!(success, ResourceLoadStatus::Loaded);
    assert_eq!(failure, ResourceLoadStatus::Rejected);
    Ok(())
}

#[test]
fn javascript_load_failure_and_embedded_host_are_not_success() -> Result<(), Box<dyn Error>> {
    // Given
    let root = tempfile::tempdir()?;
    let module = root.path().join("broken.mjs");
    std::fs::write(&module, "throw new Error('broken')")?;
    let missing_runtime = root.path().join("missing-node");

    // When
    let failed = inspect_resources(
        vec![executable_candidate(
            &module,
            ResourceLoadCheck::JavaScriptModule {
                runtime: JavaScriptRuntime::Node,
                program: "/bin/sh".into(),
                module_path: module.clone(),
                cwd: root.path().to_path_buf(),
                timeout: Duration::from_secs(3),
            },
        )],
        WorkspaceResourceTrust::Trusted,
        &ControlledChildAbort::new(),
    );
    let unsupported = inspect_resources(
        vec![executable_candidate(
            &module,
            ResourceLoadCheck::EmbeddedJavaScriptHost,
        )],
        WorkspaceResourceTrust::Trusted,
        &ControlledChildAbort::new(),
    );
    let missing = inspect_resources(
        vec![executable_candidate(
            &module,
            ResourceLoadCheck::JavaScriptModule {
                runtime: JavaScriptRuntime::Node,
                program: missing_runtime.into_os_string(),
                module_path: module.clone(),
                cwd: root.path().to_path_buf(),
                timeout: Duration::from_secs(3),
            },
        )],
        WorkspaceResourceTrust::Trusted,
        &ControlledChildAbort::new(),
    );

    // Then
    assert_ne!(
        failed.resources[0].projection.load_status,
        ResourceLoadStatus::Loaded
    );
    assert_eq!(
        unsupported.resources[0].projection.load_status,
        ResourceLoadStatus::Unsupported
    );
    assert_eq!(
        missing.resources[0].projection.load_status,
        ResourceLoadStatus::Unsupported
    );
    assert!(missing
        .diagnostics
        .iter()
        .any(|diagnostic| { diagnostic.kind == ResourceDiagnosticKind::RuntimeUnsupported }));
    assert!(unsupported.diagnostics.iter().any(|diagnostic| {
        diagnostic.kind == ResourceDiagnosticKind::ScopedUnsupported
            && diagnostic.reason.contains("embedded")
    }));
    Ok(())
}
