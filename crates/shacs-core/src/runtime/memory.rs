use crate::runtime::runner::{AgentRunSpec, AgentRunner, ToolStatus};
use crate::tools::{
    EditFileTool, FileState, PathContext, ReadFileTool, ToolRegistry, WriteFileTool,
};
use chrono::Local;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use shacs_providers::{
    chat_with_retry, GenerationSettings, ProviderClient, ProviderRequest, ProviderRetryMode,
};
use shacs_session::{Session, SessionManager};
use shacs_templates::{
    render_agent_template, render_workspace_template, template_variables, AgentTemplate,
    WorkspaceTemplate,
};
use shacs_utils::gitstore::{GitCliStore, GitStore};
use shacs_utils::text::{strip_think, truncate_text};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

pub const DEFAULT_MAX_HISTORY_ENTRIES: usize = 1000;
pub const HISTORY_ENTRY_HARD_CAP: usize = 64_000;
pub const RAW_ARCHIVE_MAX_CHARS: usize = 16_000;
pub const ARCHIVE_SUMMARY_MAX_CHARS: usize = 8_000;
pub const DEFAULT_CONSOLIDATION_SAFETY_BUFFER: usize = 2_000;
pub const DEFAULT_MAX_CONSOLIDATION_ROUNDS: usize = 5;
pub const DREAM_STALE_THRESHOLD_DAYS: u64 = 14;
pub const DREAM_MEMORY_FILE_MAX_CHARS: usize = 32_000;
pub const DREAM_SOUL_FILE_MAX_CHARS: usize = 16_000;
pub const DREAM_USER_FILE_MAX_CHARS: usize = 16_000;
pub const DREAM_HISTORY_ENTRY_PREVIEW_MAX_CHARS: usize = 4_000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryHistoryEntry {
    pub cursor: u64,
    pub timestamp: String,
    pub content: String,
}

#[derive(Debug, Clone)]
pub struct MemoryStore {
    workspace: PathBuf,
    memory_dir: PathBuf,
    max_history_entries: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryConsolidationRequest {
    pub existing_memory: String,
    pub history: Vec<MemoryHistoryEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryConsolidationOutcome {
    pub processed_entries: usize,
    pub processed_cursor: u64,
    pub memory_updated: bool,
}

#[derive(Debug)]
pub enum MemoryConsolidationError {
    Io(std::io::Error),
    Provider(String),
}

impl std::fmt::Display for MemoryConsolidationError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "memory persistence failed: {error}"),
            Self::Provider(error) => write!(formatter, "memory provider failed: {error}"),
        }
    }
}

impl std::error::Error for MemoryConsolidationError {}

impl From<std::io::Error> for MemoryConsolidationError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

pub trait MemoryConsolidator {
    fn consolidate(
        &self,
        request: MemoryConsolidationRequest,
    ) -> Result<Option<String>, MemoryConsolidationError>;
}

#[derive(Clone)]
pub struct ProviderMemoryConsolidator<'a> {
    client: &'a dyn ProviderClient,
    model: String,
    settings: GenerationSettings,
    retry_mode: ProviderRetryMode,
}

impl<'a> ProviderMemoryConsolidator<'a> {
    pub fn new(client: &'a dyn ProviderClient, model: impl Into<String>) -> Self {
        Self {
            client,
            model: model.into(),
            settings: GenerationSettings::default(),
            retry_mode: ProviderRetryMode::Standard,
        }
    }

    pub fn with_settings(mut self, settings: GenerationSettings) -> Self {
        self.settings = settings;
        self
    }

    pub fn with_retry_mode(mut self, retry_mode: ProviderRetryMode) -> Self {
        self.retry_mode = retry_mode;
        self
    }
}

impl MemoryConsolidator for ProviderMemoryConsolidator<'_> {
    fn consolidate(
        &self,
        request: MemoryConsolidationRequest,
    ) -> Result<Option<String>, MemoryConsolidationError> {
        if request.history.is_empty() {
            return Ok(None);
        }
        let provider_request = ProviderRequest {
            messages: build_memory_update_messages(&request),
            tools: Vec::new(),
            model: self.model.clone(),
            settings: self.settings.clone(),
            tool_choice: None,
        };
        let response = chat_with_retry(self.client, provider_request, self.retry_mode)
            .map_err(|error| MemoryConsolidationError::Provider(error.to_string()))?;
        if response.finish_reason == "error" {
            return Err(MemoryConsolidationError::Provider(
                response
                    .content
                    .unwrap_or_else(|| "provider returned error response".to_owned()),
            ));
        }
        Ok(response
            .content
            .map(|content| content.trim().to_owned())
            .filter(|content| !content.is_empty()))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryArchiveOutcome {
    pub archived_messages: usize,
    pub summary: Option<String>,
    pub raw_fallback: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TokenConsolidationConfig {
    pub context_window_tokens: usize,
    pub max_completion_tokens: usize,
    pub safety_buffer: usize,
    pub consolidation_ratio: f32,
    pub max_rounds: usize,
}

impl TokenConsolidationConfig {
    pub fn new(context_window_tokens: usize, max_completion_tokens: usize) -> Self {
        Self {
            context_window_tokens,
            max_completion_tokens,
            safety_buffer: DEFAULT_CONSOLIDATION_SAFETY_BUFFER,
            consolidation_ratio: 0.6,
            max_rounds: DEFAULT_MAX_CONSOLIDATION_ROUNDS,
        }
    }

    pub fn input_token_budget(&self) -> usize {
        self.context_window_tokens
            .saturating_sub(self.max_completion_tokens)
            .saturating_sub(self.safety_buffer)
    }

    fn target_tokens(&self) -> usize {
        let budget = self.input_token_budget();
        ((budget as f32) * self.consolidation_ratio.clamp(0.0, 1.0)).floor() as usize
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionTokenConsolidationOutcome {
    pub rounds: usize,
    pub archived_messages: usize,
    pub last_consolidated: usize,
    pub estimated_tokens: usize,
    pub budget: usize,
    pub raw_fallback: bool,
    pub summary_stored: bool,
}

#[derive(Debug, Default, Clone)]
pub struct SessionConsolidationLocks {
    active: BTreeSet<String>,
}

impl SessionConsolidationLocks {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn try_acquire(&mut self, key: &str) -> bool {
        self.active.insert(key.to_owned())
    }

    pub fn release(&mut self, key: &str) {
        self.active.remove(key);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MemoryLineAge {
    pub age_days: u64,
}

pub trait MemoryGitBoundary {
    fn line_ages(&self, _relative_path: &str) -> Result<Option<Vec<MemoryLineAge>>, String> {
        Ok(None)
    }

    fn is_initialized(&self) -> bool {
        false
    }

    fn auto_commit(&self, _message: &str) -> Result<Option<String>, String> {
        Ok(None)
    }
}

#[derive(Debug, Default, Clone, Copy)]
pub struct NoGitBoundary;

impl MemoryGitBoundary for NoGitBoundary {}

impl MemoryGitBoundary for GitCliStore {
    fn line_ages(&self, relative_path: &str) -> Result<Option<Vec<MemoryLineAge>>, String> {
        GitStore::line_ages(self, relative_path).map(|ages| {
            ages.map(|ages| {
                ages.into_iter()
                    .map(|age| MemoryLineAge {
                        age_days: age.age_days,
                    })
                    .collect()
            })
        })
    }

    fn is_initialized(&self) -> bool {
        GitStore::is_initialized(self)
    }

    fn auto_commit(&self, message: &str) -> Result<Option<String>, String> {
        GitStore::auto_commit(self, message)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DreamRunOutcome {
    pub worked: bool,
    pub processed_entries: usize,
    pub processed_cursor: u64,
    pub phase1_completed: bool,
    pub phase2_completed: bool,
    pub cursor_advanced: bool,
    pub changelog: Vec<String>,
    pub commit: Option<String>,
}

impl DreamRunOutcome {
    fn idle(cursor: u64) -> Self {
        Self {
            worked: false,
            processed_entries: 0,
            processed_cursor: cursor,
            phase1_completed: false,
            phase2_completed: false,
            cursor_advanced: false,
            changelog: Vec::new(),
            commit: None,
        }
    }
}

pub struct DreamProcessor<'a> {
    store: MemoryStore,
    client: &'a dyn ProviderClient,
    model: String,
    settings: GenerationSettings,
    retry_mode: ProviderRetryMode,
    max_batch_size: usize,
    max_iterations: usize,
    max_tool_result_chars: usize,
    annotate_line_ages: bool,
    git: Option<&'a dyn MemoryGitBoundary>,
}

impl<'a> DreamProcessor<'a> {
    pub fn new(
        store: MemoryStore,
        client: &'a dyn ProviderClient,
        model: impl Into<String>,
    ) -> Self {
        Self {
            store,
            client,
            model: model.into(),
            settings: GenerationSettings::default(),
            retry_mode: ProviderRetryMode::Standard,
            max_batch_size: 20,
            max_iterations: 10,
            max_tool_result_chars: 16_000,
            annotate_line_ages: true,
            git: None,
        }
    }

    pub fn with_settings(mut self, settings: GenerationSettings) -> Self {
        self.settings = settings;
        self
    }

    pub fn with_retry_mode(mut self, retry_mode: ProviderRetryMode) -> Self {
        self.retry_mode = retry_mode;
        self
    }

    pub fn with_max_batch_size(mut self, max_batch_size: usize) -> Self {
        self.max_batch_size = max_batch_size.max(1);
        self
    }

    pub fn with_max_iterations(mut self, max_iterations: usize) -> Self {
        self.max_iterations = max_iterations.max(1);
        self
    }

    pub fn with_max_tool_result_chars(mut self, max_tool_result_chars: usize) -> Self {
        self.max_tool_result_chars = max_tool_result_chars;
        self
    }

    pub fn with_line_age_annotation(mut self, enabled: bool) -> Self {
        self.annotate_line_ages = enabled;
        self
    }

    pub fn with_git_boundary(mut self, git: &'a dyn MemoryGitBoundary) -> Self {
        self.git = Some(git);
        self
    }

    pub fn run(&self) -> Result<DreamRunOutcome, MemoryConsolidationError> {
        let last_cursor = self.store.get_last_dream_cursor();
        let entries = self.store.read_unprocessed_history(last_cursor);
        if entries.is_empty() {
            return Ok(DreamRunOutcome::idle(last_cursor));
        }

        let batch_len = entries.len().min(self.max_batch_size.max(1));
        let batch = &entries[..batch_len];
        let processed_cursor = batch
            .last()
            .map(|entry| entry.cursor)
            .unwrap_or(last_cursor);
        let file_context = self.build_file_context();
        let history_text = batch
            .iter()
            .map(|entry| {
                format!(
                    "[{}] {}",
                    entry.timestamp,
                    truncate_text(&entry.content, DREAM_HISTORY_ENTRY_PREVIEW_MAX_CHARS)
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        let phase1_messages = build_dream_phase1_messages(&history_text, &file_context);
        let phase1_request = ProviderRequest {
            messages: phase1_messages,
            tools: Vec::new(),
            model: self.model.clone(),
            settings: self.settings.clone(),
            tool_choice: None,
        };
        let analysis = match chat_with_retry(self.client, phase1_request, self.retry_mode) {
            Ok(response) if response.finish_reason != "error" => {
                response.content.unwrap_or_default()
            }
            Ok(_) | Err(_) => {
                return Ok(DreamRunOutcome {
                    worked: false,
                    processed_entries: batch_len,
                    processed_cursor: last_cursor,
                    phase1_completed: false,
                    phase2_completed: false,
                    cursor_advanced: false,
                    changelog: Vec::new(),
                    commit: None,
                });
            }
        };

        let skills = list_existing_skills(self.store.workspace());
        let phase2_messages = build_dream_phase2_messages(&analysis, &file_context, &skills);
        let tools = self.build_tools()?;
        let mut spec = AgentRunSpec::new(phase2_messages, &tools, self.client, self.model.clone());
        spec.settings = self.settings.clone();
        spec.retry_mode = self.retry_mode;
        spec.max_iterations = self.max_iterations;
        spec.max_tool_result_chars = self.max_tool_result_chars;
        spec.fail_on_tool_error = false;
        spec.workspace = Some(self.store.workspace().to_path_buf());
        let result = AgentRunner::new().run(spec).ok();
        let phase2_completed = result
            .as_ref()
            .is_some_and(|result| result.stop_reason == "completed");
        let changelog = result
            .as_ref()
            .map(|result| {
                result
                    .tool_events
                    .iter()
                    .filter(|event| event.status == ToolStatus::Ok)
                    .map(|event| format!("{}: {}", event.name, event.detail))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        self.store.set_last_dream_cursor(processed_cursor)?;
        self.store.compact_history()?;

        let commit = if !changelog.is_empty() && self.git.is_some_and(|git| git.is_initialized()) {
            let timestamp = batch
                .last()
                .map(|entry| entry.timestamp.as_str())
                .unwrap_or_default();
            let message = format!(
                "dream: {timestamp}, {} change(s)\n\n{}",
                changelog.len(),
                analysis.trim()
            );
            self.git
                .and_then(|git| git.auto_commit(&message).ok().flatten())
        } else {
            None
        };

        Ok(DreamRunOutcome {
            worked: true,
            processed_entries: batch_len,
            processed_cursor,
            phase1_completed: true,
            phase2_completed,
            cursor_advanced: true,
            changelog,
            commit,
        })
    }

    fn build_file_context(&self) -> String {
        let raw_memory = normalize_empty_file(self.store.read_memory());
        let annotated_memory = if self.annotate_line_ages {
            self.git
                .and_then(|git| git.line_ages("memory/MEMORY.md").ok().flatten())
                .map(|ages| annotate_memory_with_ages(&raw_memory, &ages))
                .unwrap_or_else(|| raw_memory.clone())
        } else {
            raw_memory.clone()
        };
        let current_memory = truncate_text(&annotated_memory, DREAM_MEMORY_FILE_MAX_CHARS);
        let current_soul = truncate_text(
            &normalize_empty_file(self.store.read_soul()),
            DREAM_SOUL_FILE_MAX_CHARS,
        );
        let current_user = truncate_text(
            &normalize_empty_file(self.store.read_user()),
            DREAM_USER_FILE_MAX_CHARS,
        );
        format!(
            "## Current Date\n{}\n\n## Current MEMORY.md ({} chars)\n{}\n\n## Current SOUL.md ({} chars)\n{}\n\n## Current USER.md ({} chars)\n{}",
            Local::now().format("%Y-%m-%d"),
            current_memory.chars().count(),
            current_memory,
            current_soul.chars().count(),
            current_soul,
            current_user.chars().count(),
            current_user,
        )
    }

    fn build_tools(&self) -> Result<ToolRegistry, MemoryConsolidationError> {
        let workspace = self.store.workspace().to_path_buf();
        let skills_dir = workspace.join("skills");
        fs::create_dir_all(&skills_dir)?;
        let file_state = Arc::new(Mutex::new(FileState::new()));
        let mut registry = ToolRegistry::new();
        registry.register(ReadFileTool::with_file_state(
            PathContext::workspace(workspace.clone()),
            file_state.clone(),
        ));
        registry.register(EditFileTool::with_file_state(
            PathContext::workspace(workspace.clone()),
            file_state.clone(),
        ));
        registry.register(WriteFileTool::with_file_state(
            PathContext {
                workspace: Some(workspace),
                allowed_dir: Some(skills_dir),
                media_dir: None,
                extra_allowed_dirs: Vec::new(),
            },
            file_state,
        ));
        Ok(registry)
    }
}

#[derive(Clone)]
pub struct ProviderArchiveConsolidator<'a> {
    client: &'a dyn ProviderClient,
    model: String,
    settings: GenerationSettings,
    retry_mode: ProviderRetryMode,
}

impl<'a> ProviderArchiveConsolidator<'a> {
    pub fn new(client: &'a dyn ProviderClient, model: impl Into<String>) -> Self {
        Self {
            client,
            model: model.into(),
            settings: GenerationSettings::default(),
            retry_mode: ProviderRetryMode::Standard,
        }
    }

    pub fn with_settings(mut self, settings: GenerationSettings) -> Self {
        self.settings = settings;
        self
    }

    pub fn with_retry_mode(mut self, retry_mode: ProviderRetryMode) -> Self {
        self.retry_mode = retry_mode;
        self
    }

    pub fn archive(
        &self,
        store: &MemoryStore,
        messages: &[Value],
    ) -> Result<MemoryArchiveOutcome, MemoryConsolidationError> {
        self.archive_with_max_chars(store, messages, RAW_ARCHIVE_MAX_CHARS)
    }

    pub fn archive_with_max_chars(
        &self,
        store: &MemoryStore,
        messages: &[Value],
        max_chars: usize,
    ) -> Result<MemoryArchiveOutcome, MemoryConsolidationError> {
        if messages.is_empty() {
            return Ok(MemoryArchiveOutcome {
                archived_messages: 0,
                summary: None,
                raw_fallback: false,
            });
        }
        let provider_request = ProviderRequest {
            messages: build_archive_messages(messages, max_chars),
            tools: Vec::new(),
            model: self.model.clone(),
            settings: self.settings.clone(),
            tool_choice: None,
        };
        let response = chat_with_retry(self.client, provider_request, self.retry_mode);
        let mut provider_returned_nothing = false;
        let summary = match response {
            Ok(response) if response.finish_reason != "error" => {
                response.content.and_then(|content| {
                    let trimmed = content.trim();
                    if trimmed == "(nothing)" {
                        provider_returned_nothing = true;
                        None
                    } else {
                        let truncated = truncate_text(trimmed, ARCHIVE_SUMMARY_MAX_CHARS);
                        (!truncated.is_empty()).then_some(truncated)
                    }
                })
            }
            Ok(_) | Err(_) => None,
        };
        if let Some(summary) = summary {
            store.append_history(&summary, Some(ARCHIVE_SUMMARY_MAX_CHARS))?;
            Ok(MemoryArchiveOutcome {
                archived_messages: messages.len(),
                summary: Some(summary),
                raw_fallback: false,
            })
        } else if provider_returned_nothing {
            Ok(MemoryArchiveOutcome {
                archived_messages: messages.len(),
                summary: None,
                raw_fallback: false,
            })
        } else {
            store.raw_archive(messages, Some(max_chars))?;
            Ok(MemoryArchiveOutcome {
                archived_messages: messages.len(),
                summary: None,
                raw_fallback: true,
            })
        }
    }

    pub fn maybe_consolidate_session_by_tokens(
        &self,
        store: &MemoryStore,
        sessions: &mut SessionManager,
        session: &mut Session,
        config: &TokenConsolidationConfig,
        tool_definitions: &[Value],
        session_summary: Option<&str>,
    ) -> Result<SessionTokenConsolidationOutcome, MemoryConsolidationError> {
        let budget = config.input_token_budget();
        let mut estimated =
            estimate_session_prompt_tokens(session, session_summary, tool_definitions).0;
        if session.messages.is_empty() || config.context_window_tokens == 0 || budget == 0 {
            return Ok(SessionTokenConsolidationOutcome {
                rounds: 0,
                archived_messages: 0,
                last_consolidated: session.last_consolidated,
                estimated_tokens: estimated,
                budget,
                raw_fallback: false,
                summary_stored: false,
            });
        }
        if estimated < budget {
            return Ok(SessionTokenConsolidationOutcome {
                rounds: 0,
                archived_messages: 0,
                last_consolidated: session.last_consolidated,
                estimated_tokens: estimated,
                budget,
                raw_fallback: false,
                summary_stored: false,
            });
        }

        let target = config.target_tokens();
        let mut rounds = 0;
        let mut archived_messages = 0;
        let mut raw_fallback = false;
        let mut last_summary = None;
        for _ in 0..config.max_rounds.max(1) {
            if estimated <= target {
                break;
            }
            let tokens_to_remove = estimated.saturating_sub(target).max(1);
            let Some((end_idx, _removed)) = pick_consolidation_boundary(session, tokens_to_remove)
            else {
                break;
            };
            let start = session.last_consolidated.min(session.messages.len());
            if end_idx <= start || end_idx > session.messages.len() {
                break;
            }
            let chunk = session.messages[start..end_idx].to_vec();
            if chunk.is_empty() {
                break;
            }
            let archive = self.archive_with_max_chars(
                store,
                &chunk,
                archive_prompt_char_budget(config.input_token_budget()),
            )?;
            rounds += 1;
            archived_messages += archive.archived_messages;
            raw_fallback |= archive.raw_fallback;
            if let Some(summary) = archive.summary.filter(|summary| summary != "(nothing)") {
                last_summary = Some(summary);
            }
            session.last_consolidated = end_idx;
            sessions.save(session)?;
            if archive.raw_fallback {
                break;
            }
            estimated =
                estimate_session_prompt_tokens(session, session_summary, tool_definitions).0;
            if estimated == 0 {
                break;
            }
        }

        let summary_stored = if let Some(summary) = last_summary {
            session.metadata.insert(
                "_last_summary".to_owned(),
                json!({"text": summary, "last_active": session.updated_at}),
            );
            sessions.save(session)?;
            true
        } else {
            false
        };

        Ok(SessionTokenConsolidationOutcome {
            rounds,
            archived_messages,
            last_consolidated: session.last_consolidated,
            estimated_tokens: estimated,
            budget,
            raw_fallback,
            summary_stored,
        })
    }
}

impl MemoryStore {
    pub fn new(workspace: impl AsRef<Path>) -> std::io::Result<Self> {
        Self::with_max_history_entries(workspace, DEFAULT_MAX_HISTORY_ENTRIES)
    }

    pub fn with_max_history_entries(
        workspace: impl AsRef<Path>,
        max_history_entries: usize,
    ) -> std::io::Result<Self> {
        let workspace = workspace.as_ref().to_path_buf();
        let memory_dir = workspace.join("memory");
        if memory_dir.exists() {
            reject_symlink(&memory_dir)?;
        }
        fs::create_dir_all(&memory_dir)?;
        let store = Self {
            workspace,
            memory_dir,
            max_history_entries,
        };
        store.maybe_migrate_legacy_history()?;
        Ok(store)
    }

    pub fn workspace(&self) -> &Path {
        &self.workspace
    }

    pub fn memory_dir(&self) -> &Path {
        &self.memory_dir
    }

    pub fn memory_path(&self) -> PathBuf {
        self.memory_dir.join("MEMORY.md")
    }

    pub fn history_path(&self) -> PathBuf {
        self.memory_dir.join("history.jsonl")
    }

    pub fn legacy_history_path(&self) -> PathBuf {
        self.memory_dir.join("HISTORY.md")
    }

    pub fn soul_path(&self) -> PathBuf {
        self.workspace.join("SOUL.md")
    }

    pub fn user_path(&self) -> PathBuf {
        self.workspace.join("USER.md")
    }

    pub fn cursor_path(&self) -> PathBuf {
        self.memory_dir.join(".cursor")
    }

    pub fn dream_cursor_path(&self) -> PathBuf {
        self.memory_dir.join(".dream_cursor")
    }

    pub fn read_memory(&self) -> String {
        read_file_or_empty(&self.memory_path())
    }

    pub fn write_memory(&self, content: &str) -> std::io::Result<()> {
        write_text_file(&self.memory_path(), content)
    }

    pub fn read_soul(&self) -> String {
        read_file_or_empty(&self.soul_path())
    }

    pub fn write_soul(&self, content: &str) -> std::io::Result<()> {
        write_text_file(&self.soul_path(), content)
    }

    pub fn read_user(&self) -> String {
        read_file_or_empty(&self.user_path())
    }

    pub fn write_user(&self, content: &str) -> std::io::Result<()> {
        write_text_file(&self.user_path(), content)
    }

    pub fn append_history(&self, entry: &str, max_chars: Option<usize>) -> std::io::Result<u64> {
        let limit = max_chars.unwrap_or(HISTORY_ENTRY_HARD_CAP);
        let cursor = self.next_cursor();
        let mut raw = entry.trim_end().to_owned();
        if raw.chars().count() > limit {
            raw = truncate_text(&raw, limit);
        }
        let content = strip_think(&raw);
        let record = MemoryHistoryEntry {
            cursor,
            timestamp: Local::now().format("%Y-%m-%d %H:%M").to_string(),
            content,
        };
        let path = self.history_path();
        reject_existing_symlink(&path)?;
        let mut file = OpenOptions::new().create(true).append(true).open(&path)?;
        writeln!(file, "{}", json_string(&record)?)?;
        write_text_file(&self.cursor_path(), &cursor.to_string())?;
        Ok(cursor)
    }

    pub fn raw_archive(
        &self,
        messages: &[Value],
        max_chars: Option<usize>,
    ) -> std::io::Result<u64> {
        let limit = max_chars.unwrap_or(RAW_ARCHIVE_MAX_CHARS);
        let formatted = truncate_text(&format_messages(messages), limit);
        self.append_history(
            &format!("[RAW] {} messages\n{formatted}", messages.len()),
            None,
        )
    }

    pub fn read_entries(&self) -> Vec<MemoryHistoryEntry> {
        read_entries_from_path(&self.history_path())
    }

    pub fn read_unprocessed_history(&self, since_cursor: u64) -> Vec<MemoryHistoryEntry> {
        self.read_entries()
            .into_iter()
            .filter(|entry| entry.cursor > since_cursor)
            .collect()
    }

    pub fn compact_history(&self) -> std::io::Result<()> {
        if self.max_history_entries == 0 {
            return Ok(());
        }
        let entries = self.read_entries();
        if entries.len() <= self.max_history_entries {
            return Ok(());
        }
        let start = entries.len().saturating_sub(self.max_history_entries);
        self.write_entries(&entries[start..])
    }

    pub fn get_last_dream_cursor(&self) -> u64 {
        read_cursor_file(&self.dream_cursor_path()).unwrap_or(0)
    }

    pub fn set_last_dream_cursor(&self, cursor: u64) -> std::io::Result<()> {
        write_text_file(&self.dream_cursor_path(), &cursor.to_string())
    }

    pub fn consolidate_pending(
        &self,
        consolidator: &dyn MemoryConsolidator,
    ) -> Result<MemoryConsolidationOutcome, MemoryConsolidationError> {
        let since = self.get_last_dream_cursor();
        let history = self.read_unprocessed_history(since);
        let Some(last_cursor) = history.last().map(|entry| entry.cursor) else {
            return Ok(MemoryConsolidationOutcome {
                processed_entries: 0,
                processed_cursor: since,
                memory_updated: false,
            });
        };
        let request = MemoryConsolidationRequest {
            existing_memory: self.read_memory(),
            history,
        };
        let updated_memory = consolidator.consolidate(request)?;
        let memory_updated = updated_memory.is_some();
        if let Some(updated_memory) = updated_memory {
            self.write_memory(&updated_memory)?;
        }
        self.set_last_dream_cursor(last_cursor)?;
        self.compact_history()?;
        Ok(MemoryConsolidationOutcome {
            processed_entries: self.read_unprocessed_history(since).len(),
            processed_cursor: last_cursor,
            memory_updated,
        })
    }

    pub fn memory_context_from_workspace(workspace: impl AsRef<Path>) -> Option<String> {
        let path = workspace.as_ref().join("memory").join("MEMORY.md");
        let content = read_file_or_empty(&path);
        let trimmed = content.trim();
        if trimmed.is_empty() || is_template_memory_placeholder(trimmed) {
            return None;
        }
        Some(format!("## Long-term Memory\n{trimmed}"))
    }

    pub fn recent_history_from_workspace(
        workspace: impl AsRef<Path>,
        max_entries: usize,
        max_chars: usize,
    ) -> Option<String> {
        let memory_dir = workspace.as_ref().join("memory");
        let cursor = read_cursor_file(&memory_dir.join(".dream_cursor")).unwrap_or(0);
        let entries = read_entries_from_path(&memory_dir.join("history.jsonl"))
            .into_iter()
            .filter(|entry| entry.cursor > cursor && !entry.content.is_empty())
            .map(|entry| format!("- [{}] {}", entry.timestamp, entry.content))
            .collect::<Vec<_>>();
        if entries.is_empty() {
            return None;
        }
        let start = entries.len().saturating_sub(max_entries);
        Some(truncate_text(&entries[start..].join("\n"), max_chars))
    }

    fn next_cursor(&self) -> u64 {
        if let Some(cursor) = read_cursor_file(&self.cursor_path()) {
            return cursor.saturating_add(1);
        }
        self.read_entries()
            .into_iter()
            .map(|entry| entry.cursor)
            .max()
            .unwrap_or(0)
            .saturating_add(1)
    }

    fn write_entries(&self, entries: &[MemoryHistoryEntry]) -> std::io::Result<()> {
        let path = self.history_path();
        reject_existing_symlink(&path)?;
        let tmp_path = unique_tmp_path(&path);
        let write_result = (|| -> std::io::Result<()> {
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&tmp_path)?;
            for entry in entries {
                writeln!(file, "{}", json_string(entry)?)?;
            }
            file.flush()?;
            file.sync_all()?;
            fs::rename(&tmp_path, &path)?;
            fsync_dir(&self.memory_dir)?;
            Ok(())
        })();
        if write_result.is_err() {
            let _ = fs::remove_file(&tmp_path);
        }
        write_result
    }

    fn maybe_migrate_legacy_history(&self) -> std::io::Result<()> {
        let legacy_path = self.legacy_history_path();
        if !legacy_path.exists()
            || self
                .history_path()
                .metadata()
                .map(|meta| meta.len() > 0)
                .unwrap_or(false)
        {
            return Ok(());
        }
        reject_symlink(&legacy_path)?;
        let text = fs::read_to_string(&legacy_path)?;
        let entries = parse_legacy_history(&text, legacy_fallback_timestamp(&legacy_path));
        if !entries.is_empty() {
            self.write_entries(&entries)?;
            if let Some(last) = entries.last() {
                write_text_file(&self.cursor_path(), &last.cursor.to_string())?;
                write_text_file(&self.dream_cursor_path(), &last.cursor.to_string())?;
            }
        }
        fs::rename(&legacy_path, next_legacy_backup_path(&self.memory_dir))?;
        Ok(())
    }
}

pub fn pick_consolidation_boundary(
    session: &Session,
    tokens_to_remove: usize,
) -> Option<(usize, usize)> {
    let start = session.last_consolidated.min(session.messages.len());
    if start >= session.messages.len() || tokens_to_remove == 0 {
        return None;
    }
    let mut removed_tokens = 0;
    let mut last_boundary = None;
    for index in start..session.messages.len() {
        let message = &session.messages[index];
        if index > start && message.get("role").and_then(Value::as_str) == Some("user") {
            last_boundary = Some((index, removed_tokens));
            if removed_tokens >= tokens_to_remove {
                return last_boundary;
            }
        }
        removed_tokens += estimate_message_tokens(message);
    }
    last_boundary
}

pub fn estimate_session_prompt_tokens(
    session: &Session,
    session_summary: Option<&str>,
    tool_definitions: &[Value],
) -> (usize, &'static str) {
    let mut probe_messages = Vec::new();
    if let Some(summary) = session_summary.filter(|summary| !summary.trim().is_empty()) {
        probe_messages.push(json!({"role": "system", "content": summary}));
    }
    probe_messages.extend(
        session
            .messages
            .iter()
            .skip(session.last_consolidated)
            .cloned(),
    );
    probe_messages.push(json!({"role": "user", "content": "[token-probe]"}));
    let message_tokens = estimate_messages_tokens(&probe_messages);
    let tool_tokens = tool_definitions
        .iter()
        .map(estimate_json_tokens)
        .sum::<usize>();
    (message_tokens + tool_tokens, "heuristic")
}

pub fn estimate_message_tokens(message: &Value) -> usize {
    estimate_json_tokens(message).max(1)
}

fn estimate_messages_tokens(messages: &[Value]) -> usize {
    messages.iter().map(estimate_message_tokens).sum()
}

fn estimate_json_tokens(value: &Value) -> usize {
    match value {
        Value::Null | Value::Bool(_) => 1,
        Value::Number(number) => estimate_text_tokens(&number.to_string()),
        Value::String(text) => estimate_text_tokens(text),
        Value::Array(values) => values.iter().map(estimate_json_tokens).sum::<usize>() + 1,
        Value::Object(object) => {
            object
                .iter()
                .map(|(key, value)| estimate_text_tokens(key) + estimate_json_tokens(value))
                .sum::<usize>()
                + 2
        }
    }
}

fn estimate_text_tokens(text: &str) -> usize {
    let chars = text.chars().count();
    let words = text.split_whitespace().count();
    chars.div_ceil(4).max(words).max(1)
}

fn build_dream_phase1_messages(history_text: &str, file_context: &str) -> Vec<Value> {
    let stale_threshold_days = DREAM_STALE_THRESHOLD_DAYS.to_string();
    let system_prompt = render_agent_template(
        AgentTemplate::DreamPhase1,
        &template_variables(&[("stale_threshold_days", &stale_threshold_days)]),
    )
    .unwrap_or_else(|_| {
        format!(
            "Analyze conversation history and current memory files. Identify durable facts, obsolete items, contradictions, and stale MEMORY.md lines older than {DREAM_STALE_THRESHOLD_DAYS} days. Return concise analysis for a follow-up editing agent."
        )
    });
    vec![
        json!({
            "role": "system",
            "content": system_prompt,
        }),
        json!({
            "role": "user",
            "content": format!("## Conversation History\n{history_text}\n\n{file_context}"),
        }),
    ]
}

fn build_dream_phase2_messages(
    analysis: &str,
    file_context: &str,
    skills: &[String],
) -> Vec<Value> {
    let system_prompt = render_agent_template(
        AgentTemplate::DreamPhase2,
        &template_variables(&[(
            "skill_creator_path",
            "skills/skill-creator/SKILL.md",
        )]),
    )
    .unwrap_or_else(|_| {
        "Edit memory files incrementally using read_file and edit_file. Use write_file only for new skills under skills/. Preserve useful facts, remove stale duplicates, and avoid wholesale rewrites unless the file is empty. Skill creation guidance lives at skills/skill-creator/SKILL.md when available.".to_owned()
    });
    let skills_section = if skills.is_empty() {
        String::new()
    } else {
        format!(
            "\n\n## Existing Skills\n{}",
            skills
                .iter()
                .map(|skill| format!("- {skill}"))
                .collect::<Vec<_>>()
                .join("\n")
        )
    };
    vec![
        json!({
            "role": "system",
            "content": system_prompt,
        }),
        json!({
            "role": "user",
            "content": format!("## Analysis Result\n{analysis}\n\n{file_context}{skills_section}"),
        }),
    ]
}

fn normalize_empty_file(content: String) -> String {
    if content.trim().is_empty() {
        "(empty)".to_owned()
    } else {
        content
    }
}

fn annotate_memory_with_ages(content: &str, ages: &[MemoryLineAge]) -> String {
    let had_trailing = content.ends_with('\n');
    let lines = content.lines().collect::<Vec<_>>();
    if lines.len() != ages.len() {
        return content.to_owned();
    }
    let mut annotated = lines
        .iter()
        .zip(ages)
        .map(|(line, age)| {
            if line.trim().is_empty() || age.age_days <= DREAM_STALE_THRESHOLD_DAYS {
                (*line).to_owned()
            } else {
                format!("{line}  ← {}d", age.age_days)
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    if had_trailing {
        annotated.push('\n');
    }
    annotated
}

fn list_existing_skills(workspace: &Path) -> Vec<String> {
    let skills_dir = workspace.join("skills");
    let Ok(entries) = fs::read_dir(&skills_dir) else {
        return Vec::new();
    };
    let mut skills = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .filter_map(|path| {
            let name = path.file_name()?.to_str()?.to_owned();
            let content = fs::read_to_string(path.join("SKILL.md")).ok()?;
            let description = extract_skill_description(&content)
                .unwrap_or_else(|| "(no description)".to_owned());
            Some(format!("{name} — {description}"))
        })
        .collect::<Vec<_>>();
    skills.sort();
    skills
}

fn extract_skill_description(content: &str) -> Option<String> {
    content.lines().find_map(|line| {
        let trimmed = line.trim();
        let (key, value) = trimmed.split_once(':')?;
        if key.eq_ignore_ascii_case("description") {
            let description = value.trim().trim_matches('"').to_owned();
            (!description.is_empty()).then_some(description)
        } else {
            None
        }
    })
}

fn build_memory_update_messages(request: &MemoryConsolidationRequest) -> Vec<Value> {
    vec![
        json!({
            "role": "system",
            "content": "Update MEMORY.md using the new history. Return only the complete updated MEMORY.md content. Preserve useful existing facts and remove obsolete duplicates.",
        }),
        json!({
            "role": "user",
            "content": format!(
                "Current MEMORY.md:\n{}\n\nNew history:\n{}",
                request.existing_memory.trim(),
                format_history_entries(&request.history),
            ),
        }),
    ]
}

fn build_archive_messages(messages: &[Value], max_chars: usize) -> Vec<Value> {
    let system_prompt = render_agent_template(AgentTemplate::ConsolidatorArchive, &BTreeMap::new())
        .unwrap_or_else(|_| {
            "Summarize the archived conversation into concise long-term memory facts. Return only the summary, or (nothing) if there is nothing useful.".to_owned()
        });
    vec![
        json!({
            "role": "system",
            "content": system_prompt,
        }),
        json!({
            "role": "user",
            "content": truncate_text(&format_messages(messages), max_chars.max(1)),
        }),
    ]
}

fn archive_prompt_char_budget(input_token_budget: usize) -> usize {
    if input_token_budget == 0 {
        return RAW_ARCHIVE_MAX_CHARS;
    }
    input_token_budget
        .saturating_mul(4)
        .clamp(1, RAW_ARCHIVE_MAX_CHARS)
}

fn format_history_entries(entries: &[MemoryHistoryEntry]) -> String {
    entries
        .iter()
        .filter(|entry| !entry.content.is_empty())
        .map(|entry| format!("[{} #{}] {}", entry.timestamp, entry.cursor, entry.content))
        .collect::<Vec<_>>()
        .join("\n")
}

fn read_entries_from_path(path: &Path) -> Vec<MemoryHistoryEntry> {
    let Ok(file) = File::open(path) else {
        return Vec::new();
    };
    BufReader::new(file)
        .lines()
        .map_while(Result::ok)
        .filter_map(|line| {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                return None;
            }
            serde_json::from_str::<MemoryHistoryEntry>(trimmed).ok()
        })
        .collect()
}

fn json_string(entry: &MemoryHistoryEntry) -> std::io::Result<String> {
    serde_json::to_string(entry).map_err(|error| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("memory history serialization failed: {error}"),
        )
    })
}

fn parse_legacy_history(text: &str, fallback_timestamp: String) -> Vec<MemoryHistoryEntry> {
    let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
    let chunks = split_legacy_history_chunks(normalized.trim());
    chunks
        .into_iter()
        .enumerate()
        .map(|(index, chunk)| {
            let (timestamp, content) = parse_legacy_timestamp(&chunk)
                .unwrap_or_else(|| (fallback_timestamp.clone(), chunk));
            MemoryHistoryEntry {
                cursor: index as u64 + 1,
                timestamp,
                content,
            }
        })
        .collect()
}

fn split_legacy_history_chunks(text: &str) -> Vec<String> {
    if text.is_empty() {
        return Vec::new();
    }
    let mut chunks = Vec::new();
    let mut current = Vec::new();
    let mut saw_blank_separator = false;
    let mut in_raw_block = false;
    for line in text.lines() {
        if current.is_empty() {
            in_raw_block = is_legacy_raw_block_start(line);
        }
        let starts_new_entry = if in_raw_block {
            saw_blank_separator && looks_like_legacy_entry_start(line)
        } else {
            (saw_blank_separator && !line.trim().is_empty()) || looks_like_legacy_entry_start(line)
        };
        if !current.is_empty() && starts_new_entry {
            let chunk = current.join("\n").trim().to_owned();
            if !chunk.is_empty() {
                chunks.push(chunk);
            }
            current.clear();
            in_raw_block = is_legacy_raw_block_start(line);
        }
        current.push(line.to_owned());
        saw_blank_separator = line.trim().is_empty();
    }
    let chunk = current.join("\n").trim().to_owned();
    if !chunk.is_empty() {
        chunks.push(chunk);
    }
    chunks
}

fn is_legacy_raw_block_start(line: &str) -> bool {
    let trimmed = line.trim_start();
    if trimmed.starts_with("[RAW]") {
        return true;
    }
    let Some((_, content)) = parse_legacy_timestamp(trimmed) else {
        return false;
    };
    content.trim_start().starts_with("[RAW]")
}

fn looks_like_legacy_entry_start(line: &str) -> bool {
    let trimmed = line.trim_start();
    trimmed.len() >= 12
        && trimmed.starts_with('[')
        && trimmed
            .get(1..5)
            .is_some_and(|year| year.chars().all(|c| c.is_ascii_digit()))
        && trimmed.get(5..6) == Some("-")
        && trimmed.get(8..9) == Some("-")
}

fn parse_legacy_timestamp(chunk: &str) -> Option<(String, String)> {
    let trimmed = chunk.trim_start();
    let close = trimmed.find(']')?;
    let timestamp = trimmed.get(1..close)?.trim().to_owned();
    if timestamp.len() < 10 || !timestamp.get(0..4)?.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let content = trimmed[close + 1..].trim_start().to_owned();
    Some((timestamp, content))
}

fn legacy_fallback_timestamp(path: &Path) -> String {
    path.metadata()
        .and_then(|metadata| metadata.modified())
        .ok()
        .map(chrono::DateTime::<Local>::from)
        .unwrap_or_else(Local::now)
        .format("%Y-%m-%d %H:%M")
        .to_string()
}

fn next_legacy_backup_path(memory_dir: &Path) -> PathBuf {
    let mut candidate = memory_dir.join("HISTORY.md.bak");
    let mut suffix = 2;
    while candidate.exists() {
        candidate = memory_dir.join(format!("HISTORY.md.bak.{suffix}"));
        suffix += 1;
    }
    candidate
}

fn format_messages(messages: &[Value]) -> String {
    messages
        .iter()
        .filter_map(|message| {
            let content = message
                .get("content")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let tools_used = format_tools_used(message);
            if content.is_empty() && tools_used.is_none() {
                return None;
            }
            let timestamp = message
                .get("timestamp")
                .and_then(Value::as_str)
                .unwrap_or("?");
            let role = message.get("role").and_then(Value::as_str).unwrap_or("?");
            let mut body = content.to_owned();
            if let Some(tools_used) = tools_used {
                if !body.is_empty() {
                    body.push('\n');
                }
                body.push_str(&tools_used);
            }
            Some(format!(
                "[{}] {}: {}",
                timestamp.chars().take(16).collect::<String>(),
                role.to_ascii_uppercase(),
                body
            ))
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn format_tools_used(message: &Value) -> Option<String> {
    if let Some(calls) = message.get("tool_calls").and_then(Value::as_array) {
        let names = calls
            .iter()
            .filter_map(|call| {
                call.get("function")
                    .and_then(|function| function.get("name"))
                    .or_else(|| call.get("name"))
                    .and_then(Value::as_str)
            })
            .filter(|name| !name.is_empty())
            .collect::<Vec<_>>();
        if !names.is_empty() {
            return Some(format!("tools_used: {}", names.join(", ")));
        }
    }
    if message.get("role").and_then(Value::as_str) == Some("tool") {
        let name = message
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("tool");
        let call_id = message
            .get("tool_call_id")
            .and_then(Value::as_str)
            .unwrap_or("?");
        return Some(format!("tool_result: {name} ({call_id})"));
    }
    None
}

fn read_file_or_empty(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_default()
}

fn read_cursor_file(path: &Path) -> Option<u64> {
    fs::read_to_string(path).ok()?.trim().parse::<u64>().ok()
}

fn write_text_file(path: &Path, content: &str) -> std::io::Result<()> {
    reject_existing_symlink(path)?;
    fs::write(path, content)
}

fn is_template_memory_placeholder(content: &str) -> bool {
    content.trim() == render_workspace_template(WorkspaceTemplate::Memory).trim()
}

fn reject_existing_symlink(path: &Path) -> std::io::Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "symlink paths are not allowed for memory persistence",
        )),
        Ok(_) | Err(_) => Ok(()),
    }
}

fn reject_symlink(path: &Path) -> std::io::Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() {
        Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "symlink paths are not allowed for memory persistence",
        ))
    } else {
        Ok(())
    }
}

fn fsync_dir(path: &Path) -> std::io::Result<()> {
    let dir = OpenOptions::new().read(true).open(path)?;
    dir.sync_all()
}

fn unique_tmp_path(path: &Path) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    let process_id = std::process::id();
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("history.jsonl");
    path.with_file_name(format!(".{file_name}.{process_id}.{nanos}.tmp"))
}
