use shacs_config::{self, Config};
use shacs_core::runtime::ContextBuilder;
use std::error::Error;
use std::fs;
use std::process::Command;

#[test]
fn skills_cli_and_context_share_selected_source_and_value_free_readiness(
) -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let workspace = root.path().join("workspace");
    let config_path = root.path().join("config.json");
    let mut config = Config::default();
    config.agents.defaults.workspace = workspace.display().to_string();
    config.providers.insert(
        "openai".to_owned(),
        shacs_config::ProviderConfig {
            api_key: Some("${UNRELATED_PROVIDER_SECRET}".to_owned()),
            ..shacs_config::ProviderConfig::default()
        },
    );
    config
        .env
        .insert("READY_KEY".to_owned(), "${FILE_KEY}".to_owned());
    config
        .env
        .insert("EMPTY_KEY".to_owned(), "top-level-canary".to_owned());
    config
        .tools
        .exec
        .env
        .insert("EMPTY_KEY".to_owned(), String::new());
    shacs_config::save_config_to_path(&config, &config_path)?;
    fs::write(
        root.path().join(".env.json"),
        r#"{"FILE_KEY":"file-secret-canary","UNDECLARED_KEY":"unrelated-secret-canary"}"#,
    )?;
    for (path, fields, body) in [
        (
            root.path().join("skills/user-copy"),
            "requires.env: USER_KEY",
            "Lower body",
        ),
        (
            workspace.join("skills/workspace-copy"),
            "always: true\nrequires.env: [MISSING_KEY, READY_KEY, EMPTY_KEY, HOME]",
            "Selected body",
        ),
    ] {
        fs::create_dir_all(&path)?;
        fs::write(
            path.join("SKILL.md"),
            format!("---\nname: selected\n{fields}\n---\n{body}"),
        )?;
    }
    let broken = workspace.join("skills/broken");
    fs::create_dir_all(&broken)?;
    fs::write(
        broken.join("SKILL.md"),
        "---\nname: broken\nrequires.env: ['invalid-secret-canary']\n---\nReadable invalid body",
    )?;

    let mut outputs = Vec::new();
    for arguments in [
        vec!["skills", "list"],
        vec!["skills", "show", "selected"],
        vec!["skills", "show", "broken"],
    ] {
        let result = Command::new(env!("CARGO_BIN_EXE_shacs-bot"))
            .args(arguments)
            .arg("--config")
            .arg(&config_path)
            .env_clear()
            .env("HOME", root.path())
            .output()?;
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        outputs.push(String::from_utf8(result.stdout)?);
    }
    assert!(
        outputs[0].contains("selected"),
        "missing credentials keep the skill listed"
    );
    assert!(
        outputs[0].contains("missing env: EMPTY_KEY, MISSING_KEY"),
        "list uses resolved explicit env"
    );
    assert!(
        outputs[1].contains("workspace-local"),
        "CLI selects registry winner"
    );
    assert!(
        outputs[1].contains("overridden_empty: EMPTY_KEY"),
        "empty override has its own reason"
    );
    assert!(
        outputs[1].contains("reserved (excluded): HOME"),
        "reserved names are not missing"
    );
    assert!(
        outputs[2].contains("credential declaration invalid"),
        "invalid declarations are diagnosed"
    );
    for output in &outputs {
        assert!(
            !output.contains("canary"),
            "public status must not reveal values or invalid input"
        );
        assert!(
            output.contains(&root.path().join(".env.json").display().to_string()),
            "CLI reports its data path"
        );
    }
    let context = ContextBuilder::new(&workspace)
        .with_skill_config(&config_path)
        .with_configured_env([
            ("READY_KEY".to_owned(), "file-secret-canary".to_owned()),
            ("EMPTY_KEY".to_owned(), String::new()),
        ]);
    let prompt = context.build_system_prompt(None);
    let child_prompt = context.build_subagent_prompt();
    assert!(
        prompt.contains("Selected body"),
        "always body remains available"
    );
    assert!(
        !prompt.contains("Lower body"),
        "context selects the same source as CLI"
    );
    assert!(
        context
            .load_skill("broken")
            .is_some_and(|raw| raw.contains("Readable invalid body")),
        "invalid declaration keeps body readable"
    );
    for prompt in [&prompt, &child_prompt] {
        assert!(
            prompt.contains("missing env: EMPTY_KEY, MISSING_KEY"),
            "both contexts report missing names"
        );
        assert!(
            !prompt.contains("canary"),
            "context guidance never includes values"
        );
        assert!(
            !prompt.contains("USER_KEY"),
            "shadowed declarations are excluded"
        );
    }

    fs::write(
        workspace.join("skills/workspace-copy/SKILL.md"),
        "---\nname: selected\ndisabled: true\nrequires.env: MISSING_KEY\n---\nDisabled body",
    )?;
    for (arguments, visible) in [
        (vec!["skills", "list"], false),
        (vec!["skills", "list", "--all"], true),
        (vec!["skills", "show", "selected"], true),
    ] {
        let result = Command::new(env!("CARGO_BIN_EXE_shacs-bot"))
            .args(arguments)
            .arg("--config")
            .arg(&config_path)
            .env_clear()
            .env("HOME", root.path())
            .output()?;
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        let text = String::from_utf8(result.stdout)?;
        assert_eq!(text.contains("inactive: frontmatter disabled"), visible);
        if !visible {
            assert!(
                !text.contains("- selected"),
                "disabled winner does not reveal the lower source"
            );
        }
    }
    assert!(
        context.load_skill("selected").is_none(),
        "context also excludes the disabled winner"
    );

    fs::write(
        root.path().join(".env.json"),
        r#"{"FILE_KEY": "file-secret-canary", "BAD":"x", "BAD":"invalid-secret-canary"}"#,
    )?;
    let error = Command::new(env!("CARGO_BIN_EXE_shacs-bot"))
        .args(["skills", "list", "--config"])
        .arg(&config_path)
        .env_clear()
        .env("HOME", root.path())
        .output()?;
    assert!(
        !error.status.success(),
        "malformed secret files are not treated as missing keys"
    );
    assert!(
        !String::from_utf8_lossy(&error.stderr).contains("canary"),
        "file errors never reveal values"
    );
    Ok(())
}
