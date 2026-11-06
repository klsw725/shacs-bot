use super::*;
use crate::runtime::auto_mode::classifier_slots::SlotOverrides;
use crate::runtime::auto_mode::permission_environment::PermissionEnvironmentInput;
use crate::runtime::auto_mode::permission_rule::RawRuleLists;
use crate::runtime::{
    normalize_runtime_tool_call, PermissionModeSnapshot, PermissionedActionInput,
    PermissionedActionOrigin, RuntimeToolCall,
};
use crate::tools::ToolRegistry;
use serde_json::json;
use std::error::Error;
use std::fs;

struct Folders {
    _root: tempfile::TempDir,
    workspace: PathBuf,
    data: PathBuf,
    home: PathBuf,
}

fn folders() -> Result<Folders, Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let base = fs::canonicalize(root.path())?;
    let (workspace, data, home) = (base.join("ws"), base.join("data"), base.join("home"));
    for folder in [&workspace, &data, &home] {
        fs::create_dir_all(folder)?;
    }
    fs::write(data.join("auth.json"), "{}")?;
    Ok(Folders {
        _root: root,
        workspace,
        data,
        home,
    })
}

fn environment(
    folders: &Folders,
    allow: &[&str],
    workspace_is_data: bool,
) -> PermissionEnvironment {
    let allow = allow
        .iter()
        .map(|rule| (*rule).to_owned())
        .collect::<Vec<_>>();
    let workspace = if workspace_is_data {
        &folders.data
    } else {
        &folders.workspace
    };
    PermissionEnvironment::new(PermissionEnvironmentInput {
        workspace,
        data_folder: &folders.data,
        config_file: &folders.data.join("config.json"),
        home: &folders.home,
        rules: RawRuleLists {
            allow: &allow,
            ask: &[],
            deny: &[],
        },
        mcp_servers: &[],
        extra_skill_roots: &[],
        extra_context_files: &[],
        configured_targets: &[],
        classify_all_shell: false,
        slots: SlotOverrides::default(),
        classifier_model: None,
        env_file: &shacs_config::EnvFile::default(),
        env_file_current: true,
    })
}

fn materials(
    environment: &PermissionEnvironment,
    tool: &str,
    arguments: Value,
) -> PermissionMaterials {
    let action = normalize_runtime_tool_call(
        &ToolRegistry::new(),
        &RuntimeToolCall::new("call_1", tool, arguments),
        PermissionedActionInput {
            session_id: "cli:direct".to_owned(),
            turn_id: "turn_1".to_owned(),
            origin: PermissionedActionOrigin::UserTurn,
            permission_mode_snapshot: PermissionModeSnapshot::default(),
            containment_snapshot: None,
            intent_snapshot: None,
            permission_environment: None,
        },
    );
    permission_materials(MaterialsInput {
        action: &action,
        environment,
        auto_mode: true,
        allow_workspace_edits: true,
        preflight_refusal: None,
    })
}

#[test]
fn secret_files_are_found_through_file_tools_command_paths_and_command_text(
) -> Result<(), Box<dyn Error>> {
    // Given
    let folders = folders()?;
    let data_workspace = environment(&folders, &[], true);
    let elsewhere = environment(&folders, &[], false);
    let data = folders.data.display();

    // When / Then
    assert!(materials(&data_workspace, "read_file", json!({"path": "auth.json"})).secret_read);
    let cat = materials(
        &elsewhere,
        "exec",
        json!({"command": format!("cd {data} && cat auth.json")}),
    );
    assert!(cat.secret_read && !cat.secret_read_by_text);
    let script = materials(
        &elsewhere,
        "exec",
        json!({"command": "python3 - <<'EOF'\nopen('auth.json')\nEOF"}),
    );
    assert!(script.secret_read && script.secret_read_by_text);
    let redirect = materials(
        &elsewhere,
        "exec",
        json!({"command": format!("echo x > {data}/auth.json")}),
    );
    assert!(redirect.secret_write);
    assert!(materials(&data_workspace, "write_file", json!({"path": "auth.json"})).secret_write);
    assert!(!materials(&elsewhere, "read_file", json!({"path": "notes.md"})).secret_read);
    Ok(())
}

#[test]
fn user_only_markers_and_their_folder_are_found_through_file_tools_and_command_paths(
) -> Result<(), Box<dyn Error>> {
    // Given
    let folders = folders()?;
    let environment = environment(&folders, &["exec(touch *)", "write"], false);
    let runtime = folders.data.join("runtime");
    let off = runtime.join("auto-mode-setup-hint-off");
    let notice = runtime.join("auto-mode-default-notice");
    let writes = [
        ("write_file", json!({"path": off.display().to_string()})),
        ("edit_file", json!({"path": notice.display().to_string()})),
        (
            "exec",
            json!({"command": format!("touch {}", off.display())}),
        ),
        (
            "exec",
            json!({"command": format!("rm -f {}", notice.display())}),
        ),
        (
            "exec",
            json!({"command": format!("echo x > {}", off.display())}),
        ),
        (
            "exec",
            json!({"command": format!("mv {} /tmp/old", runtime.display())}),
        ),
        (
            "exec",
            json!({"command": format!("rm -rf {}/../runtime", runtime.display())}),
        ),
        (
            "exec",
            json!({"command": format!("rm {}/auto-mode-*", runtime.display())}),
        ),
        (
            "exec",
            json!({"command": format!("rm -f {}/*", runtime.display())}),
        ),
        (
            "exec",
            json!({"command": format!("rm {}/r?ntime/auto-mode-setup-hint-off", folders.data.display())}),
        ),
        (
            "exec",
            json!({"command": "python3 -c \"import os; os.remove('auto-mode-setup-hint-off')\""}),
        ),
    ];

    // When / Then
    for (tool, arguments) in writes {
        let found = materials(&environment, tool, arguments.clone());
        assert!(found.user_only_write, "{tool} {arguments}");
    }
    let read = materials(
        &environment,
        "read_file",
        json!({"path": off.display().to_string()}),
    );
    let beside = materials(
        &environment,
        "exec",
        json!({"command": format!("touch {}", runtime.join("other").display())}),
    );
    let logs = materials(
        &environment,
        "exec",
        json!({"command": format!("rm {}/*.log", runtime.display())}),
    );
    assert!(!read.user_only_write && !beside.user_only_write && !logs.user_only_write);
    Ok(())
}

#[test]
fn protected_and_linked_writes_and_workspace_edits_are_told_apart() -> Result<(), Box<dyn Error>> {
    // Given
    let folders = folders()?;
    let environment = environment(&folders, &[], false);
    fs::create_dir(folders.workspace.join("out"))?;
    std::os::unix::fs::symlink(
        folders.workspace.join(".git"),
        folders.workspace.join("linked"),
    )?;
    std::os::unix::fs::symlink(&folders.home, folders.workspace.join("away"))?;

    // When
    let edit = materials(&environment, "write_file", json!({"path": "out/report.md"}));
    let git = materials(&environment, "write_file", json!({"path": ".git/config"}));
    let linked = materials(
        &environment,
        "exec",
        json!({"command": "echo x > linked/config"}),
    );
    let away = materials(
        &environment,
        "exec",
        json!({"command": "echo x > away/notes"}),
    );

    // Then
    assert!(edit.workspace_edit && !edit.protected_write);
    assert!(git.protected_write && !git.workspace_edit && git.auto_allow_blocked);
    assert!(linked.symlinked_protected_write.is_some());
    assert!(away.symlinked_outside_write.is_some());
    Ok(())
}

#[test]
fn allow_rules_approve_commands_but_not_their_outside_redirect_targets(
) -> Result<(), Box<dyn Error>> {
    // Given
    let folders = folders()?;
    let environment = environment(&folders, &["exec(sort *)", "exec(python3 *)"], false);

    // When
    let inside = materials(&environment, "exec", json!({"command": "sort a > out.txt"}));
    let outside = materials(
        &environment,
        "exec",
        json!({"command": "sort a > /tmp/out.txt"}),
    );
    let broad = materials(&environment, "exec", json!({"command": "python3 run.py"}));
    let spawn = materials(&environment, "spawn", json!({"task": "x"}));

    // Then
    assert_eq!(inside.allow_rule.as_deref(), Some("exec(sort *)"));
    assert!(!inside.auto_allow_blocked);
    assert!(outside.auto_allow_blocked);
    assert_eq!(broad.allow_rule, None);
    assert!(spawn.auto_allow_blocked);
    Ok(())
}

#[test]
fn critical_removals_and_web_hosts_are_read_from_arguments() -> Result<(), Box<dyn Error>> {
    // Given
    let folders = folders()?;
    let environment = environment(&folders, &["web_fetch(domain:example.com)"], false);

    // When
    let removal = materials(
        &environment,
        "exec",
        json!({"command": "rm --recursive --force ."}),
    );
    let fetch = materials(
        &environment,
        "web_fetch",
        json!({"url": "https://user@Example.com:443/a"}),
    );

    // Then
    assert_eq!(
        removal.critical_removal,
        Some(folders.workspace.display().to_string())
    );
    assert!(fetch.allow_rule.is_some());
    Ok(())
}
