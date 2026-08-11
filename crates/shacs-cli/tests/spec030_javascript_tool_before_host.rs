use shacs_projection::{HookDiagnosticKind, HookRuntimeStatus};
use std::error::Error;
use std::fs;

#[path = "spec030_javascript_tool_before_host/support.rs"]
mod support;
use support::{run_scenario, Activation, PluginLocation, Scenario, ScopedEnv};

#[test]
fn production_constructor_runs_only_enabled_trusted_javascript_tool_before_handlers(
) -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let responses = root.path().join("responses.json");
    fs::write(&responses, "[]")?;
    let _provider_env = ScopedEnv::set("SHACS_DEBUG_FAKE_PROVIDER_RESPONSES", &responses);

    let allow = run_scenario(
        root.path(),
        &responses,
        Scenario::enabled_user_data(
            "allow",
            "js",
            "function toolBefore(context) { return context.name === 'write_file' ? {allow: true} : {block: true, reason: 'wrong tool'}; }",
        ),
    )?;
    let block = run_scenario(
        root.path(),
        &responses,
        Scenario::enabled_user_data(
            "block",
            "mjs",
            "function toolBefore(context) { return {block: true, reason: `blocked ${context.id}`}; }",
        ),
    )?;
    let error = run_scenario(
        root.path(),
        &responses,
        Scenario::enabled_user_data(
            "error",
            "js",
            "function toolBefore() { throw new Error('fixture failure'); }",
        ),
    )?;
    let infinite_loop = run_scenario(
        root.path(),
        &responses,
        Scenario::enabled_user_data("loop", "js", "function toolBefore() { while (true) {} }"),
    )?;
    let typescript = run_scenario(
        root.path(),
        &responses,
        Scenario::enabled_user_data(
            "typescript",
            "ts",
            "function toolBefore(context: { name: string }): { block: true; reason: string } { return {block: true, reason: `ts blocked ${context.name}`}; }",
        ),
    )?;
    let disabled = run_scenario(
        root.path(),
        &responses,
        Scenario {
            id: "disabled",
            location: PluginLocation::UserData,
            activation: Activation::Disabled,
            extension: "js",
            source: "function toolBefore() { return {block: true, reason: 'must not run'}; }",
        },
    )?;
    let untrusted = run_scenario(
        root.path(),
        &responses,
        Scenario {
            id: "untrusted",
            location: PluginLocation::Workspace,
            activation: Activation::Enabled,
            extension: "js",
            source: "function toolBefore() { return {block: true, reason: 'must not run'}; }",
        },
    )?;

    assert!(
        allow.marker.exists(),
        "allow handler did not reach the tool"
    );
    assert!(!block.marker.exists(), "block handler reached the tool");
    assert!(error.marker.exists(), "throwing handler was not fail-open");
    assert!(
        infinite_loop.marker.exists(),
        "bounded loop was not fail-open"
    );
    assert!(!typescript.marker.exists(), "TypeScript block reached exec");
    assert!(disabled.marker.exists(), "disabled handler executed");
    assert!(untrusted.marker.exists(), "untrusted handler executed");
    assert_eq!(allow.registered_handlers, 1);
    assert_eq!(block.status, HookRuntimeStatus::Active);
    assert!(error.diagnostics.contains(&HookDiagnosticKind::Panic));
    assert!(infinite_loop
        .diagnostics
        .contains(&HookDiagnosticKind::Panic));
    assert_eq!(disabled.registered_handlers, 0);
    assert_eq!(untrusted.registered_handlers, 0);
    Ok(())
}
