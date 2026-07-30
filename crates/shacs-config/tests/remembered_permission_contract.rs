use shacs_config::{
    RememberedPermissionEffect, RememberedPermissionMatcher, RememberedPermissionRule,
    RememberedPermissionStore, RememberedPermissionStoreErrorKind, WorkspacePermissionId,
};

#[test]
fn remembered_permission_contract_round_trips() -> Result<(), Box<dyn std::error::Error>> {
    let workspace_id = WorkspacePermissionId::from_canonical_workspace_path(
        "/Users/alice/workspace/raw-workspace-sentinel",
    );
    let matcher = RememberedPermissionMatcher::ExecPrefix {
        tokens: vec!["cargo".to_owned(), "test".to_owned()],
    };
    let allow_rule = RememberedPermissionRule::new(
        RememberedPermissionEffect::Allow,
        matcher.clone(),
        1_777_000_000_000,
    );
    let deny_rule = RememberedPermissionRule::new(
        RememberedPermissionEffect::Deny,
        matcher.clone(),
        1_777_000_000_001,
    );
    let overlapping_deny = RememberedPermissionRule::new(
        RememberedPermissionEffect::Deny,
        RememberedPermissionMatcher::WorkspacePath {
            tool_name: "filesystem".to_owned(),
            path: "src".to_owned(),
            scope: shacs_config::WorkspacePathScope::Subtree,
        },
        1_777_000_000_002,
    );
    let overlapping_allow = RememberedPermissionRule::new(
        RememberedPermissionEffect::Allow,
        RememberedPermissionMatcher::WorkspacePath {
            tool_name: "filesystem".to_owned(),
            path: "src/lib.rs".to_owned(),
            scope: shacs_config::WorkspacePathScope::Exact,
        },
        1_777_000_000_003,
    );

    let mut store = RememberedPermissionStore::default();
    store.upsert_rule(workspace_id.clone(), allow_rule.clone());
    store.upsert_rule(workspace_id.clone(), deny_rule.clone());
    store.upsert_rule(workspace_id.clone(), overlapping_deny);
    store.upsert_rule(workspace_id.clone(), overlapping_allow);

    let bucket = store
        .project(&workspace_id)
        .ok_or("missing project bucket")?;
    assert_eq!(bucket.len(), 3);
    assert_eq!(bucket[0].effect(), RememberedPermissionEffect::Deny);
    assert_ne!(allow_rule.id(), deny_rule.id());
    assert_eq!(deny_rule.id().as_str().len(), 64);

    let serialized = store.to_json_string()?;
    assert!(!serialized.contains("raw-workspace-sentinel"));

    let reloaded = RememberedPermissionStore::from_json_str(&serialized)?;
    assert_eq!(reloaded, store);

    let reordered = r#"{
      "projects": {
        "WORKSPACE_ID": {
          "rules": [
            {
              "lastUsedUnixMs": 1777000000001,
              "id": "RULE_ID",
              "matcher": {"tokens": ["cargo", "test"], "kind": "exec_prefix"},
              "effect": "deny",
              "useCount": 0,
              "createdUnixMs": 1777000000001
            }
          ]
        }
      },
      "schemaVersion": 1
    }"#
    .replace("WORKSPACE_ID", workspace_id.as_str())
    .replace("RULE_ID", deny_rule.id().as_str());
    let reordered_store = RememberedPermissionStore::from_json_str(&reordered)?;
    let reordered_rule = &reordered_store
        .project(&workspace_id)
        .ok_or("missing reordered bucket")?[0];
    assert_eq!(reordered_rule.id(), deny_rule.id());

    let unknown_schema = serialized.replace("\"schemaVersion\": 1", "\"schemaVersion\": 2");
    let error = RememberedPermissionStore::from_json_str(&unknown_schema)
        .expect_err("unknown schema must fail closed");
    assert_eq!(
        error.kind(),
        RememberedPermissionStoreErrorKind::UnknownSchemaVersion
    );

    let forbidden_raw = serialized.replace(
        "\"schemaVersion\": 1",
        "\"schemaVersion\": 1, \"rawWorkspacePath\": \"raw-workspace-sentinel\"",
    );
    let error = RememberedPermissionStore::from_json_str(&forbidden_raw)
        .expect_err("forbidden raw sentinel field must fail closed");
    assert_eq!(
        error.kind(),
        RememberedPermissionStoreErrorKind::ForbiddenRawField
    );
    assert!(!error.to_string().contains("raw-workspace-sentinel"));

    Ok(())
}

#[test]
fn remembered_permission_contract_ids_distinguish_embedded_delimiters() {
    let single_token = RememberedPermissionRule::new(
        RememberedPermissionEffect::Allow,
        RememberedPermissionMatcher::ExecPrefix {
            tokens: vec!["a\nb".to_owned()],
        },
        1_777_000_000_010,
    );
    let two_tokens = RememberedPermissionRule::new(
        RememberedPermissionEffect::Allow,
        RememberedPermissionMatcher::ExecPrefix {
            tokens: vec!["a".to_owned(), "b".to_owned()],
        },
        1_777_000_000_010,
    );

    assert_ne!(single_token.id(), two_tokens.id());

    let delimiter_token = RememberedPermissionRule::new(
        RememberedPermissionEffect::Allow,
        RememberedPermissionMatcher::ExecPrefix {
            tokens: vec!["kind=exec_prefix;tokens=2".to_owned()],
        },
        1_777_000_000_011,
    );
    let delimiter_tokens = RememberedPermissionRule::new(
        RememberedPermissionEffect::Allow,
        RememberedPermissionMatcher::ExecPrefix {
            tokens: vec!["kind=exec_prefix".to_owned(), "tokens=2".to_owned()],
        },
        1_777_000_000_011,
    );

    assert_ne!(delimiter_token.id(), delimiter_tokens.id());

    let newline_path = RememberedPermissionRule::new(
        RememberedPermissionEffect::Allow,
        RememberedPermissionMatcher::WorkspacePath {
            tool_name: "filesystem".to_owned(),
            path: "src\nlib.rs".to_owned(),
            scope: shacs_config::WorkspacePathScope::Exact,
        },
        1_777_000_000_012,
    );
    let split_path = RememberedPermissionRule::new(
        RememberedPermissionEffect::Allow,
        RememberedPermissionMatcher::WorkspacePath {
            tool_name: "filesystem\nsrc".to_owned(),
            path: "lib.rs".to_owned(),
            scope: shacs_config::WorkspacePathScope::Exact,
        },
        1_777_000_000_012,
    );

    assert_ne!(newline_path.id(), split_path.id());
}

#[test]
fn remembered_permission_contract_rejects_forbidden_raw_sentinel_fields() {
    let forbidden = r#"{
      "schemaVersion": 1,
      "rawArguments": "--raw-argument-sentinel",
      "projects": {
        "workspace:sha256:fixture": {"rules": []}
      }
    }"#;
    let error = RememberedPermissionStore::from_json_str(forbidden)
        .expect_err("raw arguments must fail closed");
    assert_eq!(
        error.kind(),
        RememberedPermissionStoreErrorKind::ForbiddenRawField
    );
    assert!(!error.to_string().contains("--raw-argument-sentinel"));

    let nested_secret = r#"{
      "schemaVersion": 1,
      "projects": {
        "workspace:sha256:fixture": {
          "rules": [],
          "metadata": {"secret": "sk-secret-sentinel"}
        }
      }
    }"#;
    let error = RememberedPermissionStore::from_json_str(nested_secret)
        .expect_err("raw secret must fail closed");
    assert_eq!(
        error.kind(),
        RememberedPermissionStoreErrorKind::ForbiddenRawField
    );
    assert!(!error.to_string().contains("sk-secret-sentinel"));
}
