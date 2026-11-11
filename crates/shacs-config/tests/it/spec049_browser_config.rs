use serde_json::json;
use shacs_config::{
    BrowserConfig, BrowserViewConfig, Config, LoadOptions, PermissionMode, SafetyCapability,
    UserLocalBrowserEnable,
};
use std::collections::BTreeMap;
use std::error::Error;
use std::fs;

#[test]
fn browser_defaults_and_limits() -> Result<(), Box<dyn Error>> {
    assert_eq!(
        SafetyCapability::parse("browser_act"),
        Some(SafetyCapability::BrowserAct)
    );
    assert_eq!(
        serde_json::to_value(SafetyCapability::BrowserAct)?,
        json!("browser_act")
    );
    assert_eq!(SafetyCapability::all().len(), 11);
    assert!(SafetyCapability::all().contains(&SafetyCapability::BrowserAct));
    assert!(PermissionMode::BypassPermissions
        .baseline_candidate_capabilities()
        .contains(&SafetyCapability::BrowserAct));
    let stored: Vec<SafetyCapability> = serde_json::from_value(json!(["net_outbound"]))?;
    assert!(!stored.contains(&SafetyCapability::BrowserAct));
    let expanded: Vec<SafetyCapability> =
        serde_json::from_value(json!(["net_outbound", "browser_act"]))?;
    assert!(expanded.contains(&SafetyCapability::BrowserAct));
    assert_eq!(serde_json::to_value(stored)?, json!(["net_outbound"]));
    let default: Config = serde_json::from_value(json!({}))?;
    assert_eq!(default.tools.browser, BrowserConfig::default());
    let serialized = serde_json::to_value(&default.tools.browser)?;
    assert!(serialized.get("screenshots").is_none());
    for screenshots in [false, true] {
        let config: BrowserConfig = serde_json::from_value(json!({"screenshots":screenshots}))?;
        assert_eq!(config.screenshots, Some(screenshots));
        assert_eq!(serde_json::to_value(config)?["screenshots"], screenshots);
    }
    assert_eq!(
        serialized,
        json!({"enable":false,"startupTimeoutSeconds":20,"idleTimeoutSeconds":300,"pageTimeoutSeconds":30,"view":{"enable":false,"password":""}})
    );
    for (key, low, high) in [
        ("startupTimeoutSeconds", 5, 60),
        ("idleTimeoutSeconds", 60, 1800),
        ("pageTimeoutSeconds", 5, 120),
    ] {
        for value in [low, high] {
            assert!(serde_json::from_value::<BrowserConfig>(json!({key:value})).is_ok());
        }
        for value in [low - 1, high + 1] {
            assert!(serde_json::from_value::<BrowserConfig>(json!({key:value})).is_err());
        }
    }
    let directory = tempfile::tempdir()?;
    let workspace = directory.path().join("workspace");
    fs::create_dir(&workspace)?;
    let config = directory.path().join("config.json");
    fs::write(
        &config,
        r#"{"tools":{"browser":{"enable":true}},"permissions":{"deny":["*"],"ask":["browser_startup"]}}"#,
    )?;
    let evidence = UserLocalBrowserEnable::read(&config, &workspace)?.ok_or("missing evidence")?;
    assert!(evidence.is_current());
    assert_eq!(evidence.permission_rules().deny, ["*"]);
    assert_eq!(evidence.permission_rules().ask, ["browser_startup"]);
    fs::write(&config, r#"{"tools":{"browser":{"enable":false}}}"#)?;
    assert!(!evidence.is_current());
    assert!(UserLocalBrowserEnable::read(&config, &workspace)?.is_none());
    fs::write(
        &config,
        r#"{"tools":{"browser":{"enable":true}},"permissions":{"deny":["${SHACS_SPEC049_BROWSER_START_RULE}"]}}"#,
    )?;
    let environment_file = directory.path().join(".env.json");
    fs::write(
        &environment_file,
        r#"{"SHACS_SPEC049_BROWSER_START_RULE":"*"}"#,
    )?;
    let resolved =
        UserLocalBrowserEnable::read(&config, &workspace)?.ok_or("missing resolved evidence")?;
    assert_eq!(resolved.permission_rules().deny, ["*"]);
    assert!(resolved.is_current());
    fs::write(
        &environment_file,
        r#"{"SHACS_SPEC049_BROWSER_START_RULE":"web_fetch"}"#,
    )?;
    assert!(
        !resolved.is_current(),
        "changing a rule reference must invalidate startup evidence"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs as unix_fs;
        let selected = directory.path().join("selected");
        fs::create_dir(&selected)?;
        let selected_config = selected.join("config.json");
        unix_fs::symlink(&config, &selected_config)?;
        fs::write(
            selected.join(".env.json"),
            r#"{"SHACS_SPEC049_BROWSER_START_RULE":"*"}"#,
        )?;
        let selected_evidence = UserLocalBrowserEnable::read(&selected_config, &workspace)?
            .ok_or("selected config evidence missing")?;
        assert_eq!(
            selected_evidence.permission_rules().deny,
            ["*"],
            "env refs use the selected config folder, like the normal loader"
        );
    }
    let workspace_config = workspace.join("config.json");
    fs::write(
        &workspace_config,
        r#"{"tools":{"browser":{"enable":true}}}"#,
    )?;
    assert!(UserLocalBrowserEnable::read(&workspace_config, &workspace)?.is_none());
    Ok(())
}

#[test]
fn view_defaults_and_password_secret_handling() -> Result<(), Box<dyn Error>> {
    const SECRET: &str = "view-secret-marker-1234";
    let view = BrowserViewConfig::default();
    assert!(!view.enable && view.password.is_empty());
    let partial: BrowserConfig = serde_json::from_value(json!({"view":{"enable":true}}))?;
    assert!(partial.view.enable && partial.view.password.is_empty());
    let literal: BrowserConfig =
        serde_json::from_value(json!({"view":{"enable":true,"password":SECRET}}))?;
    assert_eq!(literal.view.password, SECRET);
    // The stored form keeps the value as written; only Debug hides it.
    assert_eq!(
        serde_json::to_value(&literal.view)?,
        json!({"enable":true,"password":SECRET})
    );
    let debug = format!("{literal:?}");
    assert!(
        debug.contains("[REDACTED]") && !debug.contains(SECRET),
        "{debug}"
    );
    assert!(!format!("{:?}", BrowserViewConfig::default()).contains("[REDACTED]"));

    let directory = tempfile::tempdir()?;
    let workspace = directory.path().join("workspace");
    fs::create_dir(&workspace)?;
    let config_path = directory.path().join("config.json");
    // A literal password is a secret place for `runtime config-migrate`, named
    // like the other password keys; a `${NAME}` reference stays as it is.
    fs::write(
        &config_path,
        serde_json::to_vec(&json!({
            "agents":{"defaults":{"workspace":workspace}},
            "tools":{"browser":{"view":{"enable":true,"password":SECRET}}}
        }))?,
    )?;
    let plan = shacs_config::dry_run_secrets_migration(&config_path, &BTreeMap::new())?;
    assert_eq!(
        plan.moved
            .iter()
            .map(|moved| (moved.field.as_str(), moved.name.as_str()))
            .collect::<Vec<_>>(),
        [(
            "tools.browser.view.password",
            "SHACS_TOOLS_BROWSER_VIEW_PASSWORD"
        )]
    );
    assert_eq!(
        shacs_config::secret_env_name(&["tools", "browser", "view", "password"]),
        "SHACS_TOOLS_BROWSER_VIEW_PASSWORD"
    );
    fs::write(
        &config_path,
        serde_json::to_vec(&json!({
            "agents":{"defaults":{"workspace":workspace}},
            "tools":{"browser":{"view":{"enable":true,"password":"${SHACS_BROWSER_VIEW_PASSWORD}"}}}
        }))?,
    )?;
    let plan = shacs_config::dry_run_secrets_migration(&config_path, &BTreeMap::new())?;
    assert!(
        plan.moved.is_empty() && plan.kept.is_empty() && plan.conflicts.is_empty(),
        "a reference is already in its final place: {plan:?}"
    );

    // The recommended reference with no value anywhere (no `.env.json`, no
    // process variable, or an empty process variable) loads with an empty
    // password, viewer enabled or not. Every other field keeps the strict
    // missing-variable error.
    for enable in [true, false] {
        fs::write(
            &config_path,
            serde_json::to_vec(&json!({
                "agents":{"defaults":{"workspace":workspace}},
                "tools":{"browser":{"view":{"enable":enable,"password":"${SHACS_BROWSER_VIEW_PASSWORD}"}}}
            }))?,
        )?;
        for env in [
            BTreeMap::new(),
            BTreeMap::from([("SHACS_BROWSER_VIEW_PASSWORD".to_owned(), String::new())]),
        ] {
            let loaded = shacs_config::load_config_with_env(
                LoadOptions {
                    config_path: Some(config_path.clone()),
                    ..LoadOptions::default()
                },
                &env,
            )?;
            assert_eq!(loaded.config.tools.browser.view.enable, enable);
            assert_eq!(loaded.config.tools.browser.view.password, "");
        }
    }
    fs::write(
        &config_path,
        serde_json::to_vec(&json!({
            "agents":{"defaults":{"workspace":workspace}},
            "providers":{"openai":{"apiKey":"${SHACS_SPEC049_ABSENT_PROVIDER_KEY}"}},
            "tools":{"browser":{"view":{"enable":true,"password":"${SHACS_BROWSER_VIEW_PASSWORD}"}}}
        }))?,
    )?;
    let strict = shacs_config::load_config_with_env(
        LoadOptions {
            config_path: Some(config_path.clone()),
            ..LoadOptions::default()
        },
        &BTreeMap::new(),
    );
    assert!(
        strict.err().is_some_and(|error| error
            .to_string()
            .contains("SHACS_SPEC049_ABSENT_PROVIDER_KEY")),
        "other fields keep the strict missing-variable error"
    );
    fs::write(
        &config_path,
        serde_json::to_vec(&json!({
            "agents":{"defaults":{"workspace":workspace}},
            "tools":{"browser":{"view":{"enable":true,"password":"${SHACS_BROWSER_VIEW_PASSWORD}"}}}
        }))?,
    )?;

    // `${NAME}` resolves from a non-empty process variable first, then from
    // `.env.json`; the resolved value stays in the loaded config only.
    fs::write(
        directory.path().join(".env.json"),
        format!(r#"{{"SHACS_BROWSER_VIEW_PASSWORD":"{SECRET}"}}"#),
    )?;
    let from_file = shacs_config::load_config_with_env(
        LoadOptions {
            config_path: Some(config_path.clone()),
            ..LoadOptions::default()
        },
        &BTreeMap::new(),
    )?;
    assert_eq!(from_file.config.tools.browser.view.password, SECRET);
    let process = BTreeMap::from([(
        "SHACS_BROWSER_VIEW_PASSWORD".to_owned(),
        "process-value-marker".to_owned(),
    )]);
    let from_process = shacs_config::load_config_with_env(
        LoadOptions {
            config_path: Some(config_path.clone()),
            ..LoadOptions::default()
        },
        &process,
    )?;
    assert_eq!(
        from_process.config.tools.browser.view.password,
        "process-value-marker"
    );
    assert!(std::env::var_os("SHACS_BROWSER_VIEW_PASSWORD").is_none());
    let unresolved = shacs_config::load_config_with_env(
        LoadOptions {
            config_path: Some(config_path),
            resolve_env: false,
            ..LoadOptions::default()
        },
        &BTreeMap::new(),
    )?;
    assert_eq!(
        unresolved.config.tools.browser.view.password,
        "${SHACS_BROWSER_VIEW_PASSWORD}"
    );
    Ok(())
}
