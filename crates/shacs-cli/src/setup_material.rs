//! What `permissions auto-mode setup` reads to draft `environment` entries
//! (Spec 048 PRD 050 scope 7): instruction and project files, git remotes,
//! and the tools, hosts and commands of recent sessions' tool calls. Files
//! are read through open descriptors only, each part is cut to its share,
//! and everything is redacted with `.env.json` values masked.

use crate::review_provider::{self, MAX_ENTRY_CHARS};
use serde_json::{json, Value};
use shacs_config::PermissionsConfig;
use shacs_core::runtime::{PermissionEnvironment, SecretValues};
use std::collections::BTreeSet;
use std::path::Path;

const INSTRUCTION_FILES: [&str; 5] = ["AGENTS.md", "SOUL.md", "USER.md", "TOOLS.md", "README.md"];
const PROJECT_FILES: [&str; 5] = [
    "package.json",
    "pyproject.toml",
    "Cargo.toml",
    "compose.yaml",
    "docker-compose.yml",
];
const INSTRUCTION_FILE_CHARS: usize = 4_000;
const PROJECT_FILE_CHARS: usize = 2_000;
const MAX_REMOTES: usize = 10;
const REMOTE_CHARS: usize = 300;
const GIT_CONFIG_BYTES: usize = 64 * 1024;
const ENVIRONMENT_CHARS: usize = 4_000;
const CURRENT_LIST_CHARS: usize = 1_000;
const MAX_SESSIONS: usize = 20;
const SESSION_BYTES: usize = 2 * 1024 * 1024;
const SESSION_ENTRY_CHARS: usize = 64;
const MAX_SESSION_ENTRIES: usize = 200;
const SESSION_LIST_CHARS: usize = 1_500;

/// The request material and the candidates left unread.
#[derive(Debug, Default)]
pub(crate) struct SetupMaterial {
    pub(crate) workspace: Vec<Value>,
    pub(crate) remotes: Vec<String>,
    pub(crate) tools: Vec<String>,
    pub(crate) hosts: Vec<String>,
    pub(crate) commands: Vec<String>,
    /// `name: reason` for each candidate left unread, and notes about parts
    /// not read at all.
    pub(crate) skipped: Vec<String>,
}

/// The lists the user wrote that a draft adds to.
pub(crate) fn current(permissions: &PermissionsConfig, secrets: &SecretValues) -> Value {
    let auto_mode = &permissions.auto_mode;
    let mut lists = serde_json::Map::new();
    for (name, list, limit) in [
        ("environment", &auto_mode.environment, ENVIRONMENT_CHARS),
        ("hard_deny", &auto_mode.hard_deny, CURRENT_LIST_CHARS),
        ("soft_deny", &auto_mode.soft_deny, CURRENT_LIST_CHARS),
        ("allow", &auto_mode.allow, CURRENT_LIST_CHARS),
    ] {
        if let Some(list) = list {
            lists.insert(
                name.to_owned(),
                json!(fitted(list, secrets, MAX_ENTRY_CHARS, limit)),
            );
        }
    }
    json!({
        "autoMode": lists,
        "allow": fitted(&permissions.user_rules.allow, secrets, MAX_ENTRY_CHARS, CURRENT_LIST_CHARS),
    })
}

#[cfg(unix)]
pub(crate) fn read(
    workspace: &Path,
    environment: &PermissionEnvironment,
    secrets: &SecretValues,
) -> SetupMaterial {
    use shacs_core::runtime::ConfinedFolder;
    let mut material = SetupMaterial::default();
    let Ok(folder) = ConfinedFolder::open(workspace) else {
        material.skipped.push(
            "workspace: it could not be opened without following a symlink, so no workspace file or session was read"
                .to_owned(),
        );
        return material;
    };
    for (names, limit) in [
        (&INSTRUCTION_FILES, INSTRUCTION_FILE_CHARS),
        (&PROJECT_FILES, PROJECT_FILE_CHARS),
    ] {
        for name in names {
            if let Some(item) = file_item(
                &folder,
                workspace,
                name,
                limit,
                environment,
                secrets,
                &mut material.skipped,
            ) {
                material.workspace.push(item);
            }
        }
    }
    material.remotes = remotes(
        &folder,
        workspace,
        environment,
        secrets,
        &mut material.skipped,
    );
    read_sessions(&folder, workspace, environment, secrets, &mut material);
    material
}

#[cfg(not(unix))]
pub(crate) fn read(
    _workspace: &Path,
    _environment: &PermissionEnvironment,
    _secrets: &SecretValues,
) -> SetupMaterial {
    SetupMaterial {
        skipped: vec![
            "workspace: files and sessions are read only on Unix, so the draft uses the config alone"
                .to_owned(),
        ],
        ..SetupMaterial::default()
    }
}

#[cfg(unix)]
fn file_item(
    folder: &shacs_core::runtime::ConfinedFolder,
    workspace: &Path,
    name: &str,
    limit: usize,
    environment: &PermissionEnvironment,
    secrets: &SecretValues,
    skipped: &mut Vec<String>,
) -> Option<Value> {
    use shacs_core::runtime::ConfinedSkip;
    if environment.withholds_read(&workspace.join(name)) {
        skipped.push(format!("{name}: permission rule"));
        return None;
    }
    let max_bytes = limit * 4 + 1;
    let bytes = match folder.read_prefix(Path::new(name), max_bytes) {
        Ok(bytes) => bytes,
        Err(ConfinedSkip::Missing) => return None,
        Err(skip) => {
            skipped.push(format!("{name}: {}", skip.reason()));
            return None;
        }
    };
    let text = review_provider::shown(secrets, &String::from_utf8_lossy(&bytes));
    let (text, cut) = review_provider::fit_text(&text, limit);
    let mut item = json!({"name": name, "text": text});
    if cut || bytes.len() == max_bytes {
        item["truncated"] = Value::Bool(true);
    }
    Some(item)
}

#[cfg(unix)]
fn remotes(
    folder: &shacs_core::runtime::ConfinedFolder,
    workspace: &Path,
    environment: &PermissionEnvironment,
    secrets: &SecretValues,
    skipped: &mut Vec<String>,
) -> Vec<String> {
    use shacs_core::runtime::ConfinedSkip;
    let git = match folder.folder(Path::new(".git")) {
        Ok(git) => git,
        Err(ConfinedSkip::Missing) => return Vec::new(),
        Err(skip) => {
            skipped.push(format!(".git: {}", skip.reason()));
            return Vec::new();
        }
    };
    if environment.withholds_read(&workspace.join(".git").join("config")) {
        skipped.push(".git/config: permission rule".to_owned());
        return Vec::new();
    }
    let bytes = match git.read_prefix(Path::new("config"), GIT_CONFIG_BYTES) {
        Ok(bytes) => bytes,
        Err(ConfinedSkip::Missing) => return Vec::new(),
        Err(skip) => {
            skipped.push(format!(".git/config: {}", skip.reason()));
            return Vec::new();
        }
    };
    let urls = remote_urls(&String::from_utf8_lossy(&bytes))
        .iter()
        .map(|url| review_provider::cut_entry(&review_provider::shown(secrets, url), REMOTE_CHARS))
        .collect::<Vec<_>>();
    first(urls, MAX_REMOTES)
}

/// The `url` of each `[remote "…"]` section, without user info, query or
/// fragment.
pub(crate) fn remote_urls(config: &str) -> Vec<String> {
    let mut urls = Vec::new();
    let mut in_remote = false;
    for line in config.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_remote = line.starts_with("[remote ");
            continue;
        }
        if !in_remote {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        if key.trim().eq_ignore_ascii_case("url") {
            urls.push(clean_url(value.trim().trim_matches('"')));
        }
    }
    urls
}

fn clean_url(url: &str) -> String {
    let url = url.split(['?', '#']).next().unwrap_or_default();
    match url.split_once("://") {
        Some((scheme, rest)) => {
            let (authority, path) = rest.split_at(rest.find('/').unwrap_or(rest.len()));
            let host = authority.rsplit('@').next().unwrap_or(authority);
            format!("{scheme}://{host}{path}")
        }
        None => match url.split_once('@') {
            Some((_, rest)) if !url.contains('/') || url.find('@') < url.find('/') => {
                rest.to_owned()
            }
            _ => url.to_owned(),
        },
    }
}

#[cfg(unix)]
fn read_sessions(
    folder: &shacs_core::runtime::ConfinedFolder,
    workspace: &Path,
    environment: &PermissionEnvironment,
    secrets: &SecretValues,
    material: &mut SetupMaterial,
) {
    use shacs_core::runtime::ConfinedSkip;
    if environment.withholds_read(&workspace.join("sessions")) {
        material
            .skipped
            .push("sessions: permission rule".to_owned());
        return;
    }
    let sessions = match folder.folder(Path::new("sessions")) {
        Ok(sessions) => sessions,
        Err(ConfinedSkip::Missing) => return,
        Err(skip) => {
            material.skipped.push(format!(
                "sessions: {}, so no session was read",
                skip.reason()
            ));
            return;
        }
    };
    let Ok(mut entries) = sessions.regular_files() else {
        material.skipped.push("sessions: unreadable".to_owned());
        return;
    };
    entries.retain(|entry| entry.name.ends_with(".jsonl"));
    entries.sort_by_key(|entry| std::cmp::Reverse(entry.modified));
    let mut seen = SessionCalls::default();
    for entry in entries.iter().take(MAX_SESSIONS) {
        if environment.withholds_read(&workspace.join("sessions").join(&entry.name)) {
            material
                .skipped
                .push(format!("sessions/{}: permission rule", entry.name));
            continue;
        }
        match sessions.read_prefix(Path::new(&entry.name), SESSION_BYTES) {
            Ok(bytes) => seen.add_session(&String::from_utf8_lossy(&bytes), secrets),
            Err(skip) => {
                material
                    .skipped
                    .push(format!("sessions/{}: {}", entry.name, skip.reason()))
            }
        }
    }
    material.tools = review_provider::fit_list(seen.tools.list, SESSION_LIST_CHARS);
    material.hosts = review_provider::fit_list(seen.hosts.list, SESSION_LIST_CHARS);
    material.commands = review_provider::fit_list(seen.commands.list, SESSION_LIST_CHARS);
}

/// Tool names, URL hosts and command words, each once and in the order
/// first seen.
#[derive(Default)]
pub(crate) struct SessionCalls {
    pub(crate) tools: Seen,
    pub(crate) hosts: Seen,
    pub(crate) commands: Seen,
}

#[derive(Default)]
pub(crate) struct Seen {
    pub(crate) list: Vec<String>,
    known: BTreeSet<String>,
}

impl Seen {
    fn add(&mut self, text: &str, secrets: &SecretValues) {
        if self.list.len() >= MAX_SESSION_ENTRIES {
            return;
        }
        let entry =
            review_provider::cut_entry(&review_provider::shown(secrets, text), SESSION_ENTRY_CHARS);
        if self.known.insert(entry.clone()) {
            self.list.push(entry);
        }
    }
}

impl SessionCalls {
    /// Reads only the assistant's tool calls: names, URL hosts in the
    /// arguments, and the first word of `exec` commands. Messages and tool
    /// results stay unread.
    pub(crate) fn add_session(&mut self, jsonl: &str, secrets: &SecretValues) {
        for line in jsonl.lines() {
            let Ok(message) = serde_json::from_str::<Value>(line) else {
                continue;
            };
            if message.get("role").and_then(Value::as_str) != Some("assistant") {
                continue;
            }
            let calls = message.get("tool_calls").and_then(Value::as_array);
            for call in calls.into_iter().flatten() {
                let function = call.get("function").unwrap_or(call);
                let Some(name) = function.get("name").and_then(Value::as_str) else {
                    continue;
                };
                self.tools.add(name, secrets);
                let arguments = match function.get("arguments") {
                    Some(Value::String(text)) => serde_json::from_str(text).unwrap_or(Value::Null),
                    Some(other) => other.clone(),
                    None => Value::Null,
                };
                for host in url_hosts(&arguments) {
                    self.hosts.add(&host, secrets);
                }
                if name == "exec" {
                    if let Some(word) = arguments
                        .get("command")
                        .and_then(Value::as_str)
                        .and_then(command_word)
                    {
                        self.commands.add(&word, secrets);
                    }
                }
            }
        }
    }
}

fn url_hosts(value: &Value) -> Vec<String> {
    let mut hosts = Vec::new();
    let mut texts = vec![value];
    while let Some(value) = texts.pop() {
        match value {
            Value::String(text) => {
                for (start, _) in text.match_indices("://") {
                    let scheme = text[..start]
                        .rsplit(|character: char| !character.is_ascii_alphanumeric())
                        .next()
                        .unwrap_or_default();
                    if !matches!(scheme.to_ascii_lowercase().as_str(), "http" | "https") {
                        continue;
                    }
                    let rest = &text[start + 3..];
                    let authority = rest
                        .split(|character: char| {
                            matches!(character, '/' | '?' | '#' | '"' | '\'' | '`')
                                || character.is_whitespace()
                        })
                        .next()
                        .unwrap_or_default();
                    let host = authority.rsplit('@').next().unwrap_or(authority);
                    let host = host.split(':').next().unwrap_or(host);
                    if !host.is_empty() {
                        hosts.push(host.to_ascii_lowercase());
                    }
                }
            }
            Value::Array(items) => texts.extend(items.iter().rev()),
            Value::Object(object) => texts.extend(object.values().rev()),
            Value::Null | Value::Bool(_) | Value::Number(_) => {}
        }
    }
    hosts
}

/// The command's first word after `NAME=value` assignments, the last part
/// of a path; a word that still holds `=` is dropped.
pub(crate) fn command_word(command: &str) -> Option<String> {
    let word = command
        .split_whitespace()
        .find(|word| !is_assignment(word))?;
    let word = word.rsplit('/').next().unwrap_or(word);
    (!word.is_empty() && !word.contains('=')).then(|| word.to_owned())
}

fn is_assignment(word: &str) -> bool {
    word.split_once('=').is_some_and(|(name, _)| {
        !name.is_empty()
            && name
                .chars()
                .all(|character| character.is_ascii_alphanumeric() || character == '_')
            && !name.starts_with(|character: char| character.is_ascii_digit())
    })
}

fn fitted(
    list: &[String],
    secrets: &SecretValues,
    entry_chars: usize,
    list_chars: usize,
) -> Vec<String> {
    let entries = list
        .iter()
        .map(|entry| {
            review_provider::cut_entry(&review_provider::shown(secrets, entry), entry_chars)
        })
        .collect();
    review_provider::fit_list(entries, list_chars)
}

fn first(mut entries: Vec<String>, count: usize) -> Vec<String> {
    if entries.len() > count {
        let left_out = entries.len() - count;
        entries.truncate(count);
        entries.push(format!("[{left_out} more entries not shown]"));
    }
    entries
}
