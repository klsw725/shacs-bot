use serde_json::json;
use shacs_config::RememberedPermissionMatcher;
use shacs_core::runtime::safe_remembered_permission_matcher;
use std::error::Error;
use tempfile::TempDir;

mod remembered_permission_matcher_support;

use remembered_permission_matcher_support::{action, registry};

#[test]
fn remembered_permission_matcher_falls_back_for_unsafe_inputs_without_leaking(
) -> Result<(), Box<dyn Error>> {
    let workspace = TempDir::new()?;
    std::fs::create_dir_all(workspace.path().join("src"))?;
    std::fs::write(workspace.path().join("src/lib.rs"), "pub fn demo() {}")?;
    let outside = TempDir::new()?;
    std::fs::write(outside.path().join("secret.txt"), "secret")?;
    #[cfg(unix)]
    std::os::unix::fs::symlink(outside.path(), workspace.path().join("link"))?;
    let registry = registry();

    let complex_exec = action(
        &registry,
        "exec-1",
        "exec",
        json!({ "command": "cargo test; rm -rf ." }),
    );
    let complex_pattern = safe_remembered_permission_matcher(&complex_exec, workspace.path())?;
    assert_eq!(
        complex_pattern.matcher,
        RememberedPermissionMatcher::ExactAction {
            action_digest: complex_exec.action_digest.clone()
        }
    );
    assert!(!complex_pattern.preview.contains("rm -rf"));
    assert_eq!(
        complex_pattern.preview,
        format!("exact action {}", &complex_exec.action_digest[..12])
    );

    let web_search = action(
        &registry,
        "search-1",
        "web_search",
        json!({ "query": "secret query" }),
    );
    let web_search_pattern = safe_remembered_permission_matcher(&web_search, workspace.path())?;
    assert_eq!(
        web_search_pattern.matcher,
        RememberedPermissionMatcher::ExactAction {
            action_digest: web_search.action_digest.clone()
        }
    );

    #[cfg(unix)]
    {
        let escaping = action(
            &registry,
            "read-1",
            "read_file",
            json!({ "path": "link/secret.txt" }),
        );
        let escaping_pattern = safe_remembered_permission_matcher(&escaping, workspace.path())?;
        assert_eq!(
            escaping_pattern.matcher,
            RememberedPermissionMatcher::ExactAction {
                action_digest: escaping.action_digest.clone()
            }
        );
        assert!(!escaping_pattern.preview.contains("secret.txt"));
    }

    let credential_url = action(
        &registry,
        "web-1",
        "web_fetch",
        json!({ "url": "https://user:pass@example.com/private" }),
    );
    let credential_pattern = safe_remembered_permission_matcher(&credential_url, workspace.path())?;
    assert_eq!(
        credential_pattern.matcher,
        RememberedPermissionMatcher::ExactAction {
            action_digest: credential_url.action_digest.clone()
        }
    );
    assert!(!credential_pattern.preview.contains("user"));
    assert!(!credential_pattern.preview.contains("pass"));
    assert!(!credential_pattern.preview.contains("example.com/private"));

    let bad_port = action(
        &registry,
        "web-2",
        "web_fetch",
        json!({ "url": "https://example.com:bad/docs" }),
    );
    let bad_port_pattern = safe_remembered_permission_matcher(&bad_port, workspace.path())?;
    assert_eq!(
        bad_port_pattern.matcher,
        RememberedPermissionMatcher::ExactAction {
            action_digest: bad_port.action_digest.clone()
        }
    );

    let redacted = action(
        &registry,
        "redacted-1",
        "read_file",
        json!({ "path": "sk-raw-secret" }),
    );
    let redacted_pattern = safe_remembered_permission_matcher(&redacted, workspace.path())?;
    assert_eq!(
        redacted_pattern.matcher,
        RememberedPermissionMatcher::ExactAction {
            action_digest: redacted.action_digest.clone()
        }
    );
    assert!(!redacted_pattern.preview.contains("sk-raw-secret"));

    Ok(())
}
