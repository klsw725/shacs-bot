use serde_json::json;
use shacs_config::{RememberedPermissionMatcher, WorkspacePathScope};
use shacs_core::runtime::{
    remembered_permission_matcher_matches, safe_remembered_permission_matcher,
};
use std::error::Error;
use tempfile::TempDir;

mod remembered_permission_matcher_support;

use remembered_permission_matcher_support::{action, registry};

#[test]
fn remembered_permission_matcher_generates_safe_broader_patterns() -> Result<(), Box<dyn Error>> {
    let workspace = TempDir::new()?;
    std::fs::create_dir_all(workspace.path().join("src/nested"))?;
    std::fs::write(workspace.path().join("src/lib.rs"), "pub fn demo() {}")?;
    let registry = registry();

    let exec = action(
        &registry,
        "exec-1",
        "exec",
        json!({ "command": "cargo test --workspace" }),
    );
    let exec_pattern = safe_remembered_permission_matcher(&exec, workspace.path())?;
    assert_eq!(
        exec_pattern.matcher,
        RememberedPermissionMatcher::ExecPrefix {
            tokens: vec!["cargo".to_owned(), "test".to_owned()]
        }
    );
    assert_eq!(exec_pattern.preview, "exec cargo test *");
    assert!(remembered_permission_matcher_matches(
        &exec_pattern.matcher,
        &action(
            &registry,
            "exec-2",
            "exec",
            json!({ "command": "cargo test -- --nocapture" })
        ),
        workspace.path()
    )?);
    assert!(!remembered_permission_matcher_matches(
        &exec_pattern.matcher,
        &action(
            &registry,
            "exec-3",
            "exec",
            json!({ "command": "cargo build" })
        ),
        workspace.path()
    )?);

    let read = action(
        &registry,
        "read-1",
        "read_file",
        json!({ "path": "src/lib.rs" }),
    );
    let read_pattern = safe_remembered_permission_matcher(&read, workspace.path())?;
    assert_eq!(
        read_pattern.matcher,
        RememberedPermissionMatcher::WorkspacePath {
            tool_name: "read_file".to_owned(),
            path: "src/lib.rs".to_owned(),
            scope: WorkspacePathScope::Exact,
        }
    );
    assert_eq!(read_pattern.preview, "read_file src/lib.rs");

    let write = action(
        &registry,
        "write-1",
        "write_file",
        json!({ "path": "src/generated.rs" }),
    );
    let write_pattern = safe_remembered_permission_matcher(&write, workspace.path())?;
    assert_eq!(
        write_pattern.matcher,
        RememberedPermissionMatcher::WorkspacePath {
            tool_name: "write_file".to_owned(),
            path: "src/generated.rs".to_owned(),
            scope: WorkspacePathScope::Exact,
        }
    );

    let list = action(&registry, "list-1", "list_dir", json!({ "path": "src" }));
    let list_pattern = safe_remembered_permission_matcher(&list, workspace.path())?;
    assert_eq!(
        list_pattern.matcher,
        RememberedPermissionMatcher::WorkspacePath {
            tool_name: "list_dir".to_owned(),
            path: "src".to_owned(),
            scope: WorkspacePathScope::Subtree,
        }
    );
    assert!(remembered_permission_matcher_matches(
        &list_pattern.matcher,
        &action(
            &registry,
            "list-2",
            "list_dir",
            json!({ "path": "src/nested" })
        ),
        workspace.path()
    )?);

    let web = action(
        &registry,
        "web-1",
        "web_fetch",
        json!({ "url": "HTTPS://Example.COM/docs?token=not-persisted" }),
    );
    let web_pattern = safe_remembered_permission_matcher(&web, workspace.path())?;
    assert_eq!(
        web_pattern.matcher,
        RememberedPermissionMatcher::WebOrigin {
            origin: "https://example.com:443".to_owned()
        }
    );
    assert_eq!(web_pattern.preview, "web_fetch https://example.com:443");

    let mcp = action(
        &registry,
        "mcp-1",
        "mcp_server_tool_name",
        json!({ "path": "query" }),
    );
    let mcp_pattern = safe_remembered_permission_matcher(&mcp, workspace.path())?;
    assert_eq!(
        mcp_pattern.matcher,
        RememberedPermissionMatcher::McpTool {
            tool_name: "mcp_server_tool_name".to_owned()
        }
    );
    assert_eq!(mcp_pattern.preview, "mcp_server_tool_name");

    Ok(())
}
