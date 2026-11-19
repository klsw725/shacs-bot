use crate::spec054_exec_environment_support::{self as support, Provider, TestResult};
use serde_json::json;
use shacs_cli::{load_runtime_config_with_env, RuntimeConfigOptions};
use shacs_config::Config;
use std::collections::BTreeMap;
use std::fs;
use std::sync::{mpsc, Arc};
use std::time::Duration;

#[test]
fn production_main_and_spawn_use_startup_values_without_reading_skills() -> TestResult {
    let root = tempfile::tempdir()?;
    let workspace = root.path().join("workspace");
    fs::create_dir_all(workspace.join("skills/fixture"))?;
    fs::write(
        workspace.join("skills/fixture/SKILL.md"),
        "---\nname: fixture\nrequires.env: NOTION_API_KEY, WORKSPACE_KEY\n---\nFixture",
    )?;
    let config: Config = serde_json::from_value(json!({
        "agents":{"defaults":{"provider":"openai","model":"gpt-4o","workspace":workspace,"unifiedSession":false}},
        "permissions":{"mode":"auto","allow":["exec","spawn"]},
        "tools":{"exec":{"enable":true},"toolSearch":{"enabled":"off"}}
    }))?;
    fs::write(
        root.path().join("config.json"),
        serde_json::to_vec(&config)?,
    )?;
    fs::write(
        root.path().join(".env.json"),
        r#"{"NOTION_API_KEY":"startup-notion-7713","UNDECLARED_CANARY":"excluded-canary-7713","WORKSPACE_KEY":"workspace-canary","OTHER_KEY":"other-canary"}"#,
    )?;
    for (script, condition) in [
        (
            "check-startup.sh",
            "test \"$NOTION_API_KEY\" = startup-notion-7713",
        ),
        (
            "check-rotated.sh",
            "test \"$NOTION_API_KEY\" = rotated-notion-7713",
        ),
        ("check-missing.sh", "test -z \"${NOTION_API_KEY+x}\""),
    ] {
        fs::write(workspace.join(script), format!("{condition} && test -z \"${{UNDECLARED_CANARY+x}}\" && test -z \"${{OTHER_KEY+x}}\" && printf credential-ok\n"))?;
    }
    let (sender, results) = mpsc::channel();
    let provider = Arc::new(Provider { results: sender });
    let bundle = support::load(root.path(), &BTreeMap::new())?;
    assert!(bundle.config.env.is_empty() && bundle.config.tools.exec.env.is_empty());
    let runtime = support::adapter(bundle, &provider)?;
    support::run(&runtime, "check-startup", "main-first")?;
    support::assert_success(&results)?;

    let other_workspace = root.path().join("other");
    fs::create_dir_all(other_workspace.join("skills/other"))?;
    fs::write(
        other_workspace.join("skills/other/SKILL.md"),
        "---\nname: other\nrequires.env: OTHER_KEY\n---\nOther fixture",
    )?;
    fs::write(other_workspace.join("check-startup.sh"), "test \"$OTHER_KEY\" = other-canary && test -z \"${WORKSPACE_KEY+x}\" && printf credential-ok\n")?;
    let other_bundle = load_runtime_config_with_env(
        RuntimeConfigOptions {
            config_path: Some(root.path().join("config.json")),
            workspace_override: Some(other_workspace),
            resolve_env: true,
        },
        &BTreeMap::new(),
    )?;
    let other = support::adapter(other_bundle, &provider)?;
    support::run(&other, "check-startup", "other-workspace")?;
    support::assert_success(&results)?;

    fs::write(
        root.path().join(".env.json"),
        r#"{"NOTION_API_KEY":"rotated-notion-7713"}"#,
    )?;
    support::run(&runtime, "spawn-parent", "spawn-main")?;
    let outcomes = [
        results.recv_timeout(Duration::from_secs(10))?,
        results.recv_timeout(Duration::from_secs(10))?,
    ];
    assert!(
        outcomes.iter().any(|text| text.contains("credential-ok")),
        "{outcomes:?}"
    );
    support::run(&runtime, "check-startup", "main-after-rotation")?;
    support::assert_success(&results)?;
    let restarted = support::adapter(support::load(root.path(), &BTreeMap::new())?, &provider)?;
    support::run(&restarted, "check-rotated", "main-restarted")?;
    support::assert_success(&results)?;
    fs::remove_file(root.path().join(".env.json"))?;
    support::run(&runtime, "check-startup", "main-after-deletion")?;
    support::assert_success(&results)?;
    let missing = support::adapter(support::load(root.path(), &BTreeMap::new())?, &provider)?;
    support::run(&missing, "check-missing", "main-missing")?;
    support::assert_success(&results)?;

    let mut sessions = shacs_session::SessionManager::new(&workspace)?;
    for entry in sessions.list_sessions()? {
        let session = sessions.get_or_create(&entry.key);
        let metadata = serde_json::to_string(&session.metadata)?;
        for value in [
            "startup-notion-7713",
            "rotated-notion-7713",
            "excluded-canary-7713",
        ] {
            assert!(
                !metadata.contains(value),
                "values must not enter persisted runtime metadata"
            );
        }
    }
    Ok(())
}

#[test]
fn production_exec_uses_process_fallback_and_explicit_winners() -> TestResult {
    let root = tempfile::tempdir()?;
    let workspace = root.path().join("workspace");
    fs::create_dir_all(workspace.join("skills/fixture"))?;
    fs::write(workspace.join("skills/fixture/SKILL.md"), "---\nname: fixture\nrequires.env: PROCESS_KEY, EMPTY_KEY, OVERRIDE_KEY, CLEARED_KEY, HOME, PATH, LANG, TERM\n---\nFixture")?;
    let config: Config = serde_json::from_value(json!({
        "agents":{"defaults":{"provider":"openai","model":"gpt-4o","workspace":workspace,"unifiedSession":false}},
        "env":{"OVERRIDE_KEY":"top-level-canary"},
        "permissions":{"mode":"auto","allow":["exec"]},
        "tools":{"exec":{"enable":true,"env":{"OVERRIDE_KEY":"explicit-canary","CLEARED_KEY":""}},"toolSearch":{"enabled":"off"}}
    }))?;
    fs::write(
        root.path().join("config.json"),
        serde_json::to_vec(&config)?,
    )?;
    fs::write(
        root.path().join(".env.json"),
        r#"{"PROCESS_KEY":"file-loses","EMPTY_KEY":"fallback-canary","OVERRIDE_KEY":"file-loses","CLEARED_KEY":"file-loses","HOME":"reserved-canary","PATH":"reserved-canary","LANG":"reserved-canary","TERM":"reserved-canary"}"#,
    )?;
    fs::write(workspace.join("check-startup.sh"), "test \"$PROCESS_KEY\" = process-canary && test \"$EMPTY_KEY\" = fallback-canary && test \"$OVERRIDE_KEY\" = explicit-canary && test -z \"$CLEARED_KEY\" && test \"$HOME\" != reserved-canary && test \"$PATH\" != reserved-canary && test \"$LANG\" != reserved-canary && test \"$TERM\" != reserved-canary && printf credential-ok\n")?;
    let (sender, results) = mpsc::channel();
    let provider = Arc::new(Provider { results: sender });
    let process = BTreeMap::from([
        ("PROCESS_KEY".to_owned(), "process-canary".to_owned()),
        ("EMPTY_KEY".to_owned(), String::new()),
    ]);
    let runtime = support::adapter(support::load(root.path(), &process)?, &provider)?;
    support::run(&runtime, "check-startup", "precedence")?;
    support::assert_success(&results)?;
    Ok(())
}
