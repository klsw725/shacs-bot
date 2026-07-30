mod remembered_project_permission_support;

use remembered_project_permission_support::{
    exec_tool_call_response, registry, runtime_with_project_permissions,
    runtime_with_project_permissions_interactive, MockProvider, ProjectPermissionFixture,
};
use shacs_config::{RememberedPermissionEffect, RememberedPermissionStoreErrorKind};
use shacs_core::runtime::MessageBus;
use shacs_providers::LlmResponse;
use std::error::Error;
use std::fs;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

#[test]
fn project_deny_persists_before_cancellation_and_reuse_updates_metadata(
) -> Result<(), Box<dyn Error>> {
    let fixture = ProjectPermissionFixture::new()?;
    let bus = MessageBus::new();
    let calls = Arc::new(AtomicUsize::new(0));
    let registry = registry(calls.clone());
    let client = MockProvider::new(vec![
        exec_tool_call_response("exec-deny", "cargo fmt --check"),
        exec_tool_call_response("exec-deny-reused", "cargo fmt --check"),
        LlmResponse {
            content: Some("denied by memory".to_owned()),
            ..LlmResponse::default()
        },
    ]);
    let mut runtime = runtime_with_project_permissions(
        fixture.workspace.path(),
        bus,
        &registry,
        &client,
        fixture.store.path().to_path_buf(),
        fixture.workspace_id.clone(),
    )?;

    let first = runtime.process_direct("start", Some("cli:project-deny"))?;
    assert_eq!(first.stop_reason, "ask_user");
    let cancelled = runtime.process_direct("deny_project", Some("cli:project-deny"))?;
    let stored = fixture.store.load()?;
    let rule = stored
        .project(&fixture.workspace_id)
        .and_then(|rules| rules.first())
        .ok_or("missing stored project deny")?;
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(cancelled.stop_reason, "permission_denied_by_user");
    assert_eq!(rule.effect(), RememberedPermissionEffect::Deny);
    assert_eq!(rule.use_count(), 0);

    let reused = runtime.process_direct("again", Some("cli:project-deny"))?;
    let updated = fixture.store.load()?;
    let updated_rule = updated
        .project(&fixture.workspace_id)
        .and_then(|rules| rules.first())
        .ok_or("missing updated project deny")?;
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_ne!(reused.stop_reason, "ask_user");
    assert!(reused.ask_user_options.is_empty());
    assert_eq!(reused.final_content.as_deref(), Some("denied by memory"));
    assert_eq!(updated_rule.use_count(), 1);
    assert!(updated_rule.last_used_unix_ms() >= updated_rule.created_unix_ms());
    Ok(())
}

#[test]
fn project_save_failure_blocks_execution_without_session_downgrade() -> Result<(), Box<dyn Error>> {
    let fixture = ProjectPermissionFixture::new()?;
    let bus = MessageBus::new();
    let calls = Arc::new(AtomicUsize::new(0));
    let registry = registry(calls.clone());
    let client = MockProvider::new(vec![
        exec_tool_call_response("exec-save-fails", "cargo fmt --check"),
        exec_tool_call_response("exec-after-failure", "cargo fmt --check"),
    ]);
    let mut runtime = runtime_with_project_permissions(
        fixture.workspace.path(),
        bus,
        &registry,
        &client,
        fixture.store.path().to_path_buf(),
        fixture.workspace_id.clone(),
    )?;

    assert_eq!(
        runtime
            .process_direct("start", Some("cli:project-save-fails"))?
            .stop_reason,
        "ask_user"
    );
    fs::create_dir_all(fixture.store.path())?;
    let blocked = runtime.process_direct("approve_project", Some("cli:project-save-fails"))?;
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(blocked.stop_reason, "permission_project_store_unavailable");

    let after_failure = runtime.process_direct("again", Some("cli:project-save-fails"))?;
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(after_failure.stop_reason, "ask_user");
    Ok(())
}

#[cfg(unix)]
#[test]
fn unreadable_project_store_asks_interactive_and_denies_noninteractive_with_redacted_error(
) -> Result<(), Box<dyn Error>> {
    let fixture = ProjectPermissionFixture::new()?;
    fs::write(fixture.store.path(), "sk-project-secret")?;
    fs::set_permissions(fixture.store.path(), fs::Permissions::from_mode(0o000))?;
    let load_error = fixture
        .store
        .load()
        .expect_err("store should be unreadable");
    assert_eq!(load_error.kind(), RememberedPermissionStoreErrorKind::Io);
    assert!(!format!("{load_error:?}").contains("sk-project-secret"));

    let calls = Arc::new(AtomicUsize::new(0));
    let registry = registry(calls.clone());
    let interactive_client = MockProvider::new(vec![exec_tool_call_response(
        "exec-unreadable-interactive",
        "cargo fmt --check",
    )]);
    let mut interactive = runtime_with_project_permissions_interactive(
        fixture.workspace.path(),
        MessageBus::new(),
        &registry,
        &interactive_client,
        fixture.store.path().to_path_buf(),
        fixture.workspace_id.clone(),
        true,
    )?;
    let asked = interactive.process_direct("start", Some("cli:unreadable-interactive"))?;
    assert_eq!(asked.stop_reason, "ask_user");
    assert_eq!(calls.load(Ordering::SeqCst), 0);

    let noninteractive_client = MockProvider::new(vec![
        exec_tool_call_response("exec-unreadable-noninteractive", "cargo fmt --check"),
        LlmResponse {
            content: Some("denied without prompt".to_owned()),
            ..LlmResponse::default()
        },
    ]);
    let mut noninteractive = runtime_with_project_permissions_interactive(
        fixture.workspace.path(),
        MessageBus::new(),
        &registry,
        &noninteractive_client,
        fixture.store.path().to_path_buf(),
        fixture.workspace_id.clone(),
        false,
    )?;
    let denied = noninteractive.process_direct("again", Some("cli:unreadable-noninteractive"))?;
    assert_ne!(denied.stop_reason, "ask_user");
    assert!(denied.ask_user_options.is_empty());
    assert_eq!(
        denied.final_content.as_deref(),
        Some("denied without prompt")
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    fs::set_permissions(fixture.store.path(), fs::Permissions::from_mode(0o600))?;
    Ok(())
}
