use super::*;
use crate::consent_prompt::ScriptedPrompt;
use crate::review_provider::ScriptedClient;
use std::error::Error;
use std::os::unix::fs::symlink;

const VALUE: &str = "spec048-setup-value-58";
const DRAFT: &str = r#"["[My Repo] github.com/me/repo, the user's own repository"]"#;
const MATERIAL_NAMES: [&str; 10] = [
    "AGENTS.md",
    "SOUL.md",
    "USER.md",
    "TOOLS.md",
    "README.md",
    "package.json",
    "pyproject.toml",
    "Cargo.toml",
    "compose.yaml",
    "docker-compose.yml",
];

struct Places {
    _root: tempfile::TempDir,
    data: PathBuf,
    workspace: PathBuf,
    outside: PathBuf,
}

impl Places {
    fn new(permissions: Value) -> Result<Self, Box<dyn Error>> {
        let root = tempfile::tempdir()?;
        let base = fs::canonicalize(root.path())?;
        let (data, workspace, outside) = (base.join("data"), base.join("ws"), base.join("outside"));
        for folder in [&data, &workspace, &outside] {
            fs::create_dir_all(folder)?;
        }
        fs::write(
            data.join("config.json"),
            serde_json::to_vec_pretty(&json!({
                "agents": {"defaults": {"workspace": workspace}},
                "providers": {"custom": {"apiKey": "${CUSTOM_KEY}"}},
                "permissions": permissions,
            }))?,
        )?;
        fs::write(
            data.join(".env.json"),
            format!("{{\"CUSTOM_KEY\": \"{VALUE}\"}}"),
        )?;
        Ok(Self {
            _root: root,
            data,
            workspace,
            outside,
        })
    }

    fn options(&self, yes: bool) -> AutoModeWriteOptions {
        AutoModeWriteOptions {
            config_path: Some(self.data.join("config.json")),
            workspace_override: None,
            yes,
        }
    }

    fn config(&self) -> Result<Value, Box<dyn Error>> {
        Ok(serde_json::from_slice(&fs::read(
            self.data.join("config.json"),
        )?)?)
    }
}

/// Runs setup on the config as it is now, the way `setup` reads it first.
fn run(
    options: &AutoModeWriteOptions,
    client: &std::sync::Arc<ScriptedClient>,
    prompt: &mut ScriptedPrompt,
) -> Result<String, CliError> {
    let path = options
        .config_path
        .clone()
        .unwrap_or_else(default_config_path);
    setup_with(options, ReadConfig::read(path)?, &client.provider(), prompt)
}

fn fifo(path: &Path) -> Result<(), Box<dyn Error>> {
    let made = std::process::Command::new("mkfifo").arg(path).status()?;
    if !made.success() {
        return Err(format!("mkfifo failed: {made}").into());
    }
    Ok(())
}

fn session_lines() -> String {
    [
        json!({"_type": "metadata", "key": "cli:a"}),
        json!({"role": "user", "content": "user wrote spec048-user-words"}),
        json!({"role": "assistant", "content": "assistant wrote spec048-assistant-words", "tool_calls": [
            {"id": "c1", "type": "function", "function": {"name": "exec", "arguments": "{\"command\": \"TOKEN=abc /usr/bin/python3 x.py\"}"}},
            {"id": "c2", "type": "function", "function": {"name": "web_fetch", "arguments": {"url": "https://user:pw@Docs.Example.com:443/page?q=1"}}},
        ]}),
        json!({"role": "tool", "tool_call_id": "c1", "content": "result spec048-tool-words"}),
    ]
    .iter()
    .map(Value::to_string)
    .collect::<Vec<_>>()
    .join("\n")
}

#[test]
fn the_request_holds_workspace_material_and_leaves_out_what_it_must() -> Result<(), Box<dyn Error>>
{
    // Given
    let places = Places::new(json!({"deny": ["read(AGENTS.md)"]}))?;
    let workspace = &places.workspace;
    fs::write(workspace.join("AGENTS.md"), "denied spec048-denied-words")?;
    fs::write(workspace.join("SOUL.md"), format!("soul text with {VALUE}"))?;
    fs::write(
        places.outside.join("README.md"),
        "outside spec048-outside-words",
    )?;
    symlink(
        places.outside.join("README.md"),
        workspace.join("README.md"),
    )?;
    fifo(&workspace.join("TOOLS.md"))?;
    fs::write(workspace.join("package.json"), r#"{"name": "my-app"}"#)?;
    fs::create_dir_all(workspace.join(".git"))?;
    fs::write(
        workspace.join(".git/config"),
        "[core]\n\turl = https://not-a-remote.example\n[remote \"origin\"]\n\turl = https://me:tok3n@github.com/me/repo.git?x=1#top\n",
    )?;
    fs::create_dir_all(workspace.join("sessions"))?;
    fs::write(workspace.join("sessions/cli_a.jsonl"), session_lines())?;
    fifo(&workspace.join("sessions/pipe.jsonl"))?;
    let client = ScriptedClient::new(Ok(DRAFT));
    let mut prompt = ScriptedPrompt::answering(None);

    // When
    let reply = run(&places.options(true), &client, &mut prompt)?;

    // Then
    let sent = client.user_texts().join("\n");
    let payload: Value = serde_json::from_str(&sent)?;
    let names = payload["workspace"].as_array().map(|items| {
        items
            .iter()
            .filter_map(|item| item["name"].as_str())
            .collect::<Vec<_>>()
    });
    assert_eq!(names, Some(vec!["SOUL.md", "package.json"]));
    assert_eq!(
        payload["remotes"],
        json!(["https://github.com/me/repo.git"])
    );
    assert_eq!(payload["sessions"]["tools"], json!(["exec", "web_fetch"]));
    assert_eq!(payload["sessions"]["hosts"], json!(["docs.example.com"]));
    assert_eq!(payload["sessions"]["commands"], json!(["python3"]));
    for absent in [
        VALUE,
        "spec048-denied-words",
        "spec048-outside-words",
        "spec048-user-words",
        "spec048-assistant-words",
        "spec048-tool-words",
        "tok3n",
        "not-a-remote",
    ] {
        assert!(!sent.contains(absent), "{absent}: {sent}");
    }
    let shown = prompt.shown.join("\n");
    for skipped in [
        "AGENTS.md: permission rule",
        "README.md: symlink",
        "TOOLS.md: not a regular file",
    ] {
        assert!(shown.contains(skipped), "{skipped}: {shown}");
    }
    assert!(reply.starts_with("Added 1 environment entries"), "{reply}");
    assert_eq!(
        places.config()?["permissions"]["autoMode"]["environment"],
        json!([
            "$defaults",
            "[My Repo] github.com/me/repo, the user's own repository"
        ])
    );
    Ok(())
}

#[test]
fn a_git_file_and_a_symlinked_sessions_folder_are_not_read() -> Result<(), Box<dyn Error>> {
    // Given
    let places = Places::new(json!({}))?;
    fs::write(places.workspace.join(".git"), "gitdir: ../elsewhere/.git")?;
    fs::write(places.outside.join("cli_b.jsonl"), session_lines())?;
    symlink(&places.outside, places.workspace.join("sessions"))?;
    let client = ScriptedClient::new(Ok("[]"));
    let mut prompt = ScriptedPrompt::answering(None);

    // When
    let reply = run(&places.options(true), &client, &mut prompt)?;

    // Then
    let payload: Value = serde_json::from_str(&client.user_texts().join("\n"))?;
    assert_eq!(payload["remotes"], json!([]));
    assert_eq!(payload["sessions"]["tools"], json!([]));
    let shown = prompt.shown.join("\n");
    assert!(shown.contains(".git: not a folder"), "{shown}");
    assert!(shown.contains("sessions: symlink"), "{shown}");
    assert!(reply.contains("nothing was written"), "{reply}");
    assert_eq!(places.config()?["permissions"], json!({}));
    Ok(())
}

#[test]
fn overfilled_material_stays_within_the_request_limit() -> Result<(), Box<dyn Error>> {
    // Given: every file, list and session part far over its share.
    let full = |label: &str| {
        (0..64)
            .map(|index| format!("[{label}{index}] {}", "\"".repeat(980)))
            .collect::<Vec<_>>()
    };
    let places = Places::new(json!({
        "allow": (0..200).map(|index| format!("exec(tool-{index} {})", "y".repeat(2_000))).collect::<Vec<_>>(),
        "autoMode": {
            "environment": full("Env"), "hard_deny": full("Hard"),
            "soft_deny": full("Soft"), "allow": full("Allow"),
        },
    }))?;
    let workspace = &places.workspace;
    for name in MATERIAL_NAMES {
        fs::write(workspace.join(name), "\"".repeat(20_000))?;
    }
    fs::create_dir_all(workspace.join(".git"))?;
    let remotes = (0..30)
        .map(|index| {
            format!(
                "[remote \"r{index}\"]\n\turl = https://host{index}.example/{}\n",
                "p".repeat(500)
            )
        })
        .collect::<String>();
    fs::write(workspace.join(".git/config"), remotes)?;
    fs::create_dir_all(workspace.join("sessions"))?;
    for session in 0..25 {
        let calls = (0..20)
            .map(|call| {
                let id = session * 20 + call;
                let name = if call % 2 == 0 {
                    format!("tool_{id}_{}", "n".repeat(80))
                } else {
                    "exec".to_owned()
                };
                json!({"id": format!("c{id}"), "function": {
                    "name": name,
                    "arguments": {
                        "command": format!("cmd{id}{} x", "c".repeat(80)),
                        "url": format!("https://h{id}{}.example/", "h".repeat(80)),
                    },
                }})
            })
            .collect::<Vec<_>>();
        let line = json!({"role": "assistant", "tool_calls": calls}).to_string();
        fs::write(workspace.join(format!("sessions/s{session}.jsonl")), line)?;
    }
    let client = ScriptedClient::new(Ok("[]"));

    // When
    run(
        &places.options(true),
        &client,
        &mut ScriptedPrompt::answering(None),
    )?;

    // Then
    let sent = client.user_texts().join("\n");
    assert!(sent.chars().count() <= 48_000, "{}", sent.chars().count());
    let payload: Value = serde_json::from_str(&sent)?;
    assert!(payload["workspace"].as_array().is_some_and(|items| {
        items.len() == 10 && items.iter().all(|item| item["truncated"] == true)
    }));
    for name in [
        "/remotes",
        "/sessions/tools",
        "/sessions/hosts",
        "/sessions/commands",
        "/current/autoMode/environment",
        "/current/allow",
    ] {
        let list = payload.pointer(name).cloned().unwrap_or_default();
        assert!(
            list.as_array()
                .and_then(|entries| entries.last())
                .and_then(Value::as_str)
                .is_some_and(|entry| entry.ends_with("more entries not shown]")),
            "{name}: {list}"
        );
    }
    Ok(())
}

#[test]
fn only_an_accepted_valid_draft_changes_environment_and_nothing_else() -> Result<(), Box<dyn Error>>
{
    let kept = json!({
        "mode": "auto",
        "autoMode": {"environment": ["$defaults", "[A] a"], "hardDeny": ["[H] h"]},
    });
    for (answer, consent, expected) in [
        (
            r#"["[A] a", "[B] b"]"#,
            Some("y"),
            Some(json!(["$defaults", "[A] a", "[B] b"])),
        ),
        (r#"["[B] b"]"#, Some("n"), None),
        (r#"["[B] b"]"#, None, None),
        ("not json", Some("y"), None),
        (r#"["no label"]"#, Some("y"), None),
        (r#"["$defaults"]"#, Some("y"), None),
    ] {
        // Given
        let places = Places::new(kept.clone())?;
        let before = fs::read(places.data.join("config.json"))?;
        let client = ScriptedClient::new(Ok(answer));

        // When
        let result = run(
            &places.options(false),
            &client,
            &mut ScriptedPrompt::answering(consent),
        );

        // Then
        let label = format!("{answer} {consent:?}");
        match expected {
            Some(environment) => {
                result.map_err(|error| format!("{label}: {error}"))?;
                let saved = places.config()?;
                assert_eq!(
                    saved["permissions"]["autoMode"]["environment"], environment,
                    "{label}"
                );
                assert_eq!(
                    saved["permissions"]["autoMode"]["hardDeny"],
                    json!(["[H] h"]),
                    "{label}"
                );
                assert_eq!(saved["permissions"]["mode"], "auto", "{label}");
                assert_eq!(
                    saved["providers"]["custom"]["apiKey"], "${CUSTOM_KEY}",
                    "{label}"
                );
            }
            None => assert_eq!(
                fs::read(places.data.join("config.json"))?,
                before,
                "{label}"
            ),
        }
    }
    Ok(())
}

#[test]
fn a_missing_config_gets_only_environment_and_a_changed_one_is_left_alone(
) -> Result<(), Box<dyn Error>> {
    // Given
    let places = Places::new(json!({}))?;
    let missing = places.data.join("new").join("config.json");
    let changed_path = places.data.join("config.json");
    let changed = changed_path.clone();
    let racing = ScriptedClient::with_hook(
        Ok(DRAFT),
        Some(Box::new(move || {
            let _ = fs::write(&changed, r#"{"permissions": {"mode": "default"}}"#);
        })),
    );

    // When
    let created = run(
        &AutoModeWriteOptions {
            config_path: Some(missing.clone()),
            workspace_override: Some(places.workspace.clone()),
            yes: true,
        },
        &ScriptedClient::new(Ok(DRAFT)),
        &mut ScriptedPrompt::answering(None),
    );
    let raced = run(
        &places.options(true),
        &racing,
        &mut ScriptedPrompt::answering(None),
    );

    // Then
    created?;
    assert_eq!(
        serde_json::from_slice::<Value>(&fs::read(&missing)?)?,
        json!({"permissions": {"autoMode": {"environment": [
            "$defaults", "[My Repo] github.com/me/repo, the user's own repository"
        ]}}})
    );
    let Err(error) = raced else {
        return Err("a config changed meanwhile was overwritten".into());
    };
    assert!(
        error.to_string().contains("changed while this command ran"),
        "{error}"
    );
    assert_eq!(
        fs::read_to_string(changed_path)?,
        r#"{"permissions": {"mode": "default"}}"#
    );
    Ok(())
}

#[test]
fn a_config_changed_after_the_first_read_is_left_alone() -> Result<(), Box<dyn Error>> {
    // Given: setup has read the config, then something else writes it.
    let places = Places::new(json!({}))?;
    let path = places.data.join("config.json");
    let first = ReadConfig::read(path.clone())?;
    fs::write(&path, r#"{"permissions": {"mode": "default"}}"#)?;
    let client = ScriptedClient::new(Ok(DRAFT));

    // When
    let result = setup_with(
        &places.options(true),
        first,
        &client.provider(),
        &mut ScriptedPrompt::answering(None),
    );

    // Then
    let Err(error) = result else {
        return Err("a config changed after the first read was overwritten".into());
    };
    assert!(
        error.to_string().contains("changed while this command ran"),
        "{error}"
    );
    assert_eq!(
        fs::read_to_string(path)?,
        r#"{"permissions": {"mode": "default"}}"#
    );
    Ok(())
}
