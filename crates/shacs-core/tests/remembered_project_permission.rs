mod remembered_project_permission_support;

use remembered_project_permission_support::{
    exec_tool_call_response, registry, runtime_with_project_permissions, MockProvider,
    ProjectPermissionFixture,
};
use serde_json::{json, Map};
use shacs_config::{
    RememberedPermissionEffect, RememberedPermissionMatcher, RememberedPermissionRule,
};
use shacs_core::runtime::MessageBus;
use shacs_providers::{LlmResponse, ToolCallRequest};
use std::error::Error;
use std::fs;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

fn write_file_tool_call_response(call_id: &str, path: &str) -> LlmResponse {
    LlmResponse {
        finish_reason: "tool_calls".to_owned(),
        tool_calls: vec![ToolCallRequest::new(
            call_id,
            "write_file",
            Map::from_iter([
                ("path".to_owned(), json!(path)),
                ("content".to_owned(), json!("blocked")),
            ]),
        )],
        ..LlmResponse::default()
    }
}

#[test]
fn remembered_project_permission_approval_persists_before_reloaded_execution(
) -> Result<(), Box<dyn Error>> {
    // Given: a runtime wired to the selected project permission store path.
    let fixture = ProjectPermissionFixture::new()?;
    let bus = MessageBus::new();
    let calls = Arc::new(AtomicUsize::new(0));
    let registry = registry(calls.clone());
    let client = MockProvider::new(vec![
        exec_tool_call_response("exec-1", "cargo fmt --check"),
        LlmResponse {
            content: Some("approved".to_owned()),
            ..LlmResponse::default()
        },
        exec_tool_call_response("exec-2", "cargo fmt --check"),
        LlmResponse {
            content: Some("reused".to_owned()),
            ..LlmResponse::default()
        },
    ]);
    let mut first_runtime = runtime_with_project_permissions(
        fixture.workspace.path(),
        bus.clone(),
        &registry,
        &client,
        fixture.store.path().to_path_buf(),
        fixture.workspace_id.clone(),
    )?;
    assert_eq!(
        first_runtime
            .process_direct("start", Some("cli:remembered-project-allow"))?
            .stop_reason,
        "ask_user"
    );
    let _approval_outbound = bus.consume_outbound().ok_or("missing approval outbound")?;

    // When: the user approves for the project, then a new runtime replays the same action.
    let approved =
        first_runtime.process_direct("approve_project", Some("cli:remembered-project-allow"))?;
    assert_eq!(approved.final_content.as_deref(), Some("approved"));
    drop(first_runtime);
    let mut reloaded_runtime = runtime_with_project_permissions(
        fixture.workspace.path(),
        bus,
        &registry,
        &client,
        fixture.store.path().to_path_buf(),
        fixture.workspace_id.clone(),
    )?;
    let calls_before_reuse = calls.load(Ordering::SeqCst);
    let reused = reloaded_runtime.process_direct("again", Some("cli:remembered-project-allow"))?;
    let store = fixture.store.load()?;
    let rules = store
        .project(&fixture.workspace_id)
        .ok_or("missing project rules")?;

    // Then: replay executes without a prompt and updates project rule metadata.
    assert_eq!(calls.load(Ordering::SeqCst), calls_before_reuse + 1);
    assert_ne!(reused.stop_reason, "ask_user");
    assert_eq!(reused.final_content.as_deref(), Some("reused"));
    assert_eq!(rules.len(), 1);
    assert_eq!(rules[0].effect(), RememberedPermissionEffect::Allow);
    assert_eq!(rules[0].use_count(), 1);
    assert!(rules[0].last_used_unix_ms() >= rules[0].created_unix_ms());
    Ok(())
}

#[test]
fn remembered_project_permission_external_revoke_is_observed_on_next_action(
) -> Result<(), Box<dyn Error>> {
    // Given: a project allow rule matching the next tool call.
    let fixture = ProjectPermissionFixture::new()?;
    let rule = RememberedPermissionRule::new(
        RememberedPermissionEffect::Allow,
        RememberedPermissionMatcher::ExecPrefix {
            tokens: vec!["cargo".to_owned(), "fmt".to_owned()],
        },
        10,
    );
    fixture.store.mutate(|store| {
        store.upsert_rule(fixture.workspace_id.clone(), rule.clone());
        Ok(())
    })?;
    let calls = Arc::new(AtomicUsize::new(0));
    let registry = registry(calls.clone());
    let client = MockProvider::new(vec![exec_tool_call_response(
        "exec-revoked",
        "cargo fmt --check",
    )]);
    let mut loop_runtime = runtime_with_project_permissions(
        fixture.workspace.path(),
        MessageBus::new(),
        &registry,
        &client,
        fixture.store.path().to_path_buf(),
        fixture.workspace_id.clone(),
    )?;

    // When: another process revokes the rule before the next action.
    fixture
        .store
        .remove_rule_by_prefix(&fixture.workspace_id, &rule.id().as_str()[..16])?;
    let result = loop_runtime.process_direct("again", Some("cli:remembered-project-revoke"))?;

    // Then: no process-lifetime cache allows the stale project decision.
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(result.stop_reason, "ask_user");
    Ok(())
}

#[test]
fn remembered_project_permission_direct_store_file_target_is_protected(
) -> Result<(), Box<dyn Error>> {
    // Given: a write_file call directly targets the selected permissions.json path.
    let fixture = ProjectPermissionFixture::new()?;
    let calls = Arc::new(AtomicUsize::new(0));
    let registry = registry(calls.clone());
    let client = MockProvider::new(vec![
        write_file_tool_call_response(
            "write-store",
            fixture.store.path().to_string_lossy().as_ref(),
        ),
        LlmResponse {
            content: Some("protected".to_owned()),
            ..LlmResponse::default()
        },
    ]);
    let mut runtime = runtime_with_project_permissions(
        fixture.workspace.path(),
        MessageBus::new(),
        &registry,
        &client,
        fixture.store.path().to_path_buf(),
        fixture.workspace_id.clone(),
    )?;

    // When: the provider asks to write that file.
    let result = runtime.process_direct("modify store", Some("cli:remembered-project-protect"))?;

    // Then: the file-target tool never executes and no approval interrupt is raised.
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_ne!(result.stop_reason, "ask_user");
    Ok(())
}

#[test]
fn remembered_project_permission_corrupt_store_fails_closed() -> Result<(), Box<dyn Error>> {
    // Given: the selected store is malformed and contains a secret-like value.
    let fixture = ProjectPermissionFixture::new()?;
    fs::write(
        fixture.store.path(),
        r#"{"schemaVersion":1,"rawArguments":"sk-project-secret","projects":{}}"#,
    )?;
    let calls = Arc::new(AtomicUsize::new(0));
    let registry = registry(calls.clone());
    let client = MockProvider::new(vec![exec_tool_call_response(
        "exec-corrupt",
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

    // When: the next action needs a permission decision.
    let result = runtime.process_direct("again", Some("cli:remembered-project-corrupt"))?;

    // Then: store-unavailable fails closed to an interactive ask and runs zero tools.
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(result.stop_reason, "ask_user");
    Ok(())
}
