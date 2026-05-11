use crate::tools::filesystem::{raw_candidate_path, PathContext};
use crate::tools::sandbox::wrap_command;
use crate::tools::SchemaFragment;
use crate::tools::{IntegerSchema, JsonMap, StringSchema, Tool, ToolParameters, ToolResult};
use regex::Regex;
use serde_json::Value;
use shacs_security::NetworkGuard;
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

const DEFAULT_TIMEOUT_SECONDS: u64 = 60;
const MAX_TIMEOUT_SECONDS: u64 = 600;
const MAX_OUTPUT: usize = 10_000;

#[derive(Debug, Clone)]
pub struct ExecConfig {
    pub timeout_seconds: u64,
    pub working_dir: Option<PathBuf>,
    pub deny_patterns: Vec<String>,
    pub allow_patterns: Vec<String>,
    pub restrict_to_workspace: bool,
    pub sandbox: Option<String>,
    pub path_append: Option<String>,
    pub allowed_env_keys: Vec<String>,
    pub env: BTreeMap<String, String>,
    pub path_context: PathContext,
    pub network_guard: NetworkGuard,
}

impl ExecConfig {
    pub fn new(path_context: PathContext) -> Self {
        Self {
            timeout_seconds: DEFAULT_TIMEOUT_SECONDS,
            working_dir: path_context.workspace.clone(),
            deny_patterns: default_deny_patterns(),
            allow_patterns: Vec::new(),
            restrict_to_workspace: false,
            sandbox: None,
            path_append: None,
            allowed_env_keys: Vec::new(),
            env: BTreeMap::new(),
            path_context,
            network_guard: NetworkGuard::default(),
        }
    }
}

#[derive(Clone)]
pub struct ExecTool {
    config: ExecConfig,
}

impl ExecTool {
    pub fn new(config: ExecConfig) -> Self {
        Self { config }
    }

    pub fn with_workspace(workspace: impl Into<PathBuf>) -> Self {
        Self::new(ExecConfig::new(PathContext::workspace(workspace)))
    }
}

impl Tool for ExecTool {
    fn name(&self) -> &str {
        "exec"
    }

    fn description(&self) -> &str {
        "Execute a shell command and return stdout/stderr plus exit code. Output is truncated at 10,000 chars."
    }

    fn parameters(&self) -> Value {
        ToolParameters::new()
            .property("command", StringSchema::new("The shell command to execute"))
            .property(
                "working_dir",
                StringSchema::new("Optional working directory for the command"),
            )
            .property(
                "timeout",
                IntegerSchema::new("Timeout in seconds (default 60, max 600)")
                    .minimum(1)
                    .maximum(MAX_TIMEOUT_SECONDS as i64),
            )
            .required(["command"])
            .to_json_schema()
    }

    fn exclusive(&self) -> bool {
        true
    }

    fn execute(&self, params: JsonMap) -> ToolResult {
        let command = params
            .get("command")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if command.trim().is_empty() {
            return "Error executing command: Unknown command".into();
        }
        let working_dir = params.get("working_dir").and_then(Value::as_str);
        let timeout = params
            .get("timeout")
            .and_then(Value::as_u64)
            .unwrap_or(self.config.timeout_seconds)
            .min(MAX_TIMEOUT_SECONDS);

        match self.execute_command(command, working_dir, timeout) {
            Ok(output) => output.into(),
            Err(error) => format!("Error executing command: {error}").into(),
        }
    }
}

impl ExecTool {
    fn execute_command(
        &self,
        command: &str,
        working_dir: Option<&str>,
        timeout_seconds: u64,
    ) -> Result<String, String> {
        let mut cwd = self.resolve_working_dir(working_dir)?;
        if let Some(error) = self.guard_command(command, &cwd) {
            return Ok(error);
        }

        let mut command_text = command.to_owned();
        if let Some(sandbox) = &self.config.sandbox {
            let workspace = self
                .config
                .working_dir
                .as_deref()
                .or(self.config.path_context.workspace.as_deref())
                .unwrap_or(cwd.as_path());
            command_text = wrap_command(
                sandbox,
                &command_text,
                workspace,
                &cwd,
                self.config.path_context.media_dir.as_deref(),
            )?;
            cwd = std::fs::canonicalize(workspace).map_err(|error| error.to_string())?;
        }

        let mut env = self.build_env();
        if let Some(path_append) = &self.config.path_append {
            let path = env.get("PATH").cloned().unwrap_or_default();
            let separator = if cfg!(windows) { ";" } else { ":" };
            env.insert(
                "PATH".to_owned(),
                if path.is_empty() {
                    path_append.clone()
                } else {
                    format!("{path}{separator}{path_append}")
                },
            );
        }

        run_shell(
            &command_text,
            &cwd,
            &env,
            Duration::from_secs(timeout_seconds),
        )
    }

    fn resolve_working_dir(&self, working_dir: Option<&str>) -> Result<PathBuf, String> {
        let cwd = if let Some(working_dir) = working_dir {
            raw_candidate_path(working_dir, &self.config.path_context)
        } else if let Some(working_dir) = &self.config.working_dir {
            working_dir.clone()
        } else {
            std::env::current_dir().map_err(|error| error.to_string())?
        };
        let cwd = std::fs::canonicalize(&cwd)
            .map_err(|_| "working_dir could not be resolved".to_owned())?;
        if self.config.restrict_to_workspace {
            let Some(workspace) = self.config.working_dir.as_ref() else {
                return Err("configured workspace is missing".to_owned());
            };
            let workspace = std::fs::canonicalize(workspace)
                .map_err(|_| "working_dir could not be resolved".to_owned())?;
            if cwd != workspace && !cwd.starts_with(&workspace) {
                return Err("working_dir is outside the configured workspace".to_owned());
            }
        }
        Ok(cwd)
    }

    fn build_env(&self) -> HashMap<String, String> {
        let mut env = HashMap::new();
        env.insert(
            "HOME".to_owned(),
            std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_owned()),
        );
        env.insert(
            "LANG".to_owned(),
            std::env::var("LANG").unwrap_or_else(|_| "C.UTF-8".to_owned()),
        );
        env.insert(
            "TERM".to_owned(),
            std::env::var("TERM").unwrap_or_else(|_| "dumb".to_owned()),
        );
        env.insert("PATH".to_owned(), std::env::var("PATH").unwrap_or_default());
        for key in &self.config.allowed_env_keys {
            if let Ok(value) = std::env::var(key) {
                env.insert(key.clone(), value);
            }
        }
        env.extend(self.config.env.clone());
        env
    }

    fn guard_command(&self, command: &str, cwd: &Path) -> Option<String> {
        let lower = command.trim().to_ascii_lowercase();
        for pattern in &self.config.deny_patterns {
            let Ok(regex) = Regex::new(pattern) else {
                return Some(
                    "Error: Command blocked by safety guard (invalid deny pattern)".to_owned(),
                );
            };
            if regex.is_match(&lower) {
                return Some(
                    "Error: Command blocked by safety guard (dangerous pattern detected)"
                        .to_owned(),
                );
            }
        }
        if !self.config.allow_patterns.is_empty()
            && !self.config.allow_patterns.iter().any(|pattern| {
                Regex::new(pattern)
                    .map(|regex| regex.is_match(&lower))
                    .unwrap_or(false)
            })
        {
            return Some("Error: Command blocked by safety guard (not in allowlist)".to_owned());
        }
        if self.config.network_guard.contains_internal_url(&lower) {
            return Some(
                "Error: Command blocked by safety guard (internal/private URL detected)".to_owned(),
            );
        }
        if self.config.restrict_to_workspace {
            if lower.contains("../") || lower.contains("..\\") {
                return Some(
                    "Error: Command blocked by safety guard (path traversal detected)".to_owned(),
                );
            }
            for raw in extract_absolute_paths(command) {
                let candidate = expand_shell_path(&raw);
                if let Some(path) = canonicalize_existing_prefix(&candidate) {
                    let media_allowed = self
                        .config
                        .path_context
                        .media_dir
                        .as_ref()
                        .and_then(|path| std::fs::canonicalize(path).ok())
                        .is_some_and(|media| path == media || path.starts_with(media));
                    if path != cwd && !path.starts_with(cwd) && !media_allowed {
                        return Some(
                            "Error: Command blocked by safety guard (path outside working dir)"
                                .to_owned(),
                        );
                    }
                }
            }
        }
        None
    }
}

fn run_shell(
    command: &str,
    cwd: &Path,
    env: &HashMap<String, String>,
    timeout: Duration,
) -> Result<String, String> {
    let mut child = if cfg!(windows) {
        Command::new("cmd.exe")
            .arg("/c")
            .arg(command)
            .current_dir(cwd)
            .env_clear()
            .envs(env)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
    } else {
        Command::new("/bin/bash")
            .arg("-l")
            .arg("-c")
            .arg(command)
            .current_dir(cwd)
            .env_clear()
            .envs(env)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
    }
    .map_err(|error| error.to_string())?;

    let start = Instant::now();
    loop {
        if child
            .try_wait()
            .map_err(|error| error.to_string())?
            .is_some()
        {
            let output = child
                .wait_with_output()
                .map_err(|error| error.to_string())?;
            return Ok(format_output(output));
        }
        if start.elapsed() >= timeout {
            let _ = child.kill();
            let _ = child.wait();
            return Ok(format!(
                "Error: Command timed out after {} seconds",
                timeout.as_secs()
            ));
        }
        thread::sleep(Duration::from_millis(20));
    }
}

fn format_output(output: std::process::Output) -> String {
    let mut parts = Vec::new();
    if !output.stdout.is_empty() {
        parts.push(String::from_utf8_lossy(&output.stdout).to_string());
    }
    if !output.stderr.is_empty() {
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        if !stderr.trim().is_empty() {
            parts.push(format!("STDERR:\n{stderr}"));
        }
    }
    parts.push(format!(
        "\nExit code: {}",
        output.status.code().unwrap_or(-1)
    ));
    let result = if parts.is_empty() {
        "(no output)".to_owned()
    } else {
        parts.join("\n")
    };
    truncate_output(result)
}

fn truncate_output(result: String) -> String {
    let char_count = result.chars().count();
    if char_count <= MAX_OUTPUT {
        return result;
    }
    let half = MAX_OUTPUT / 2;
    let first = result.chars().take(half).collect::<String>();
    let last = result
        .chars()
        .rev()
        .take(half)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<String>();
    format!(
        "{}\n\n... ({} chars truncated) ...\n\n{}",
        first,
        char_count - MAX_OUTPUT,
        last
    )
}

fn default_deny_patterns() -> Vec<String> {
    [
        r"\brm\s+-[rf]{1,2}\b",
        r"\bdel\s+/[fq]\b",
        r"\brmdir\s+/s\b",
        r"(?:^|[;&|]\s*)format\b",
        r"\b(mkfs|diskpart)\b",
        r"\bdd\s+if=",
        r">\s*/dev/sd",
        r"\b(shutdown|reboot|poweroff)\b",
        r":\(\)\s*\{.*\};\s*:",
        r">>?\s*\S*(?:history\.jsonl|\.dream_cursor)",
        r"\btee\b[^|;&<>]*(?:history\.jsonl|\.dream_cursor)",
        r"\b(?:cp|mv)\b(?:\s+[^\s|;&<>]+)+\s+\S*(?:history\.jsonl|\.dream_cursor)",
        r"\bdd\b[^|;&<>]*\bof=\S*(?:history\.jsonl|\.dream_cursor)",
        r"\bsed\s+-i[^|;&<>]*(?:history\.jsonl|\.dream_cursor)",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect()
}

fn extract_absolute_paths(command: &str) -> Vec<String> {
    let mut paths = Vec::new();
    if let Ok(regex) = Regex::new(r#"(?:^|[\s|>'"])(/[^\s"'>;|<]+)"#) {
        paths.extend(
            regex
                .captures_iter(command)
                .filter_map(|captures| captures.get(1))
                .map(|value| value.as_str().to_owned()),
        );
    }
    if let Ok(regex) = Regex::new(r#"(?:^|[\s|>'"])(~[^\s"'>;|<]*)"#) {
        paths.extend(
            regex
                .captures_iter(command)
                .filter_map(|captures| captures.get(1))
                .map(|value| value.as_str().to_owned()),
        );
    }
    paths
}

fn expand_shell_path(path: &str) -> PathBuf {
    if path == "~" {
        return std::env::var_os("HOME").map_or_else(|| PathBuf::from(path), PathBuf::from);
    }
    if let Some(rest) = path.strip_prefix("~/") {
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home).join(rest);
        }
    }
    PathBuf::from(path)
}

fn canonicalize_existing_prefix(path: &Path) -> Option<PathBuf> {
    if let Ok(path) = std::fs::canonicalize(path) {
        return Some(path);
    }
    let mut missing = Vec::new();
    let mut current = path;
    loop {
        let parent = current.parent()?;
        let name = current.file_name()?.to_owned();
        missing.push(name);
        if let Ok(mut prefix) = std::fs::canonicalize(parent) {
            for component in missing.iter().rev() {
                prefix.push(component);
            }
            return Some(prefix);
        }
        current = parent;
    }
}
