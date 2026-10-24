use shacs_cli::{
    parse_cli_args, run_command, CliCommand, PermissionsCommand, PermissionsIdOptions,
    PermissionsListOptions,
};
use shacs_config::{
    save_config_to_path, Config, ConfigContext, RememberedPermissionEffect,
    RememberedPermissionFileStore, RememberedPermissionMatcher, RememberedPermissionRule,
    WorkspacePermissionId,
};
use std::fs;
use std::path::PathBuf;
use tempfile::TempDir;

#[test]
fn remembered_permissions_cli_parses_top_level_commands() -> Result<(), Box<dyn std::error::Error>>
{
    let parsed = parse_cli_args([
        "--config",
        "/tmp/config.json",
        "permissions",
        "list",
        "--workspace",
        "/tmp/workspace",
    ])?;
    let CliCommand::Permissions(PermissionsCommand::List(options)) = parsed else {
        return Err("expected permissions list command".into());
    };
    assert_eq!(options.config_path, Some(PathBuf::from("/tmp/config.json")));
    assert_eq!(
        options.workspace_override,
        Some(PathBuf::from("/tmp/workspace"))
    );

    let parsed = parse_cli_args(["permissions", "inspect", "abcdef"])?;
    assert!(matches!(
        parsed,
        CliCommand::Permissions(PermissionsCommand::Inspect(PermissionsIdOptions { .. }))
    ));
    let parsed = parse_cli_args(["permission", "revoke", "abcdef"])?;
    assert!(matches!(
        parsed,
        CliCommand::Permissions(PermissionsCommand::Revoke(PermissionsIdOptions { .. }))
    ));
    assert!(parse_cli_args(["permissions", "add"]).is_err());
    Ok(())
}

#[test]
fn remembered_permissions_cli_lists_inspects_and_revokes_current_workspace_rules(
) -> Result<(), Box<dyn std::error::Error>> {
    let fixture = CliFixture::new()?;
    let cargo_rule = fixture.add_rule(exec_rule("cargo", 10))?;
    let test_rule = fixture.add_rule(exec_rule("test", 11))?;
    let other_workspace =
        WorkspacePermissionId::from_canonical_workspace_path("/tmp/other-workspace");
    fixture.store.mutate(|store| {
        store.upsert_rule(other_workspace, exec_rule("other", 12));
        Ok(())
    })?;

    let list = run_command(CliCommand::Permissions(PermissionsCommand::List(
        fixture.list_options(),
    )))?;
    assert!(list.contains("Remembered permissions"));
    assert!(list.contains(&cargo_rule.id().as_str()[..12]));
    assert!(list.contains(&test_rule.id().as_str()[..12]));
    assert!(!list.contains("other"));
    assert!(!list.contains(fixture.workspace.to_string_lossy().as_ref()));

    let inspect = run_command(CliCommand::Permissions(PermissionsCommand::Inspect(
        fixture.id_options(&cargo_rule.id().as_str()[..16]),
    )))?;
    assert!(inspect.contains(&cargo_rule.id().as_str()[..12]));
    assert!(inspect.contains("exec cargo *"));

    let revoked = run_command(CliCommand::Permissions(PermissionsCommand::Revoke(
        fixture.id_options(&cargo_rule.id().as_str()[..16]),
    )))?;
    assert!(revoked.contains("Revoked"));
    assert!(revoked.contains(&cargo_rule.id().as_str()[..12]));
    assert!(!revoked.contains(fixture.workspace.to_string_lossy().as_ref()));

    let after = run_command(CliCommand::Permissions(PermissionsCommand::List(
        fixture.list_options(),
    )))?;
    assert!(!after.contains(&cargo_rule.id().as_str()[..12]));
    assert!(after.contains(&test_rule.id().as_str()[..12]));
    Ok(())
}

#[test]
fn remembered_permissions_cli_read_commands_do_not_create_store_and_fail_redacted(
) -> Result<(), Box<dyn std::error::Error>> {
    let fixture = CliFixture::new()?;
    let missing_list = run_command(CliCommand::Permissions(PermissionsCommand::List(
        fixture.list_options(),
    )))?;
    assert!(missing_list.contains("Rules: 0"));
    assert!(!fixture.store.path().exists());

    let rules = fixture.add_rules_with_shared_prefix()?;
    let ambiguous_prefix = shared_prefix(&rules).ok_or("missing shared prefix")?;
    let before = fs::read(fixture.store.path())?;
    let error = run_command(CliCommand::Permissions(PermissionsCommand::Revoke(
        fixture.id_options(&ambiguous_prefix),
    )))
    .expect_err("ambiguous prefix must fail")
    .to_string();
    assert!(error.contains("ambiguous"));
    assert_eq!(fs::read(fixture.store.path())?, before);

    fs::write(
        fixture.store.path(),
        r#"{"schemaVersion":1,"rawArguments":"sk-permission-secret","projects":{}}"#,
    )?;
    let error = run_command(CliCommand::Permissions(PermissionsCommand::List(
        fixture.list_options(),
    )))
    .expect_err("malformed store must fail")
    .to_string();
    assert!(error.contains("remembered permission store"));
    assert!(!error.contains("sk-permission-secret"));
    Ok(())
}

struct CliFixture {
    _root: TempDir,
    config_path: PathBuf,
    workspace: PathBuf,
    store: RememberedPermissionFileStore,
    workspace_id: WorkspacePermissionId,
}

impl CliFixture {
    fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let root = tempfile::tempdir()?;
        let data_dir = root.path().join("data");
        let workspace = root.path().join("workspace");
        fs::create_dir_all(&data_dir)?;
        fs::create_dir_all(&workspace)?;
        let config_path = data_dir.join("config.json");
        let mut config = Config::default();
        config.agents.defaults.workspace = workspace.to_string_lossy().to_string();
        save_config_to_path(&config, &config_path)?;
        let context = ConfigContext {
            config_path: config_path.clone(),
            data_dir: data_dir.clone(),
            workspace: workspace.clone(),
        };
        let store = RememberedPermissionFileStore::for_context(&context);
        let workspace_id = WorkspacePermissionId::from_canonical_workspace_path(
            workspace.canonicalize()?.to_string_lossy().as_ref(),
        );
        Ok(Self {
            _root: root,
            config_path,
            workspace,
            store,
            workspace_id,
        })
    }

    fn add_rule(
        &self,
        rule: RememberedPermissionRule,
    ) -> Result<RememberedPermissionRule, Box<dyn std::error::Error>> {
        self.store.mutate(|store| {
            store.upsert_rule(self.workspace_id.clone(), rule.clone());
            Ok(())
        })?;
        Ok(rule)
    }

    fn add_rules_with_shared_prefix(
        &self,
    ) -> Result<Vec<RememberedPermissionRule>, Box<dyn std::error::Error>> {
        let mut rules = Vec::new();
        for index in 0..64 {
            let rule = exec_rule(&format!("cmd-{index}"), 100 + index);
            self.add_rule(rule.clone())?;
            rules.push(rule);
            if shared_prefix(&rules).is_some() {
                return Ok(rules);
            }
        }
        Err("could not build ambiguous prefix fixture".into())
    }

    fn list_options(&self) -> PermissionsListOptions {
        PermissionsListOptions {
            config_path: Some(self.config_path.clone()),
            workspace_override: Some(self.workspace.clone()),
        }
    }

    fn id_options(&self, rule_id_prefix: &str) -> PermissionsIdOptions {
        PermissionsIdOptions {
            config_path: Some(self.config_path.clone()),
            workspace_override: Some(self.workspace.clone()),
            rule_id_prefix: rule_id_prefix.to_owned(),
        }
    }
}

fn shared_prefix(rules: &[RememberedPermissionRule]) -> Option<String> {
    for (left_index, left) in rules.iter().enumerate() {
        for right in rules.iter().skip(left_index + 1) {
            let prefix = left
                .id()
                .as_str()
                .chars()
                .zip(right.id().as_str().chars())
                .take_while(|(left, right)| left == right)
                .map(|(left, _right)| left)
                .collect::<String>();
            if !prefix.is_empty() {
                return Some(prefix);
            }
        }
    }
    None
}

fn exec_rule(command: &str, created_unix_ms: u64) -> RememberedPermissionRule {
    RememberedPermissionRule::new(
        RememberedPermissionEffect::Allow,
        RememberedPermissionMatcher::ExecPrefix {
            tokens: vec![command.to_owned()],
        },
        created_unix_ms,
    )
}
