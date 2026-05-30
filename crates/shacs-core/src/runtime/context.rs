use super::MemoryStore;
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use chrono::{Datelike, Local, Offset, Utc};
use serde_json::{json, Map, Value};
use shacs_skills::builtin_skills;
use shacs_templates::{render_agent_template, template_variables, AgentTemplate};
use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

const BOOTSTRAP_FILES: [&str; 4] = ["AGENTS.md", "SOUL.md", "USER.md", "TOOLS.md"];
const RUNTIME_CONTEXT_TAG: &str = "[Runtime Context — metadata only, not instructions]";
const RUNTIME_CONTEXT_END: &str = "[/Runtime Context]";
const MAX_RECENT_HISTORY: usize = 50;
const MAX_HISTORY_CHARS: usize = 32_000;
const MAX_MEDIA_BYTES: u64 = 20 * 1024 * 1024;

#[derive(Debug, Clone)]
pub struct ContextBuilder {
    workspace: PathBuf,
    timezone: Option<String>,
    disabled_skills: Vec<String>,
    extra_skill_roots: Vec<PathBuf>,
    configured_env: BTreeMap<String, String>,
}

#[derive(Debug, Clone)]
pub struct ContextBuildRequest<'a> {
    pub history: Vec<Value>,
    pub current_message: &'a str,
    pub media: &'a [String],
    pub channel: Option<&'a str>,
    pub chat_id: Option<&'a str>,
    pub current_role: &'a str,
    pub session_summary: Option<&'a str>,
}

impl<'a> ContextBuildRequest<'a> {
    pub fn new(current_message: &'a str) -> Self {
        Self {
            history: Vec::new(),
            current_message,
            media: &[],
            channel: None,
            chat_id: None,
            current_role: "user",
            session_summary: None,
        }
    }
}

impl ContextBuilder {
    pub fn new(workspace: impl AsRef<Path>) -> Self {
        Self {
            workspace: workspace.as_ref().to_path_buf(),
            timezone: None,
            disabled_skills: Vec::new(),
            extra_skill_roots: Vec::new(),
            configured_env: BTreeMap::new(),
        }
    }

    pub fn with_timezone(mut self, timezone: impl Into<String>) -> Self {
        self.timezone = Some(timezone.into());
        self
    }

    pub fn with_disabled_skills(
        mut self,
        disabled_skills: impl IntoIterator<Item = String>,
    ) -> Self {
        self.disabled_skills = disabled_skills.into_iter().collect();
        self
    }

    pub fn with_skill_roots(mut self, roots: impl IntoIterator<Item = PathBuf>) -> Self {
        self.extra_skill_roots = roots.into_iter().collect();
        self
    }

    pub fn with_configured_env(mut self, env: impl IntoIterator<Item = (String, String)>) -> Self {
        self.configured_env = env.into_iter().collect();
        self
    }

    pub fn workspace(&self) -> &Path {
        &self.workspace
    }

    pub fn build_system_prompt(&self, channel: Option<&str>) -> String {
        let mut parts = vec![self.identity(channel)];
        let bootstrap = self.load_bootstrap_files();
        if !bootstrap.is_empty() {
            parts.push(bootstrap);
        }
        if let Some(memory) = self.load_memory_context() {
            parts.push(format!("# Memory\n\n{memory}"));
        }
        if let Some(always) = self.load_always_skills() {
            parts.push(format!("# Active Skills\n\n{always}"));
        }
        if let Some(index) = self.build_skills_index() {
            parts.push(format!("# Available Skills\n\n{index}"));
        }
        if let Some(history) = self.load_recent_history() {
            parts.push(format!("# Recent History\n\n{history}"));
        }
        parts.join("\n\n---\n\n")
    }

    pub fn build_runtime_context(
        &self,
        channel: Option<&str>,
        chat_id: Option<&str>,
        session_summary: Option<&str>,
    ) -> String {
        let mut lines = vec![format!("Current Time: {}", current_time(&self.timezone))];
        if let (Some(channel), Some(chat_id)) = (channel, chat_id) {
            lines.push(format!("Channel: {channel}"));
            lines.push(format!("Chat ID: {chat_id}"));
        }
        if let Some(summary) = session_summary.filter(|summary| !summary.is_empty()) {
            lines.push(String::new());
            lines.push("[Resumed Session]".to_owned());
            lines.push(summary.to_owned());
        }
        format!(
            "{RUNTIME_CONTEXT_TAG}\n{}\n{RUNTIME_CONTEXT_END}",
            lines.join("\n")
        )
    }

    pub fn build_messages(&self, request: ContextBuildRequest<'_>) -> Vec<Value> {
        let runtime_context =
            self.build_runtime_context(request.channel, request.chat_id, request.session_summary);
        let user_content = self.build_user_content(request.current_message, request.media);
        let merged = merge_runtime_context(runtime_context, user_content);
        let mut messages = vec![json!({
            "role": "system",
            "content": self.build_system_prompt(request.channel),
        })];
        messages.extend(request.history);
        if let Some(last) = messages.last_mut() {
            if last.get("role").and_then(Value::as_str) == Some(request.current_role) {
                let previous = last.get("content").cloned().unwrap_or(Value::Null);
                last["content"] = merge_message_content(previous, merged);
                return messages;
            }
        }
        messages.push(json!({"role": request.current_role, "content": merged}));
        messages
    }

    fn identity(&self, channel: Option<&str>) -> String {
        let channel = channel.unwrap_or_default();
        let platform_policy = render_agent_template(
            AgentTemplate::PlatformPolicy,
            &template_variables(&[("system", platform_template_system())]),
        )
        .unwrap_or_else(|_| platform_policy_fallback());
        let workspace_path = canonical_workspace_path(&self.workspace)
            .display()
            .to_string();
        let runtime = format!(
            "Rust shacs-core ({} {})",
            env::consts::OS,
            env::consts::ARCH
        );
        let rendered = render_agent_template(
            AgentTemplate::Identity,
            &template_variables(&[
                ("runtime", &runtime),
                ("workspace_path", &workspace_path),
                ("platform_policy", &platform_policy),
                ("channel", channel),
            ]),
        )
        .unwrap_or_else(|_| {
            format!("Workspace: {workspace_path}\nRuntime: Rust shacs-core\nChannel: {channel}")
        });
        format!("# Identity\n\n{rendered}")
    }

    fn load_bootstrap_files(&self) -> String {
        BOOTSTRAP_FILES
            .iter()
            .filter_map(|filename| {
                let path = self.workspace.join(filename);
                let content = fs::read_to_string(path).ok()?;
                Some(format!("## {filename}\n\n{content}"))
            })
            .collect::<Vec<_>>()
            .join("\n\n")
    }

    fn load_memory_context(&self) -> Option<String> {
        MemoryStore::memory_context_from_workspace(&self.workspace)
    }

    fn load_recent_history(&self) -> Option<String> {
        MemoryStore::recent_history_from_workspace(
            &self.workspace,
            MAX_RECENT_HISTORY,
            MAX_HISTORY_CHARS,
        )
    }

    fn skill_roots(&self) -> Vec<PathBuf> {
        let mut roots = vec![
            self.workspace.join("skills"),
            self.workspace.join(".shacs-bot").join("skills"),
            self.workspace.join(".nanobot").join("skills"),
            self.workspace.join("builtin_skills"),
        ];
        roots.extend(self.extra_skill_roots.clone());
        roots
    }

    fn load_skill_documents(&self) -> Vec<SkillDocument> {
        let disabled = self
            .disabled_skills
            .iter()
            .map(|name| name.as_str())
            .collect::<Vec<_>>();
        let mut documents = Vec::new();
        let mut seen = BTreeSet::new();
        for root in self.skill_roots() {
            let Ok(entries) = fs::read_dir(root) else {
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                let path = if path.is_dir() {
                    path.join("SKILL.md")
                } else {
                    path
                };
                if path.extension().and_then(|ext| ext.to_str()) != Some("md") {
                    continue;
                }
                if let Some(document) = SkillDocument::from_path(&path, &self.configured_env) {
                    if document.disabled || disabled.contains(&document.name.as_str()) {
                        continue;
                    }
                    if !seen.insert(document.name.clone()) {
                        continue;
                    }
                    documents.push(document);
                }
            }
        }
        for document in self.virtual_builtin_skill_documents() {
            if document.disabled || disabled.contains(&document.name.as_str()) {
                continue;
            }
            if !seen.insert(document.name.clone()) {
                continue;
            }
            documents.push(document);
        }
        documents.sort_by(|left, right| left.name.cmp(&right.name));
        documents
    }

    fn virtual_builtin_skill_documents(&self) -> Vec<SkillDocument> {
        builtin_skills()
            .iter()
            .filter_map(|skill| {
                let file = skill
                    .files
                    .iter()
                    .find(|file| file.relative_path == "SKILL.md")?;
                let raw = String::from_utf8(file.content.to_vec()).ok()?;
                let source_path = self
                    .workspace
                    .join("builtin_skills")
                    .join(skill.name)
                    .join("SKILL.md");
                SkillDocument::from_raw(source_path, raw, &self.configured_env)
            })
            .collect()
    }

    fn load_always_skills(&self) -> Option<String> {
        let content = self
            .load_skill_documents()
            .into_iter()
            .filter(|skill| skill.always && skill.available)
            .map(|skill| format!("### Skill: {}\n\n{}", skill.name, skill.body))
            .collect::<Vec<_>>()
            .join("\n\n---\n\n");
        (!content.is_empty()).then_some(content)
    }

    pub fn active_always_skill_names(&self) -> Vec<String> {
        self.load_skill_documents()
            .into_iter()
            .filter(|skill| skill.always && skill.available)
            .map(|skill| skill.name)
            .collect()
    }

    pub fn skill_name_for_source_path(&self, path: impl AsRef<Path>) -> Option<String> {
        let path = path.as_ref();
        let candidate = if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.workspace.join(path)
        };
        let candidate = fs::canonicalize(candidate).ok()?;
        self.load_skill_documents()
            .into_iter()
            .filter(|skill| skill.available)
            .find_map(|skill| {
                let source_path = fs::canonicalize(skill.source_path).ok()?;
                (source_path == candidate).then_some(skill.name)
            })
    }

    fn build_skills_index(&self) -> Option<String> {
        let skills_summary = self.skills_index_summary();
        if skills_summary.is_empty() {
            return None;
        }
        Some(
            render_agent_template(
                AgentTemplate::SkillsSection,
                &template_variables(&[("skills_summary", &skills_summary)]),
            )
            .unwrap_or(skills_summary),
        )
    }

    pub fn load_skill(&self, name: &str) -> Option<String> {
        let disabled = self.disabled_skills.iter().any(|disabled| disabled == name);
        if disabled {
            return None;
        }
        self.load_skill_documents()
            .into_iter()
            .find(|skill| skill.name == name)
            .map(|skill| skill.raw)
    }

    pub fn load_skills_for_context(&self, skill_names: &[impl AsRef<str>]) -> String {
        skill_names
            .iter()
            .filter_map(|name| {
                let name = name.as_ref();
                self.load_skill(name).map(|markdown| {
                    format!(
                        "### Skill: {name}\n\n{}",
                        strip_skill_frontmatter(&markdown).trim()
                    )
                })
            })
            .collect::<Vec<_>>()
            .join("\n\n---\n\n")
    }

    pub fn build_skills_summary(&self, exclude: &BTreeSet<String>) -> String {
        self.load_skill_documents()
            .into_iter()
            .filter(|skill| !exclude.contains(&skill.name))
            .map(|skill| {
                let description = skill
                    .description
                    .as_deref()
                    .filter(|value| !value.is_empty())
                    .unwrap_or(&skill.name);
                let mut line = format!(
                    "- **{}** — {}  `{}`",
                    skill.name,
                    description,
                    skill.source_path.display()
                );
                if !skill.available {
                    if let Some(summary) = skill.unavailable_summary.as_deref() {
                        line = format!(
                            "- **{}** — {} (unavailable: {})  `{}`",
                            skill.name,
                            description,
                            summary,
                            skill.source_path.display()
                        );
                    } else {
                        line = format!(
                            "- **{}** — {} (unavailable)  `{}`",
                            skill.name,
                            description,
                            skill.source_path.display()
                        );
                    }
                }
                line
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    pub fn build_subagent_prompt(&self) -> String {
        let time_ctx = self.build_runtime_context(None, None, None);
        let workspace = canonical_workspace_path(&self.workspace)
            .display()
            .to_string();
        let skills_summary = self.build_skills_summary(&BTreeSet::new());
        render_agent_template(
            AgentTemplate::SubagentSystem,
            &template_variables(&[
                ("time_ctx", &time_ctx),
                ("workspace", &workspace),
                ("skills_summary", &skills_summary),
            ]),
        )
        .unwrap_or_else(|_| {
            format!("# Subagent\n\n{time_ctx}\n\n## Workspace\n{workspace}\n\n## Skills\n{skills_summary}")
        })
    }

    fn skills_index_summary(&self) -> String {
        let always = self
            .load_skill_documents()
            .into_iter()
            .filter(|skill| skill.always)
            .map(|skill| skill.name)
            .collect::<BTreeSet<_>>();
        self.build_skills_summary(&always)
    }

    fn build_user_content(&self, text: &str, media: &[String]) -> Value {
        let mut blocks = Vec::new();
        for path in media {
            let requested_path = PathBuf::from(path);
            let Ok(path) = self.resolve_workspace_media_path(&requested_path) else {
                continue;
            };
            let Ok(raw) = fs::read(&path) else {
                continue;
            };
            let Some(mime) = detect_image_mime(&raw).or_else(|| image_mime_from_extension(&path))
            else {
                continue;
            };
            blocks.push(json!({
                "type": "image_url",
                "image_url": {"url": format!("data:{mime};base64,{}", STANDARD.encode(raw))},
                "_meta": {"path": path.to_string_lossy()},
            }));
        }
        if blocks.is_empty() {
            Value::String(text.to_owned())
        } else {
            blocks.push(json!({"type": "text", "text": text}));
            Value::Array(blocks)
        }
    }

    fn resolve_workspace_media_path(&self, path: &Path) -> io::Result<PathBuf> {
        let workspace = self.workspace.canonicalize()?;
        let candidate = if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.workspace.join(path)
        };
        reject_symlink_components(&self.workspace, &candidate)?;
        let metadata = fs::symlink_metadata(&candidate)?;
        if metadata.file_type().is_symlink()
            || !metadata.is_file()
            || metadata.len() > MAX_MEDIA_BYTES
        {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "media path is not an allowed regular workspace file",
            ));
        }
        let canonical = candidate.canonicalize()?;
        if !canonical.starts_with(&workspace) {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "media path escapes workspace",
            ));
        }
        Ok(canonical)
    }
}

#[derive(Debug, Clone)]
struct SkillDocument {
    name: String,
    description: Option<String>,
    unavailable_summary: Option<String>,
    source_path: PathBuf,
    always: bool,
    disabled: bool,
    available: bool,
    raw: String,
    body: String,
}

impl SkillDocument {
    fn from_path(path: &Path, configured_env: &BTreeMap<String, String>) -> Option<Self> {
        let raw = fs::read_to_string(path).ok()?;
        Self::from_raw(path.to_path_buf(), raw, configured_env)
    }

    fn from_raw(
        source_path: PathBuf,
        raw: String,
        configured_env: &BTreeMap<String, String>,
    ) -> Option<Self> {
        let (frontmatter, body) = split_frontmatter(&raw);
        let metadata = parse_frontmatter(frontmatter.unwrap_or_default());
        let fallback_name = skill_name_from_path(&source_path);
        let name = fallback_name;
        let requirement = SkillRequirement::from_metadata(&metadata);
        let unavailable_summary = requirement.unavailable_summary(configured_env);
        let body = body.trim().to_owned();
        Some(Self {
            name,
            description: metadata.get("description").cloned(),
            unavailable_summary: unavailable_summary.clone(),
            source_path,
            always: parse_bool(metadata.get("always"))
                || parse_bool(metadata.get("metadata.nanobot.always"))
                || parse_bool(metadata.get("metadata.openclaw.always")),
            disabled: parse_bool(metadata.get("disabled")),
            available: unavailable_summary.is_none(),
            raw,
            body,
        })
    }
}

#[derive(Debug, Default, Clone)]
struct SkillRequirement {
    bins: Vec<String>,
    env: Vec<String>,
}

impl SkillRequirement {
    fn from_metadata(metadata: &BTreeMap<String, String>) -> Self {
        Self {
            bins: unique_list([
                parse_list(metadata.get("requires.bins")),
                parse_list(metadata.get("metadata.nanobot.requires.bins")),
                parse_list(metadata.get("metadata.openclaw.requires.bins")),
                parse_inline_metadata_requires(metadata.get("metadata"), "nanobot", "bins"),
                parse_inline_metadata_requires(metadata.get("metadata"), "openclaw", "bins"),
            ]),
            env: unique_list([
                parse_list(metadata.get("requires.env")),
                parse_list(metadata.get("metadata.nanobot.requires.env")),
                parse_list(metadata.get("metadata.openclaw.requires.env")),
                parse_inline_metadata_requires(metadata.get("metadata"), "nanobot", "env"),
                parse_inline_metadata_requires(metadata.get("metadata"), "openclaw", "env"),
            ]),
        }
    }

    fn unavailable_summary(&self, configured_env: &BTreeMap<String, String>) -> Option<String> {
        let missing_bins = self
            .bins
            .iter()
            .filter(|name| !bin_available(name))
            .cloned()
            .collect::<Vec<_>>();
        let missing_env = self
            .env
            .iter()
            .filter(|name| !env_requirement_available(name, configured_env))
            .cloned()
            .collect::<Vec<_>>();
        let mut parts = Vec::new();
        if !missing_bins.is_empty() {
            parts.push(format!("missing bins: {}", missing_bins.join(", ")));
        }
        if !missing_env.is_empty() {
            parts.push(format!("missing env: {}", missing_env.join(", ")));
        }
        (!parts.is_empty()).then(|| parts.join("; "))
    }
}

fn env_requirement_available(name: &str, configured_env: &BTreeMap<String, String>) -> bool {
    configured_env
        .get(name)
        .map(|value| !value.is_empty())
        .unwrap_or(false)
        || env::var_os(name)
            .map(|value| !value.is_empty())
            .unwrap_or(false)
}

fn merge_runtime_context(runtime_context: String, user_content: Value) -> Value {
    match user_content {
        Value::String(text) => Value::String(format!("{runtime_context}\n\n{text}")),
        Value::Array(mut blocks) => {
            let mut merged = vec![json!({"type": "text", "text": runtime_context})];
            merged.append(&mut blocks);
            Value::Array(merged)
        }
        other => Value::Array(vec![
            json!({"type": "text", "text": runtime_context}),
            other,
        ]),
    }
}

fn merge_message_content(left: Value, right: Value) -> Value {
    match (left, right) {
        (Value::String(left), Value::String(right)) => Value::String(if left.is_empty() {
            right
        } else {
            format!("{left}\n\n{right}")
        }),
        (left, right) => {
            let mut blocks = value_to_blocks(left);
            blocks.extend(value_to_blocks(right));
            Value::Array(blocks)
        }
    }
}

pub fn add_tool_result(
    messages: &mut Vec<Value>,
    tool_call_id: impl Into<String>,
    tool_name: impl Into<String>,
    result: Value,
) {
    messages.push(json!({
        "role": "tool",
        "tool_call_id": tool_call_id.into(),
        "name": tool_name.into(),
        "content": result,
    }));
}

pub fn add_assistant_message(
    messages: &mut Vec<Value>,
    content: Option<String>,
    tool_calls: Option<Vec<Value>>,
    reasoning_content: Option<String>,
    thinking_blocks: Option<Vec<Value>>,
) {
    let mut message = Map::from_iter([
        ("role".to_owned(), Value::String("assistant".to_owned())),
        (
            "content".to_owned(),
            Value::String(content.unwrap_or_default()),
        ),
    ]);
    if let Some(tool_calls) = tool_calls {
        message.insert("tool_calls".to_owned(), Value::Array(tool_calls));
    }
    let has_thinking_blocks = thinking_blocks
        .as_ref()
        .is_some_and(|blocks| !blocks.is_empty());
    if reasoning_content.is_some() || has_thinking_blocks {
        message.insert(
            "reasoning_content".to_owned(),
            Value::String(reasoning_content.unwrap_or_default()),
        );
    }
    if let Some(thinking_blocks) = thinking_blocks {
        message.insert("thinking_blocks".to_owned(), Value::Array(thinking_blocks));
    }
    messages.push(Value::Object(message));
}

fn value_to_blocks(value: Value) -> Vec<Value> {
    match value {
        Value::Array(items) => items
            .into_iter()
            .map(|item| {
                if item.is_object() {
                    item
                } else {
                    json!({"type": "text", "text": value_to_text(item)})
                }
            })
            .collect(),
        Value::Null => Vec::new(),
        Value::String(text) => vec![json!({"type": "text", "text": text})],
        other => vec![json!({"type": "text", "text": other.to_string()})],
    }
}

fn value_to_text(value: Value) -> String {
    match value {
        Value::String(text) => text,
        Value::Null => "None".to_owned(),
        other => other.to_string(),
    }
}

fn detect_image_mime(raw: &[u8]) -> Option<&'static str> {
    if raw.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("image/png")
    } else if raw.starts_with(b"\xff\xd8\xff") {
        Some("image/jpeg")
    } else if raw.starts_with(b"GIF87a") || raw.starts_with(b"GIF89a") {
        Some("image/gif")
    } else if raw.starts_with(b"RIFF") && raw.get(8..12) == Some(b"WEBP") {
        Some("image/webp")
    } else {
        None
    }
}

fn image_mime_from_extension(path: &Path) -> Option<&'static str> {
    match path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(str::to_ascii_lowercase)
    {
        Some(ext) if ext == "png" => Some("image/png"),
        Some(ext) if ext == "jpg" || ext == "jpeg" => Some("image/jpeg"),
        Some(ext) if ext == "gif" => Some("image/gif"),
        Some(ext) if ext == "webp" => Some("image/webp"),
        Some(ext) if ext == "bmp" => Some("image/bmp"),
        Some(ext) if ext == "svg" => Some("image/svg+xml"),
        _ => None,
    }
}

fn reject_symlink_components(workspace: &Path, candidate: &Path) -> io::Result<()> {
    let relative = candidate.strip_prefix(workspace).map_err(|_| {
        io::Error::new(io::ErrorKind::PermissionDenied, "path is outside workspace")
    })?;
    let mut current = workspace.to_path_buf();
    for component in relative.components() {
        current.push(component.as_os_str());
        if let Ok(metadata) = fs::symlink_metadata(&current) {
            if metadata.file_type().is_symlink() {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "symlink components are not allowed",
                ));
            }
        }
    }
    Ok(())
}

fn platform_template_system() -> &'static str {
    if cfg!(target_os = "windows") {
        "Windows"
    } else {
        "POSIX"
    }
}

fn platform_policy_fallback() -> String {
    if cfg!(target_os = "windows") {
        "## Platform Policy (Windows)\n- You are running on Windows.".to_owned()
    } else {
        "## Platform Policy (POSIX)\n- You are running on a POSIX system.".to_owned()
    }
}

fn current_time(timezone: &Option<String>) -> String {
    if let Some(timezone) = timezone {
        if let Ok(timezone) = timezone.parse::<chrono_tz::Tz>() {
            let now = Utc::now().with_timezone(&timezone);
            return format_nanobot_time(NanobotTimeParts {
                year: now.year(),
                month: now.month(),
                day: now.day(),
                hour: now.hour(),
                minute: now.minute(),
                weekday: now.format("%A").to_string(),
                timezone: timezone.name().to_owned(),
                offset_seconds: now.offset().fix().local_minus_utc(),
            });
        }
    }
    let now = Local::now();
    let timezone_name = timezone
        .as_deref()
        .filter(|value| !value.is_empty())
        .unwrap_or("Local");
    format_nanobot_time(NanobotTimeParts {
        year: now.year(),
        month: now.month(),
        day: now.day(),
        hour: now.hour(),
        minute: now.minute(),
        weekday: now.format("%A").to_string(),
        timezone: timezone_name.to_owned(),
        offset_seconds: now.offset().fix().local_minus_utc(),
    })
}

use chrono::Timelike;

struct NanobotTimeParts {
    year: i32,
    month: u32,
    day: u32,
    hour: u32,
    minute: u32,
    weekday: String,
    timezone: String,
    offset_seconds: i32,
}

fn format_nanobot_time(parts: NanobotTimeParts) -> String {
    let sign = if parts.offset_seconds < 0 { '-' } else { '+' };
    let offset_seconds = parts.offset_seconds.abs();
    let offset_hours = offset_seconds / 3600;
    let offset_minutes = (offset_seconds % 3600) / 60;
    let NanobotTimeParts {
        year,
        month,
        day,
        hour,
        minute,
        weekday,
        timezone,
        ..
    } = parts;
    format!(
        "{year:04}-{month:02}-{day:02} {hour:02}:{minute:02} ({weekday}) ({timezone}, UTC{sign}{offset_hours:02}:{offset_minutes:02})"
    )
}

fn split_frontmatter(raw: &str) -> (Option<&str>, &str) {
    let Some(rest) = raw.strip_prefix("---") else {
        return (None, raw);
    };
    let rest = rest.strip_prefix('\n').unwrap_or(rest);
    if let Some(index) = rest.find("\n---") {
        let frontmatter = &rest[..index];
        let body = rest[index + "\n---".len()..]
            .strip_prefix('\n')
            .unwrap_or(&rest[index + "\n---".len()..]);
        (Some(frontmatter), body)
    } else {
        (None, raw)
    }
}

fn strip_skill_frontmatter(raw: &str) -> &str {
    split_frontmatter(raw).1
}

fn skill_name_from_path(path: &Path) -> String {
    if path.file_name().and_then(|value| value.to_str()) == Some("SKILL.md") {
        return path
            .parent()
            .and_then(Path::file_name)
            .and_then(|value| value.to_str())
            .unwrap_or("skill")
            .to_owned();
    }
    path.file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("skill")
        .to_owned()
}

fn parse_frontmatter(frontmatter: &str) -> BTreeMap<String, String> {
    let mut map = BTreeMap::<String, String>::new();
    let mut stack = Vec::<(usize, String)>::new();
    for line in frontmatter.lines() {
        if line.trim().is_empty() || line.trim_start().starts_with('#') {
            continue;
        }
        let indent = line
            .chars()
            .take_while(|character| character.is_whitespace())
            .count();
        while stack.last().is_some_and(|(level, _)| *level >= indent) {
            stack.pop();
        }
        let trimmed = line.trim();
        if let Some(item) = trimmed.strip_prefix("- ") {
            let full_key = stack
                .iter()
                .map(|(_, key)| key.as_str())
                .collect::<Vec<_>>()
                .join(".");
            if !full_key.is_empty() {
                let item = normalize_frontmatter_value(item);
                map.entry(full_key)
                    .and_modify(|value| {
                        value.push(',');
                        value.push_str(&item);
                    })
                    .or_insert(item);
            }
            continue;
        }
        let Some((key, value)) = trimmed.split_once(':') else {
            continue;
        };
        let key = key.trim();
        let full_key = stack
            .iter()
            .map(|(_, key)| key.as_str())
            .chain(std::iter::once(key))
            .collect::<Vec<_>>()
            .join(".");
        let value = normalize_frontmatter_value(value);
        if value.is_empty() {
            stack.push((indent, key.to_owned()));
        } else {
            map.insert(full_key, value);
        }
    }
    map
}

fn normalize_frontmatter_value(value: &str) -> String {
    value.trim().trim_matches('"').trim_matches('\'').to_owned()
}

fn parse_list(value: Option<&String>) -> Vec<String> {
    let Some(value) = value else {
        return Vec::new();
    };
    value
        .trim()
        .trim_start_matches('[')
        .trim_end_matches(']')
        .split(',')
        .map(|item| item.trim().trim_matches('"').trim_matches('\''))
        .filter(|item| !item.is_empty())
        .map(str::to_owned)
        .collect()
}

fn parse_inline_metadata_requires(
    value: Option<&String>,
    namespace: &str,
    requirement: &str,
) -> Vec<String> {
    let Some(value) = value else {
        return Vec::new();
    };
    let Ok(metadata) = serde_json::from_str::<Value>(value) else {
        return Vec::new();
    };
    metadata
        .get(namespace)
        .and_then(|value| value.get("requires"))
        .and_then(|value| value.get(requirement))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .filter(|item| !item.is_empty())
        .map(str::to_owned)
        .collect()
}

fn unique_list(lists: impl IntoIterator<Item = Vec<String>>) -> Vec<String> {
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for item in lists.into_iter().flatten() {
        if seen.insert(item.clone()) {
            out.push(item);
        }
    }
    out
}

fn bin_available(name: &str) -> bool {
    let Some(paths) = env::var_os("PATH") else {
        return false;
    };
    env::split_paths(&paths).any(|path| is_executable_file(&path.join(name)))
}

fn is_executable_file(path: &Path) -> bool {
    let Ok(metadata) = fs::metadata(path) else {
        return false;
    };
    if !metadata.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

fn canonical_workspace_path(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

fn parse_bool(value: Option<&String>) -> bool {
    value
        .map(|value| {
            matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "true" | "yes" | "1"
            )
        })
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::error::Error;

    #[test]
    fn configured_env_satisfies_skill_requires_env() -> Result<(), Box<dyn Error>> {
        let workspace = tempfile::tempdir()?;
        let skill_dir = workspace.path().join("skills").join("configured-env");
        fs::create_dir_all(&skill_dir)?;
        fs::write(
            skill_dir.join("SKILL.md"),
            "---\ndescription: Configured env skill\nrequires.env: SHACS_CORE_TEST_CONFIGURED_ENV_ONLY\n---\nUse configured env.\n",
        )?;

        let unavailable =
            ContextBuilder::new(workspace.path()).build_skills_summary(&BTreeSet::new());
        if !unavailable.contains("unavailable: missing env: SHACS_CORE_TEST_CONFIGURED_ENV_ONLY") {
            return Err(format!(
                "skill unexpectedly available without configured env: {unavailable}"
            )
            .into());
        }

        let available = ContextBuilder::new(workspace.path())
            .with_configured_env(BTreeMap::from([(
                "SHACS_CORE_TEST_CONFIGURED_ENV_ONLY".to_owned(),
                "configured".to_owned(),
            )]))
            .build_skills_summary(&BTreeSet::new());
        let configured_line = available
            .lines()
            .find(|line| line.contains("configured-env"))
            .unwrap_or_default();
        if configured_line.is_empty() || configured_line.contains("unavailable") {
            return Err(
                format!("configured env did not satisfy skill requirement: {available}").into(),
            );
        }
        Ok(())
    }
}
