use super::*;
use serde_json::json;
use std::collections::BTreeMap;

fn no_env() -> BTreeMap<String, String> {
    BTreeMap::new()
}

fn names(plan: &SecretsPlan) -> Vec<(String, String)> {
    plan.moved
        .iter()
        .map(|moved| (moved.field.clone(), moved.name.clone()))
        .collect()
}

#[test]
fn secret_fields_and_env_blocks_move_under_path_names() {
    // Given
    let config = json!({
        "env": {"GITHUB_TOKEN": "ghp-value", "http_proxy": "http://proxy:3128"},
        "providers": {
            "openrouter": {"apiKey": "or-key"},
            "custom": {
                "api_key": "custom-key",
                "extraHeaders": {"Authorization": "Bearer custom", "X-Trace": "trace-id"}
            }
        },
        "tools": {
            "exec": {"env": {"PATH": "/opt/bin"}},
            "web": {"search": {"apiKey": "search-key", "maxResults": 5}},
            "mcpServers": {
                "my-server": {
                    "env": {"API_KEY": "mcp-key"},
                    "headers": {"X-Api-Key": "header-key"}
                }
            }
        },
        "channels": {
            "discord": {"token": "discord-token"},
            "slack": {"appToken": "xapp", "bot_token": "xoxb"},
            "whatsapp": {"bridgeToken": "bridge"},
            "email": {"smtp": {"password": "smtp-pass"}, "imapPassword": "imap-pass"}
        },
        "agents": {"defaults": {"maxTokens": 8192, "model": "m"}}
    });

    // When
    let plan = plan(&config, &EnvFile::default(), &no_env());

    // Then
    let expected = [
        ("channels.discord.token", "SHACS_CHANNELS_DISCORD_TOKEN"),
        (
            "channels.email.imapPassword",
            "SHACS_CHANNELS_EMAIL_IMAP_PASSWORD",
        ),
        (
            "channels.email.smtp.password",
            "SHACS_CHANNELS_EMAIL_SMTP_PASSWORD",
        ),
        ("channels.slack.appToken", "SHACS_CHANNELS_SLACK_APP_TOKEN"),
        ("channels.slack.bot_token", "SHACS_CHANNELS_SLACK_BOT_TOKEN"),
        (
            "channels.whatsapp.bridgeToken",
            "SHACS_CHANNELS_WHATSAPP_BRIDGE_TOKEN",
        ),
        ("env.GITHUB_TOKEN", "SHACS_ENV_GITHUB_TOKEN"),
        ("env.http_proxy", "SHACS_ENV_HTTP_PROXY"),
        ("providers.custom.api_key", "SHACS_PROVIDERS_CUSTOM_API_KEY"),
        (
            "providers.custom.extraHeaders.Authorization",
            "SHACS_PROVIDERS_CUSTOM_EXTRA_HEADERS_AUTHORIZATION",
        ),
        (
            "providers.openrouter.apiKey",
            "SHACS_PROVIDERS_OPENROUTER_API_KEY",
        ),
        ("tools.exec.env.PATH", "SHACS_TOOLS_EXEC_ENV_PATH"),
        (
            "tools.mcpServers.my-server.env.API_KEY",
            "SHACS_TOOLS_MCP_SERVERS_MY_SERVER_ENV_API_KEY",
        ),
        (
            "tools.mcpServers.my-server.headers.X-Api-Key",
            "SHACS_TOOLS_MCP_SERVERS_MY_SERVER_HEADERS_X_API_KEY",
        ),
        ("tools.web.search.apiKey", "SHACS_TOOLS_WEB_SEARCH_API_KEY"),
    ];
    let mut found = names(&plan);
    found.sort();
    assert_eq!(
        found,
        expected
            .iter()
            .map(|(field, name)| ((*field).to_owned(), (*name).to_owned()))
            .collect::<Vec<_>>()
    );
    assert!(plan.conflicts.is_empty());
    assert_eq!(
        plan.config["channels"]["discord"]["token"],
        "${SHACS_CHANNELS_DISCORD_TOKEN}"
    );
    assert_eq!(
        plan.config["tools"]["exec"]["env"]["PATH"],
        "${SHACS_TOOLS_EXEC_ENV_PATH}"
    );
    assert_eq!(
        plan.config["providers"]["custom"]["extraHeaders"]["X-Trace"],
        "trace-id"
    );
    assert_eq!(plan.config["agents"]["defaults"]["maxTokens"], 8192);
    assert_eq!(
        plan.env_file.get("SHACS_CHANNELS_DISCORD_TOKEN"),
        Some("discord-token")
    );
}

#[test]
fn empty_placeholder_secret_ref_and_non_text_values_are_left_alone() {
    // Given
    let config = json!({
        "providers": {
            "a": {"apiKey": ""},
            "b": {"apiKey": "${OPENROUTER_API_KEY}"},
            "c": {"apiKey": "Bearer ${TOKEN}"},
            "d": {"apiKeyRef": {
                "kind": "secret_ref",
                "staleness_token": "sha256:owner-state",
                "locator": {"kind": "env_var", "name": "X"}
            }},
            "e": {"apiKey": 42}
        },
        "channels": {"slack": {"tokens": ["first", "${SECOND}"]}}
    });

    // When
    let plan = plan(&config, &EnvFile::default(), &no_env());

    // Then
    assert_eq!(
        names(&plan),
        vec![(
            "channels.slack.tokens[0]".to_owned(),
            "SHACS_CHANNELS_SLACK_TOKENS_0".to_owned()
        )]
    );
    assert_eq!(
        plan.kept,
        vec![KeptSecret {
            field: "providers.c.apiKey".to_owned(),
            reason: KeptReason::Placeholder,
        }]
    );
    assert_eq!(
        plan.config["providers"]["d"]["apiKeyRef"]["staleness_token"],
        "sha256:owner-state"
    );
}

#[test]
fn fields_sharing_a_name_and_value_share_one_entry() {
    // Given
    let config = json!({"providers": {"openrouter": {"apiKey": "same", "api_key": "same"}}});

    // When
    let plan = plan(&config, &EnvFile::default(), &no_env());

    // Then
    assert!(plan.conflicts.is_empty());
    assert_eq!(plan.moved.len(), 2);
    assert_eq!(
        plan.env_file.entries().collect::<Vec<_>>(),
        vec![("SHACS_PROVIDERS_OPENROUTER_API_KEY", "same")]
    );
}

#[test]
fn each_kind_of_conflict_names_fields_without_values() {
    // Given
    let same_name =
        json!({"channels": {"discordToken": "canary-one", "discord": {"token": "canary-two"}}});
    let env_file_differs = json!({"channels": {"telegram": {"token": "canary-new"}}});
    let process_differs = json!({"channels": {"slack": {"appToken": "canary-configured"}}});
    let env_file =
        EnvFile::parse(br#"{"SHACS_CHANNELS_TELEGRAM_TOKEN": "canary-old"}"#).expect("valid file");
    let process = BTreeMap::from([
        (
            "SHACS_CHANNELS_SLACK_APP_TOKEN".to_owned(),
            "canary-exported".to_owned(),
        ),
        ("SHACS_CHANNELS_TELEGRAM_TOKEN".to_owned(), String::new()),
    ]);

    // When
    let same_name = plan(&same_name, &EnvFile::default(), &no_env());
    let env_file_differs = plan(&env_file_differs, &env_file, &process);
    let process_differs = plan(&process_differs, &EnvFile::default(), &process);

    // Then
    assert_eq!(same_name.conflicts.len(), 2);
    assert!(same_name.conflicts.iter().all(|conflict| conflict.reason
        == ConflictReason::SameNameInConfig
        && conflict.name == "SHACS_CHANNELS_DISCORD_TOKEN"));
    assert_eq!(
        env_file_differs.conflicts,
        vec![SecretConflict {
            field: "channels.telegram.token".to_owned(),
            name: "SHACS_CHANNELS_TELEGRAM_TOKEN".to_owned(),
            reason: ConflictReason::EnvFileDiffers,
        }]
    );
    assert_eq!(
        process_differs.conflicts[0].reason,
        ConflictReason::ProcessEnvDiffers
    );
    let shown = [&same_name, &env_file_differs, &process_differs]
        .iter()
        .flat_map(|plan| plan.conflicts.iter().map(ToString::to_string))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(!shown.contains("canary"), "{shown}");
}

#[test]
fn an_equal_value_in_the_file_or_an_empty_process_value_is_no_conflict() {
    // Given
    let config = json!({"channels": {"telegram": {"token": "same"}}});
    let env_file =
        EnvFile::parse(br#"{"SHACS_CHANNELS_TELEGRAM_TOKEN": "same"}"#).expect("valid file");
    let process = BTreeMap::from([("SHACS_CHANNELS_TELEGRAM_TOKEN".to_owned(), String::new())]);

    // When
    let plan = plan(&config, &env_file, &process);

    // Then
    assert!(plan.conflicts.is_empty());
    assert_eq!(plan.moved.len(), 1);
}

#[test]
fn names_split_camel_case_and_collapse_other_characters() {
    let cases = [
        (
            vec!["tools", "mcpServers", "a.b", "env", "K"],
            "SHACS_TOOLS_MCP_SERVERS_A_B_ENV_K",
        ),
        (
            vec!["channels", "HTTPHeader", "token"],
            "SHACS_CHANNELS_HTTPHEADER_TOKEN",
        ),
        (vec!["channels", "v2Token"], "SHACS_CHANNELS_V2_TOKEN"),
        (
            vec!["channels", "디스코드", "token"],
            "SHACS_CHANNELS_TOKEN",
        ),
        (vec!["env", "TRAILING__"], "SHACS_ENV_TRAILING"),
    ];
    for (keys, expected) in cases {
        let path = keys
            .iter()
            .map(|key| Segment::Key((*key).to_owned()))
            .collect::<Vec<_>>();
        let name = env_name(&path);
        assert_eq!(name, expected);
        assert!(crate::is_env_name(&name), "{name}");
    }
}
