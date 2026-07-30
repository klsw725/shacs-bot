use serde_json::json;
use shacs_config::{save_config_to_path, Config, ProviderConfig};
use shacs_providers::{LlmResponse, ToolCallRequest};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

pub fn write_file_response(path: &Path, content: &str) -> LlmResponse {
    LlmResponse {
        finish_reason: "tool_calls".to_owned(),
        tool_calls: vec![ToolCallRequest::new(
            "call_write_file",
            "write_file",
            serde_json::Map::from_iter([
                ("path".to_owned(), json!(path.to_string_lossy().to_string())),
                ("content".to_owned(), json!(content)),
            ]),
        )],
        ..LlmResponse::default()
    }
}

pub fn text_response(content: &str) -> LlmResponse {
    LlmResponse {
        content: Some(content.to_owned()),
        ..LlmResponse::default()
    }
}

pub fn write_config(root: &Path, workspace: &Path) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let config_path = root.join("data").join("config.json");
    fs::create_dir_all(config_path.parent().ok_or("missing config parent")?)?;
    let mut config = Config::default();
    config.agents.defaults.workspace = workspace.to_string_lossy().to_string();
    config.agents.defaults.provider = "custom".to_owned();
    config.agents.defaults.model = "test-model".to_owned();
    config.agents.defaults.max_tool_iterations = 2;
    config.providers.insert(
        "custom".to_owned(),
        ProviderConfig {
            api_key: Some("sk-fake-provider".to_owned()),
            api_key_ref: None,
            api_base: Some("http://127.0.0.1:1/v1".to_owned()),
            extra_headers: None,
            extra_body: None,
        },
    );
    save_config_to_path(&config, &config_path)?;
    Ok(config_path)
}

pub fn remembered_rule_id(
    config_path: &Path,
    workspace: &Path,
) -> Result<String, Box<dyn std::error::Error>> {
    let listed = shacs_bot(config_path, Path::new("/dev/null"))
        .args([
            "permissions",
            "list",
            "--workspace",
            workspace_arg(workspace).as_str(),
        ])
        .output()?;
    assert_success(&listed, "permissions list")?;
    let output = stdout_text(&listed);
    output
        .lines()
        .find_map(|line| line.trim().strip_prefix("- "))
        .and_then(|line| line.split_whitespace().next())
        .map(str::to_owned)
        .ok_or_else(|| format!("missing remembered rule id in output: {output}").into())
}

pub fn shacs_bot(config_path: &Path, fake_responses_path: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_shacs-bot"));
    command.arg("--config").arg(config_path);
    command.env("SHACS_DEBUG_FAKE_PROVIDER_RESPONSES", fake_responses_path);
    command
}

pub fn workspace_arg(workspace: &Path) -> String {
    workspace.to_string_lossy().to_string()
}

pub fn stdout_text(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).to_string()
}

pub fn stderr_text(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).to_string()
}

pub fn assert_success(output: &Output, label: &str) -> Result<(), Box<dyn std::error::Error>> {
    if output.status.success() {
        Ok(())
    } else {
        Err(format!(
            "{label} failed with status {:?}\nstdout:\n{}\nstderr:\n{}",
            output.status.code(),
            stdout_text(output),
            stderr_text(output)
        )
        .into())
    }
}
