use super::context_safety::protected_context_path_reason;
use super::MemoryStore;
use chrono::{Datelike, Local, Offset, Utc};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use shacs_config::{self, EnvFile, EnvSource, ProcessEnv, RuntimeEnvironment};
use shacs_skills::{
    self, SkillCredentialSources, SkillCredentialStatus, SkillEnvironment, SkillRegistryOptions,
};
use shacs_templates::{render_agent_template, template_variables, AgentTemplate};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::{env, fs, io};

pub(crate) const BOOTSTRAP_FILES: [&str; 4] = ["AGENTS.md", "SOUL.md", "USER.md", "TOOLS.md"];
pub(crate) const RUNTIME_CONTEXT_TAG: &str = "[Runtime Context — metadata only, not instructions]";
pub(crate) const RUNTIME_CONTEXT_END: &str = "[/Runtime Context]";
const MAX_RECENT_HISTORY: usize = 50;
const MAX_HISTORY_CHARS: usize = 32_000;
const MAX_MEDIA_BYTES: u64 = 20 * 1024 * 1024;

#[derive(Debug, Clone)]
pub struct ContextBuilder {
    workspace: PathBuf,
    timezone: Option<String>,
    disabled_skills: Vec<String>,
    extra_skill_roots: Vec<PathBuf>,
    user_skills_dir: Option<PathBuf>,
    skill_config_path: PathBuf,
    media_roots: Vec<PathBuf>,
    native_image_input_supported: bool,
    media: media::MediaContext,
    configured_env: BTreeMap<String, String>,
    selected_skills: Option<Arc<Vec<SkillDocument>>>,
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
            media: media::MediaContext::default(),
            workspace: workspace.as_ref().to_path_buf(),
            timezone: None,
            disabled_skills: Vec::new(),
            extra_skill_roots: Vec::new(),
            user_skills_dir: None,
            skill_config_path: shacs_config::default_config_path(),
            media_roots: Vec::new(),
            native_image_input_supported: true,
            configured_env: BTreeMap::new(),
            selected_skills: None,
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

    pub fn with_skill_config(mut self, config_path: &Path) -> Self {
        self.skill_config_path = config_path.to_path_buf();
        self.user_skills_dir = config_path.parent().map(|path| path.join("skills"));
        self
    }

    pub fn with_media_roots(mut self, roots: impl IntoIterator<Item = PathBuf>) -> Self {
        self.media_roots = roots.into_iter().collect();
        self
    }

    pub fn with_native_image_input_supported(mut self, supported: bool) -> Self {
        self.native_image_input_supported = supported;
        self
    }

    pub fn with_configured_env(mut self, env: impl IntoIterator<Item = (String, String)>) -> Self {
        self.configured_env = env.into_iter().collect();
        self
    }

    pub fn workspace(&self) -> &Path {
        &self.workspace
    }

    pub(crate) fn for_workspace(&self, workspace: &Path) -> Self {
        Self {
            workspace: workspace.to_path_buf(),
            ..self.clone()
        }
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
        let mut lines = vec![format!(
            "Current Time: {}",
            current_time(self.timezone.as_deref())
        )];
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
        self.bootstrap_files()
            .into_iter()
            .map(|(filename, content)| format!("## {filename}\n\n{content}"))
            .collect::<Vec<_>>()
            .join("\n\n")
    }

    pub(crate) fn bootstrap_files(&self) -> Vec<(String, String)> {
        let workspace = canonical_workspace_path(&self.workspace);
        BOOTSTRAP_FILES
            .iter()
            .filter_map(|filename| {
                let path = self.workspace.join(filename);
                let content = read_bootstrap_file(&workspace, &path)?;
                Some(((*filename).to_owned(), content))
            })
            .collect()
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

    fn load_skill_documents(&self) -> Vec<SkillDocument> {
        if let Some(selected) = &self.selected_skills {
            return selected.as_ref().clone();
        }
        self.resolve_skill_documents(&ProcessEnv, &EnvFile::load(&self.skill_config_path))
            .0
    }

    /// Freezes sources, bodies and readiness once, before any tools or children run.
    pub fn select_skills(mut self, environment: &RuntimeEnvironment) -> (Self, SkillEnvironment) {
        let (documents, values) =
            self.resolve_skill_documents(environment, &Ok(environment.file.clone()));
        self.selected_skills = Some(Arc::new(documents));
        (self, SkillEnvironment::new(values))
    }

    fn resolve_skill_documents(
        &self,
        process: &impl EnvSource,
        file: &Result<EnvFile, shacs_config::ConfigError>,
    ) -> (Vec<SkillDocument>, BTreeMap<String, String>) {
        let mut options = SkillRegistryOptions::new(&self.workspace);
        options.user_skills_dir = self.user_skills_dir.clone();
        options.plugin_roots = self.extra_skill_roots.clone();
        options.plugin_roots_enabled = true;
        let Ok(registry) = shacs_skills::discover_skill_registry(options) else {
            return (Vec::new(), BTreeMap::new());
        };
        let env_path = shacs_config::env_file_path(&self.skill_config_path);
        let mut values = BTreeMap::new();
        let documents = registry
            .active_entries()
            .into_iter()
            .filter(|entry| entry.inactive_reason(&self.disabled_skills).is_none())
            .filter_map(|entry| {
                let raw = entry.raw.as_ref()?.clone();
                let descriptor = &entry.descriptor;
                let status = match file {
                    Ok(file) => {
                        let resolution = shacs_skills::resolve_skill_credentials(
                            entry,
                            &self.disabled_skills,
                            SkillCredentialSources {
                                process,
                                file,
                                explicit: &self.configured_env,
                            },
                        );
                        values.extend(resolution.values().clone());
                        resolution.status
                    }
                    Err(error) => SkillCredentialStatus {
                        required: descriptor.credentials.required.clone(),
                        reserved: descriptor.credentials.reserved.clone(),
                        diagnostics: descriptor
                            .credentials
                            .diagnostics
                            .iter()
                            .cloned()
                            .chain([error.to_string()])
                            .collect(),
                        ..SkillCredentialStatus::default()
                    },
                };
                let mut missing_bins: Vec<String> = descriptor
                    .requirements
                    .iter()
                    .filter_map(|requirement| requirement.split_once(": "))
                    .filter(|(key, _)| key.ends_with(".bins"))
                    .flat_map(|(_, value)| value.split(','))
                    .map(|name| name.trim().trim_matches(['[', ']', '\'', '"']).trim())
                    .filter(|name| !name.is_empty() && !bin_available(name))
                    .map(str::to_owned)
                    .collect();
                let mut seen_bins = BTreeSet::new();
                missing_bins.retain(|name| seen_bins.insert(name.clone()));
                let unavailable_summary = (!missing_bins.is_empty())
                    .then(|| format!("missing bins: {}", missing_bins.join(", ")));
                Some(SkillDocument {
                    name: descriptor.name.clone(),
                    description: descriptor.description.clone(),
                    source_path: descriptor.source_path.clone().unwrap_or_else(|| {
                        self.workspace
                            .join("builtin_skills")
                            .join(&descriptor.name)
                            .join("SKILL.md")
                    }),
                    always: descriptor.always,
                    available: unavailable_summary.is_none(),
                    unavailable_summary,
                    credential_guidance: status.guidance(&env_path),
                    body: strip_skill_frontmatter(&raw).trim().to_owned(),
                    raw,
                })
            })
            .collect();
        (documents, values)
    }

    fn load_always_skills(&self) -> Option<String> {
        let content = self
            .load_skill_documents()
            .into_iter()
            .filter(|skill| skill.always && skill.available)
            .map(|skill| {
                let content = format!("### Skill: {}\n\n{}", skill.name, skill.body);
                if skill.credential_guidance.is_empty() {
                    content
                } else {
                    format!("{content}\n\n{}", skill.credential_guidance)
                }
            })
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

    pub fn skill_body_hashes_for_context(&self) -> BTreeMap<String, String> {
        self.load_skill_documents()
            .into_iter()
            .map(|skill| {
                (
                    skill.name,
                    format!("sha256:{:x}", Sha256::digest(skill.raw.as_bytes())),
                )
            })
            .collect()
    }

    pub fn skill_name_for_source_path(&self, path: impl AsRef<Path>) -> Option<String> {
        let path = path.as_ref();
        let candidate = if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.workspace.join(path)
        };
        if !Self::path_within_workspace(&candidate, &self.workspace) {
            return None;
        }
        let candidate_canonical = fs::canonicalize(&candidate).ok();
        let candidate_normalized = Self::normalize_path_lexically(&candidate);
        self.load_skill_documents()
            .into_iter()
            .filter(|skill| skill.available)
            .find_map(|skill| {
                if let (Some(candidate), Ok(source_path)) = (
                    candidate_canonical.as_ref(),
                    fs::canonicalize(&skill.source_path),
                ) {
                    if source_path == *candidate {
                        return Some(skill.name);
                    }
                }
                let source_path = Self::normalize_path_lexically(&skill.source_path);
                (source_path == candidate_normalized).then_some(skill.name)
            })
    }

    fn path_within_workspace(path: &Path, workspace: &Path) -> bool {
        match fs::canonicalize(path) {
            Ok(path) => {
                let workspace = fs::canonicalize(workspace)
                    .unwrap_or_else(|_| Self::normalize_path_lexically(workspace));
                path.starts_with(workspace)
            }
            Err(_) => Self::normalize_path_lexically(path)
                .starts_with(Self::normalize_path_lexically(workspace)),
        }
    }

    fn normalize_path_lexically(path: &Path) -> PathBuf {
        let mut normalized = PathBuf::new();
        for component in path.components() {
            match component {
                std::path::Component::CurDir => {}
                std::path::Component::ParentDir => {
                    normalized.pop();
                }
                component => normalized.push(component.as_os_str()),
            }
        }
        normalized
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
            .map(|skill| {
                if skill.credential_guidance.is_empty() {
                    skill.raw
                } else {
                    format!("{}\n\n{}", skill.raw, skill.credential_guidance)
                }
            })
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
                if !skill.credential_guidance.is_empty() {
                    line.push_str(&format!(
                        "\n  {}",
                        skill.credential_guidance.replace('\n', "\n  ")
                    ));
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
// allow: SIZE_OK — preexisting context facade; Spec034 video state and routing live in a focused child module
mod media;

#[derive(Debug, Clone)]
struct SkillDocument {
    name: String,
    description: Option<String>,
    unavailable_summary: Option<String>,
    source_path: PathBuf,
    always: bool,
    available: bool,
    credential_guidance: String,
    raw: String,
    body: String,
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

pub(super) fn detect_image_mime(raw: &[u8]) -> Option<&'static str> {
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

fn workspace_image_unsupported_note(path: &Path, mime: &str) -> Value {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("image");
    json!({
        "type": "text",
        "text": format!(
            "[attachment:unsupported] name={name} mime={mime} reason=native image input is not supported by provider/model"
        ),
    })
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

fn current_time(timezone: Option<&str>) -> String {
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

fn read_bootstrap_file(workspace: &Path, path: &Path) -> Option<String> {
    let canonical = path.canonicalize().ok()?;
    if !canonical.starts_with(workspace) || protected_context_path_reason(&canonical).is_some() {
        return None;
    }
    let metadata = fs::metadata(&canonical).ok()?;
    if !metadata.is_file() {
        return None;
    }
    fs::read_to_string(canonical).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::error::Error;

    #[cfg(unix)]
    #[test]
    fn bootstrap_files_skip_protected_symlink_targets() -> Result<(), Box<dyn Error>> {
        let workspace = tempfile::tempdir()?;
        fs::write(workspace.path().join(".env"), "BOOTSTRAP_SECRET=raw")?;
        std::os::unix::fs::symlink(
            workspace.path().join(".env"),
            workspace.path().join("AGENTS.md"),
        )?;

        let prompt = ContextBuilder::new(workspace.path()).build_system_prompt(None);

        assert!(!prompt.contains("BOOTSTRAP_SECRET=raw"));
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn bootstrap_files_skip_symlinks_outside_workspace() -> Result<(), Box<dyn Error>> {
        let workspace = tempfile::tempdir()?;
        let outside = tempfile::NamedTempFile::new()?;
        fs::write(outside.path(), "outside-bootstrap-body")?;
        std::os::unix::fs::symlink(outside.path(), workspace.path().join("AGENTS.md"))?;

        let prompt = ContextBuilder::new(workspace.path()).build_system_prompt(None);

        assert!(!prompt.contains("outside-bootstrap-body"));
        Ok(())
    }

    #[test]
    fn configured_env_satisfies_skill_requires_env() -> Result<(), Box<dyn Error>> {
        let workspace = tempfile::tempdir()?;
        let skill_dir = workspace.path().join("skills").join("configured-env");
        fs::create_dir_all(&skill_dir)?;
        fs::write(
            skill_dir.join("SKILL.md"),
            "---\ndescription: Configured env skill\nalways: true\nrequires.env: SHACS_CORE_TEST_CONFIGURED_ENV_ONLY\n---\nUse configured env.\n",
        )?;

        let builder = ContextBuilder::new(workspace.path())
            .with_skill_config(&workspace.path().join("config.json"));
        let missing = builder.build_skills_summary(&BTreeSet::new());
        assert!(
            missing.contains("missing env: SHACS_CORE_TEST_CONFIGURED_ENV_ONLY"),
            "missing names stay visible"
        );
        assert!(
            !missing.contains("unavailable: missing env"),
            "credentials do not disable skills"
        );
        assert!(
            builder
                .active_always_skill_names()
                .contains(&"configured-env".to_owned()),
            "missing credentials do not block always injection"
        );
        let prompt = builder.build_system_prompt(None);
        assert!(
            prompt.contains("Use configured env."),
            "always body is injected"
        );
        assert!(
            prompt.contains(&workspace.path().join(".env.json").display().to_string()),
            "guidance uses the actual data path"
        );
        assert!(
            builder.load_skill("configured-env").is_some_and(
                |body| body.contains("missing env: SHACS_CORE_TEST_CONFIGURED_ENV_ONLY")
            ),
            "explicit loading keeps readiness guidance"
        );

        let available = builder
            .with_configured_env(BTreeMap::from([(
                "SHACS_CORE_TEST_CONFIGURED_ENV_ONLY".to_owned(),
                "configured-secret-canary".to_owned(),
            )]))
            .build_skills_summary(&BTreeSet::new());
        assert!(
            !available.contains("missing env: SHACS_CORE_TEST_CONFIGURED_ENV_ONLY"),
            "configured values satisfy readiness"
        );
        assert!(
            !available.contains("configured-secret-canary"),
            "summary never includes values"
        );
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

    #[test]
    fn unreadable_skill_keeps_good_body_always_injection_and_builtins() -> Result<(), Box<dyn Error>>
    {
        use shacs_skills::SkillRegistryStatus;

        let workspace = tempfile::tempdir()?;
        let good = workspace.path().join("skills/good");
        fs::create_dir_all(&good)?;
        fs::write(
            good.join("SKILL.md"),
            "---\nname: good\nalways: true\n---\nGood skill body.",
        )?;
        fs::write(
            workspace.path().join("skills/bad.md"),
            b"private-canary\xff",
        )?;
        let context = ContextBuilder::new(workspace.path())
            .with_skill_config(&workspace.path().join("config.json"));

        assert!(
            context
                .load_skill("good")
                .is_some_and(|raw| raw.contains("Good skill body.")),
            "one unreadable file must not hide a good skill body"
        );
        assert!(
            context
                .load_always_skills()
                .is_some_and(|body| body.contains("Good skill body.")),
            "one unreadable file must not prevent always injection"
        );
        assert!(
            context
                .build_skills_summary(&BTreeSet::new())
                .contains("**skill-creator**"),
            "virtual builtins must remain listed"
        );
        let registry =
            shacs_skills::discover_skill_registry(SkillRegistryOptions::new(workspace.path()))?;
        let bad = registry
            .find("bad")
            .ok_or("missing malformed skill diagnostic")?;
        assert_eq!(bad.status, SkillRegistryStatus::Malformed);
        assert!(
            bad.raw.is_none(),
            "unreadable input must not enter the registry"
        );
        assert!(
            !bad.diagnostics.is_empty() && !bad.diagnostics.join(" ").contains("private-canary"),
            "per-file diagnostics must explain failure without copying input"
        );
        Ok(())
    }

    #[test]
    fn registry_disabled_winners_and_conflicts_do_not_resurrect_lower_sources(
    ) -> Result<(), Box<dyn Error>> {
        let root = tempfile::tempdir()?;
        let workspace = root.path().join("workspace");
        for (directory, name, fields) in [
            (
                root.path().join("skills/lower"),
                "disabled-winner",
                "requires.env: LOWER_KEY",
            ),
            (
                workspace.join("skills/winner"),
                "disabled-winner",
                "disabled: true\nrequires.env: WINNER_KEY",
            ),
            (
                workspace.join("skills/conflict-a"),
                "conflict",
                "requires.env: FIRST_KEY",
            ),
            (
                workspace.join(".shacs-bot/skills/conflict-b"),
                "conflict",
                "requires.env: SECOND_KEY",
            ),
            (
                workspace.join("skills/config-disabled"),
                "config-disabled",
                "requires.env: DISABLED_KEY",
            ),
            (
                workspace.join("builtin_skills/teams-meeting-pipeline"),
                "teams-meeting-pipeline",
                "requires.env: DEFERRED_KEY",
            ),
        ] {
            fs::create_dir_all(&directory)?;
            fs::write(
                directory.join("SKILL.md"),
                format!("---\nname: {name}\n{fields}\n---\n{name} body"),
            )?;
        }
        let builder = ContextBuilder::new(&workspace)
            .with_skill_config(&root.path().join("config.json"))
            .with_disabled_skills(["config-disabled".to_owned()]);
        for name in [
            "disabled-winner",
            "conflict",
            "config-disabled",
            "teams-meeting-pipeline",
        ] {
            assert!(builder.load_skill(name).is_none(), "{name} must not load");
            assert!(
                !builder.skill_body_hashes_for_context().contains_key(name),
                "{name} must not enter context"
            );
        }
        Ok(())
    }
    #[test]
    fn selected_skills_freeze_bodies_and_values_until_the_next_run() -> Result<(), Box<dyn Error>> {
        let root = tempfile::tempdir()?;
        let config_path = root.path().join("config.json");
        let skill_dir = root.path().join("skills/frozen");
        fs::create_dir_all(&skill_dir)?;
        let path = skill_dir.join("SKILL.md");
        fs::write(
            &path,
            "---\nname: frozen\nrequires.env: OLD_KEY\n---\nOriginal body",
        )?;
        fs::write(
            root.path().join(".env.json"),
            r#"{"OLD_KEY":"startup-canary","NEW_KEY":"next-key-canary"}"#,
        )?;
        let mut config = shacs_config::Config::default();
        let startup = RuntimeEnvironment::resolve(&mut config, &config_path, &BTreeMap::new())?;
        let builder = ContextBuilder::new(root.path()).with_skill_config(&config_path);
        let (selected, values) = builder.clone().select_skills(&startup);

        fs::write(
            &path,
            "---\nname: frozen\nrequires.env: NEW_KEY\n---\nChanged body",
        )?;
        fs::write(
            root.path().join(".env.json"),
            r#"{"NEW_KEY":"rotated-canary"}"#,
        )?;
        let child = selected.for_workspace(&root.path().join("child"));
        assert!(
            child
                .load_skill("frozen")
                .is_some_and(|body| body.contains("Original body")),
            "children keep the parent's selected source and body"
        );
        assert_eq!(
            values.values(),
            &BTreeMap::from([("OLD_KEY".to_owned(), "startup-canary".to_owned())])
        );
        let (_, next_values) = builder.clone().select_skills(&startup);
        assert_eq!(
            next_values.values(),
            &BTreeMap::from([("NEW_KEY".to_owned(), "next-key-canary".to_owned())])
        );
        let restarted = RuntimeEnvironment::resolve(&mut config, &config_path, &BTreeMap::new())?;
        let (_, restarted_values) = builder.clone().select_skills(&restarted);
        assert_eq!(
            restarted_values.values().get("NEW_KEY").map(String::as_str),
            Some("rotated-canary")
        );
        fs::remove_file(root.path().join(".env.json"))?;
        let deleted = RuntimeEnvironment::resolve(&mut config, &config_path, &BTreeMap::new())?;
        let (missing, empty) = builder.select_skills(&deleted);
        assert!(
            empty.values().is_empty(),
            "a restarted runtime observes deletion"
        );
        assert!(missing
            .build_skills_summary(&BTreeSet::new())
            .contains("missing env: NEW_KEY"));
        assert!(
            !format!("{startup:?} {values:?}").contains("startup-canary"),
            "new debug surfaces contain names only"
        );
        Ok(())
    }
}
