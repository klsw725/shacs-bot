mod remembered_project_permission_support;

use remembered_project_permission_support::{
    exec_tool_call_response, registry, runtime_with_project_permissions, MockProvider,
    ProjectPermissionFixture,
};
use shacs_config::{
    RememberedPermissionEffect, RememberedPermissionMatcher, RememberedPermissionRule,
};
use shacs_core::runtime::{AgentLoopCommandResult, MessageBus};
use std::error::Error;
use std::fs;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

fn project_rule_count(fixture: &ProjectPermissionFixture) -> Result<usize, Box<dyn Error>> {
    Ok(fixture
        .store
        .load()?
        .project(&fixture.workspace_id)
        .map_or(0, <[_]>::len))
}

#[test]
fn permission_rule_command_lists_inspects_revokes_and_next_action_prompts(
) -> Result<(), Box<dyn Error>> {
    // Given: a runtime with one project-scoped allow rule for a cargo fmt action.
    let fixture = ProjectPermissionFixture::new()?;
    let rule = RememberedPermissionRule::new(
        RememberedPermissionEffect::Allow,
        RememberedPermissionMatcher::ExecPrefix {
            tokens: vec!["cargo".to_owned(), "fmt".to_owned()],
        },
        10,
    );
    let rule_prefix = rule.id().as_str()[..12].to_owned();
    fixture.store.mutate(|store| {
        store.upsert_rule(fixture.workspace_id.clone(), rule);
        Ok(())
    })?;
    let calls = Arc::new(AtomicUsize::new(0));
    let registry = registry(calls.clone());
    let client = MockProvider::new(vec![exec_tool_call_response(
        "exec-after-slash-revoke",
        "cargo fmt --check",
    )]);
    let mut runtime = runtime_with_project_permissions(
        fixture.workspace.path(),
        MessageBus::new(),
        &registry,
        &client,
        fixture.store.path().to_path_buf(),
        fixture.workspace_id.clone(),
    )?;

    // When: the active runtime lists, inspects, and revokes the remembered rule.
    let listed = runtime.process_direct("/permission rules", Some("cli:permission-rules"))?;
    let inspected = runtime.process_direct(
        format!("/permission inspect {rule_prefix}"),
        Some("cli:permission-rules"),
    )?;
    let revoked = runtime.process_direct(
        format!("/permission revoke {rule_prefix}"),
        Some("cli:permission-rules"),
    )?;
    let after_revoke = runtime.process_direct("again", Some("cli:permission-rules"))?;

    // Then: slash reads are command-only, revoke mutates exactly once, and the next action prompts.
    assert_eq!(listed.command, Some(AgentLoopCommandResult::Permission));
    assert_eq!(inspected.command, Some(AgentLoopCommandResult::Permission));
    assert_eq!(revoked.command, Some(AgentLoopCommandResult::Permission));
    assert_eq!(project_rule_count(&fixture)?, 0);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(after_revoke.stop_reason, "ask_user");
    Ok(())
}

#[test]
fn permission_rule_command_malformed_corrupt_and_ambiguous_inputs_do_not_mutate(
) -> Result<(), Box<dyn Error>> {
    // Given: malformed user input, a corrupt store containing secret-like bytes, and ambiguous ids.
    let corrupt = ProjectPermissionFixture::new()?;
    let raw_corrupt = r#"{"schemaVersion":1,"rawArguments":"sk-runtime-secret","projects":{}}"#;
    fs::write(corrupt.store.path(), raw_corrupt)?;
    let calls = Arc::new(AtomicUsize::new(0));
    let corrupt_registry = registry(calls);
    let client = MockProvider::new(Vec::new());
    let mut corrupt_runtime = runtime_with_project_permissions(
        corrupt.workspace.path(),
        MessageBus::new(),
        &corrupt_registry,
        &client,
        corrupt.store.path().to_path_buf(),
        corrupt.workspace_id.clone(),
    )?;

    // When: reads hit the corrupt store and malformed revoke has no id.
    let rules =
        corrupt_runtime.process_direct("/permission rules", Some("cli:permission-corrupt"))?;
    let inspect = corrupt_runtime
        .process_direct("/permission inspect abc123", Some("cli:permission-corrupt"))?;
    let malformed =
        corrupt_runtime.process_direct("/permission revoke", Some("cli:permission-corrupt"))?;

    // Then: output is redacted and the corrupt file is not rewritten.
    let combined = format!(
        "{}\n{}\n{}",
        rules.final_content.unwrap_or_default(),
        inspect.final_content.unwrap_or_default(),
        malformed.final_content.unwrap_or_default()
    );
    assert!(!combined.contains("sk-runtime-secret"));
    assert_eq!(fs::read_to_string(corrupt.store.path())?, raw_corrupt);

    // Given: two valid rules that share an id prefix.
    let ambiguous = ProjectPermissionFixture::new()?;
    let mut rules = Vec::new();
    for index in 0..17 {
        rules.push(RememberedPermissionRule::new(
            RememberedPermissionEffect::Allow,
            RememberedPermissionMatcher::ExecPrefix {
                tokens: vec!["cargo".to_owned(), format!("test-{index}")],
            },
            index,
        ));
    }
    let ambiguous_prefix = rules
        .iter()
        .enumerate()
        .find_map(|(index, left)| {
            rules.iter().skip(index + 1).find_map(|right| {
                (left.id().as_str()[..1] == right.id().as_str()[..1])
                    .then(|| left.id().as_str()[..1].to_owned())
            })
        })
        .ok_or("missing ambiguous prefix fixture")?;
    ambiguous.store.mutate(|store| {
        for rule in rules {
            store.upsert_rule(ambiguous.workspace_id.clone(), rule);
        }
        Ok(())
    })?;
    let before = project_rule_count(&ambiguous)?;
    let registry = registry(Arc::new(AtomicUsize::new(0)));
    let client = MockProvider::new(Vec::new());
    let mut runtime = runtime_with_project_permissions(
        ambiguous.workspace.path(),
        MessageBus::new(),
        &registry,
        &client,
        ambiguous.store.path().to_path_buf(),
        ambiguous.workspace_id.clone(),
    )?;

    // When: revoke receives an ambiguous prefix.
    let ambiguous_revoke = runtime.process_direct(
        format!("/permission revoke {ambiguous_prefix}"),
        Some("cli:permission-ambiguous"),
    )?;

    // Then: no rule is removed.
    assert_eq!(
        ambiguous_revoke.command,
        Some(AgentLoopCommandResult::Permission)
    );
    assert_eq!(project_rule_count(&ambiguous)?, before);
    Ok(())
}
