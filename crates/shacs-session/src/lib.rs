use chrono::Local;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashSet};
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub const FILE_MAX_MESSAGES: usize = 2000;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Session {
    pub key: String,
    #[serde(default)]
    pub messages: Vec<Value>,
    pub created_at: String,
    pub updated_at: String,
    #[serde(default)]
    pub metadata: Map<String, Value>,
    #[serde(default)]
    pub last_consolidated: usize,
}

impl Session {
    pub fn new(key: impl Into<String>) -> Self {
        let now = now_iso();
        Self {
            key: key.into(),
            messages: Vec::new(),
            created_at: now.clone(),
            updated_at: now,
            metadata: Map::new(),
            last_consolidated: 0,
        }
    }

    pub fn add_message(
        &mut self,
        role: impl Into<String>,
        content: impl Into<String>,
        extra: Map<String, Value>,
    ) {
        let mut message = Map::from_iter([
            ("role".to_owned(), Value::String(role.into())),
            ("content".to_owned(), Value::String(content.into())),
            ("timestamp".to_owned(), Value::String(now_iso())),
        ]);
        message.extend(extra);
        self.messages.push(Value::Object(message));
        self.updated_at = now_iso();
    }

    pub fn get_history(&self, max_messages: usize, include_timestamps: bool) -> Vec<Value> {
        self.get_history_with_options(SessionHistoryOptions {
            max_messages,
            include_timestamps,
            ..SessionHistoryOptions::default()
        })
    }

    pub fn get_history_with_options(&self, options: SessionHistoryOptions) -> Vec<Value> {
        let max_messages = if options.max_messages == 0 {
            120
        } else {
            options.max_messages
        };
        let include_timestamps = options.include_timestamps;
        let unconsolidated = self
            .messages
            .iter()
            .skip(self.last_consolidated.min(self.messages.len()))
            .cloned()
            .collect::<Vec<_>>();
        let mut sliced = recent_legal_history_suffix(&unconsolidated, max_messages);
        let legal_start = find_legal_message_start(&sliced);
        if legal_start > 0 {
            sliced = sliced[legal_start..].to_vec();
        }
        let mut history = drop_orphan_tool_results(sliced)
            .into_iter()
            .filter_map(|message| session_message_to_history(message, include_timestamps))
            .collect::<Vec<_>>();
        if options.max_tokens > 0 {
            history = trim_history_to_tokens(history, options.max_tokens);
        }
        history
    }

    pub fn retain_recent_legal_suffix(&mut self, max_messages: usize) {
        if max_messages == 0 {
            self.clear();
            return;
        }
        if self.messages.len() <= max_messages {
            return;
        }

        let before_len = self.messages.len();
        let mut retained = self.messages[before_len.saturating_sub(max_messages)..].to_vec();
        if let Some(first_user) = retained
            .iter()
            .position(|message| message.get("role").and_then(Value::as_str) == Some("user"))
        {
            retained = retained[first_user..].to_vec();
        } else if let Some(latest_user) = self
            .messages
            .iter()
            .rposition(|message| message.get("role").and_then(Value::as_str) == Some("user"))
        {
            let end = (latest_user + max_messages).min(self.messages.len());
            retained = self.messages[latest_user..end].to_vec();
        }

        let legal_start = find_legal_message_start(&retained);
        if legal_start > 0 {
            retained = retained[legal_start..].to_vec();
        }
        if retained.len() > max_messages {
            retained = retained[retained.len() - max_messages..].to_vec();
            let legal_start = find_legal_message_start(&retained);
            if legal_start > 0 {
                retained = retained[legal_start..].to_vec();
            }
        }
        let dropped = before_len.saturating_sub(retained.len());
        self.messages = retained;
        self.last_consolidated = self.last_consolidated.saturating_sub(dropped);
        self.updated_at = now_iso();
    }

    pub fn enforce_file_cap(&mut self) -> Vec<Value> {
        self.enforce_file_cap_with_limit(FILE_MAX_MESSAGES)
    }

    pub fn enforce_file_cap_with_limit(&mut self, limit: usize) -> Vec<Value> {
        if limit == 0 || self.messages.len() <= limit {
            return Vec::new();
        }
        let before = self.messages.clone();
        let before_last_consolidated = self.last_consolidated;
        self.retain_recent_legal_suffix(limit);
        let dropped_count = before.len().saturating_sub(self.messages.len());
        let already_consolidated = before_last_consolidated.min(dropped_count);
        before[already_consolidated..dropped_count].to_vec()
    }

    pub fn clear(&mut self) {
        self.messages.clear();
        self.last_consolidated = 0;
        self.updated_at = now_iso();
    }

    pub fn payload(&self) -> Value {
        session_payload(self)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionHistoryOptions {
    pub max_messages: usize,
    pub max_tokens: usize,
    pub include_timestamps: bool,
}

impl Default for SessionHistoryOptions {
    fn default() -> Self {
        Self {
            max_messages: 120,
            max_tokens: 0,
            include_timestamps: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionSummary {
    pub key: String,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
    pub path: PathBuf,
}

#[derive(Debug, Clone)]
pub struct SessionManager {
    workspace: PathBuf,
    sessions_dir: PathBuf,
    legacy_sessions_dir: Option<PathBuf>,
    cache: BTreeMap<String, Session>,
}

impl SessionManager {
    pub fn new(workspace: impl AsRef<Path>) -> std::io::Result<Self> {
        let workspace = workspace.as_ref().to_path_buf();
        let sessions_dir = workspace.join("sessions");
        if sessions_dir.exists() {
            reject_symlink(&sessions_dir)?;
        }
        fs::create_dir_all(&sessions_dir)?;
        Ok(Self {
            workspace,
            sessions_dir,
            legacy_sessions_dir: None,
            cache: BTreeMap::new(),
        })
    }

    pub fn with_legacy_sessions_dir(
        workspace: impl AsRef<Path>,
        legacy_sessions_dir: impl AsRef<Path>,
    ) -> std::io::Result<Self> {
        let mut manager = Self::new(workspace)?;
        manager.legacy_sessions_dir = Some(legacy_sessions_dir.as_ref().to_path_buf());
        Ok(manager)
    }

    pub fn workspace(&self) -> &Path {
        &self.workspace
    }

    pub fn sessions_dir(&self) -> &Path {
        &self.sessions_dir
    }

    pub fn legacy_sessions_dir(&self) -> Option<&Path> {
        self.legacy_sessions_dir.as_deref()
    }

    pub fn safe_key(key: &str) -> String {
        let sanitized = key
            .replace(':', "_")
            .chars()
            .map(|character| match character {
                '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
                other => other,
            })
            .collect::<String>();
        add_digest_suffix(&sanitized, key)
    }

    pub fn session_path(&self, key: &str) -> PathBuf {
        self.sessions_dir
            .join(format!("{}.jsonl", Self::safe_key(key)))
    }

    pub fn get_session_path(&self, key: &str) -> PathBuf {
        self.session_path(key)
    }

    pub fn legacy_session_path(&self, key: &str) -> Option<PathBuf> {
        self.legacy_sessions_dir
            .as_ref()
            .map(|dir| dir.join(format!("{}.jsonl", Self::safe_key(key))))
    }

    pub fn legacy_nanobot_session_path(&self, key: &str) -> PathBuf {
        self.sessions_dir
            .join(format!("{}.jsonl", legacy_nanobot_safe_key(key)))
    }

    pub fn existing_session_path(&self, key: &str) -> Option<PathBuf> {
        self.existing_session_path_with_legacy(key)
    }

    pub fn get_or_create(&mut self, key: &str) -> Session {
        if let Some(session) = self.cache.get(key) {
            return session.clone();
        }
        let session = self.load(key).unwrap_or_else(|| Session::new(key));
        self.cache.insert(key.to_owned(), session.clone());
        session
    }

    pub fn save(&mut self, session: &Session) -> std::io::Result<()> {
        self.save_inner(session, false)
    }

    pub fn save_with_fsync(&mut self, session: &Session) -> std::io::Result<()> {
        self.save_inner(session, true)
    }

    pub fn save_with_fsync_pruning_legacy(&mut self, session: &Session) -> std::io::Result<usize> {
        let paths = self.existing_session_paths_with_legacy(&session.key);
        validate_regular_session_paths(&paths)?;
        self.save_with_fsync(session)?;
        remove_non_canonical_session_paths(
            paths,
            &self.session_path(&session.key),
            &self.sessions_dir,
        )
    }

    fn save_inner(&mut self, session: &Session, fsync: bool) -> std::io::Result<()> {
        let path = self.session_path(&session.key);
        reject_symlink(&self.sessions_dir)?;
        let tmp_path = unique_tmp_path(&path);
        let write_result = (|| -> std::io::Result<()> {
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&tmp_path)?;
            let metadata = json!({
                "_type": "metadata",
                "key": session.key,
                "created_at": session.created_at,
                "updated_at": session.updated_at,
                "metadata": session.metadata,
                "last_consolidated": session.last_consolidated,
            });
            writeln!(file, "{metadata}")?;
            for message in &session.messages {
                writeln!(file, "{message}")?;
            }
            file.flush()?;
            if fsync {
                file.sync_all()?;
            }
            fs::rename(&tmp_path, &path)?;
            if fsync {
                fsync_dir(&self.sessions_dir)?;
            }
            Ok(())
        })();
        if write_result.is_err() {
            let _ = fs::remove_file(&tmp_path);
        }
        write_result?;
        self.cache.insert(session.key.clone(), session.clone());
        Ok(())
    }

    pub fn flush_all(&mut self) -> std::io::Result<usize> {
        let sessions = self.cache.values().cloned().collect::<Vec<_>>();
        let mut flushed = 0;
        for session in &sessions {
            if self.save_with_fsync(session).is_ok() {
                flushed += 1;
            }
        }
        Ok(flushed)
    }

    pub fn load(&self, key: &str) -> Option<Session> {
        let path = self.session_path(key);
        if !path.exists() {
            for legacy_path in self.legacy_session_paths(key) {
                if legacy_path.exists() {
                    let _ = migrate_legacy_regular_file(&legacy_path, &path);
                    break;
                }
            }
        }
        let path = self.existing_session_path_with_legacy(key).unwrap_or(path);
        self.load_from_path(key, &path)
    }

    pub fn load_existing(&self, key: &str) -> Option<Session> {
        self.existing_session_path_with_legacy(key)
            .and_then(|path| self.load_from_path(key, &path))
    }

    pub fn read_session_file(&self, key: &str) -> Option<Value> {
        self.load_existing(key).map(|session| {
            json!({
                "key": session.key,
                "created_at": session.created_at,
                "updated_at": session.updated_at,
                "metadata": session.metadata,
                "last_consolidated": session.last_consolidated,
                "messages": session.messages,
            })
        })
    }

    pub fn read_session_payload(&self, key: &str) -> Option<Value> {
        self.load_existing(key).map(|session| session.payload())
    }

    pub fn invalidate(&mut self, key: &str) {
        self.cache.remove(key);
    }

    pub fn delete_session(&mut self, key: &str) -> std::io::Result<bool> {
        reject_symlink(&self.sessions_dir)?;
        let paths = self.existing_session_paths_with_legacy(key);
        if paths.is_empty() {
            return Ok(false);
        };
        validate_regular_session_paths(&paths)?;
        for path in paths {
            fs::remove_file(path)?;
        }
        fsync_dir(&self.sessions_dir)?;
        self.invalidate(key);
        Ok(true)
    }

    pub fn clear_session(&mut self, key: &str) -> std::io::Result<Option<usize>> {
        let paths = self.existing_session_paths_with_legacy(key);
        if paths.is_empty() {
            return Ok(None);
        }
        validate_regular_session_paths(&paths)?;
        let Some(mut session) = self.load_existing(key) else {
            return Ok(None);
        };
        let before = session.messages.len();
        session.clear();
        self.save_with_fsync(&session)?;
        remove_non_canonical_session_paths(
            paths,
            &self.session_path(&session.key),
            &self.sessions_dir,
        )?;
        Ok(Some(before))
    }

    pub fn list_sessions(&self) -> std::io::Result<Vec<SessionSummary>> {
        let mut sessions = Vec::new();
        for entry in fs::read_dir(&self.sessions_dir)? {
            let path = entry?.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("jsonl") {
                continue;
            }
            let metadata = fs::symlink_metadata(&path)?;
            if metadata.file_type().is_symlink() || !metadata.is_file() {
                continue;
            }
            let fallback_key = path
                .file_stem()
                .and_then(|stem| stem.to_str())
                .unwrap_or_default()
                .replacen('_', ":", 1);
            if let Some(session) = self.load_from_path(&fallback_key, &path) {
                sessions.push(SessionSummary {
                    key: session.key,
                    created_at: Some(session.created_at),
                    updated_at: Some(session.updated_at),
                    path,
                });
            }
        }
        sessions.sort_by(|left, right| right.updated_at.cmp(&left.updated_at));
        Ok(sessions)
    }

    fn load_from_path(&self, key: &str, path: &Path) -> Option<Session> {
        let metadata = fs::symlink_metadata(path).ok()?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return None;
        }
        let file = File::open(path).ok()?;
        let mut messages = Vec::new();
        let mut metadata = Map::new();
        let mut created_at = None;
        let mut updated_at = None;
        let mut stored_key = None;
        let mut last_consolidated = 0;
        let mut saw_valid_record = false;
        for line in BufReader::new(file).lines().map_while(Result::ok) {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let Ok(data) = serde_json::from_str::<Value>(line) else {
                continue;
            };
            if data.get("_type").and_then(Value::as_str) == Some("metadata") {
                saw_valid_record = true;
                metadata = data
                    .get("metadata")
                    .and_then(Value::as_object)
                    .cloned()
                    .unwrap_or_default();
                created_at = data
                    .get("created_at")
                    .and_then(Value::as_str)
                    .map(str::to_owned);
                updated_at = data
                    .get("updated_at")
                    .and_then(Value::as_str)
                    .map(str::to_owned);
                stored_key = data.get("key").and_then(Value::as_str).map(str::to_owned);
                last_consolidated = data
                    .get("last_consolidated")
                    .and_then(Value::as_u64)
                    .unwrap_or(0) as usize;
            } else {
                saw_valid_record = true;
                messages.push(data);
            }
        }
        if !saw_valid_record {
            return None;
        }
        Some(Session {
            key: stored_key.unwrap_or_else(|| key.to_owned()),
            messages,
            created_at: created_at.unwrap_or_else(now_iso),
            updated_at: updated_at.unwrap_or_else(now_iso),
            metadata,
            last_consolidated,
        })
    }

    fn legacy_session_paths(&self, key: &str) -> Vec<PathBuf> {
        let Some(dir) = &self.legacy_sessions_dir else {
            return Vec::new();
        };
        let mut paths = [
            dir.join(format!("{}.jsonl", Self::safe_key(key))),
            dir.join(format!("{}.jsonl", legacy_nanobot_safe_key(key))),
        ]
        .into_iter()
        .collect::<Vec<_>>();
        paths.dedup();
        paths
    }

    fn existing_session_path_with_legacy(&self, key: &str) -> Option<PathBuf> {
        self.existing_session_paths_with_legacy(key)
            .into_iter()
            .next()
    }

    fn existing_session_paths_with_legacy(&self, key: &str) -> Vec<PathBuf> {
        let mut paths = [
            self.session_path(key),
            self.legacy_nanobot_session_path(key),
        ]
        .into_iter()
        .filter(|path| fs::symlink_metadata(path).is_ok())
        .collect::<Vec<_>>();
        paths.dedup();
        paths
    }
}

fn recent_legal_history_suffix(messages: &[Value], max_messages: usize) -> Vec<Value> {
    if max_messages == 0 || messages.is_empty() {
        return Vec::new();
    }
    let start = messages.len().saturating_sub(max_messages);
    let mut sliced = messages[start..].to_vec();
    if let Some(first_user) = sliced
        .iter()
        .position(|message| message.get("role").and_then(Value::as_str) == Some("user"))
    {
        let start = if first_user > 0
            && sliced[first_user - 1]
                .get("_channel_delivery")
                .and_then(Value::as_bool)
                .unwrap_or(false)
        {
            first_user - 1
        } else {
            first_user
        };
        sliced = sliced[start..].to_vec();
    } else if let Some(latest_user) = messages
        .iter()
        .rposition(|message| message.get("role").and_then(Value::as_str) == Some("user"))
    {
        let end = (latest_user + max_messages).min(messages.len());
        sliced = messages[latest_user..end].to_vec();
    } else {
        sliced.clear();
    }
    sliced
}

fn session_payload(session: &Session) -> Value {
    json!({
        "key": session.key,
        "created_at": session.created_at,
        "updated_at": session.updated_at,
        "metadata": session.metadata,
        "messages": session.messages,
    })
}

fn validate_regular_session_paths(paths: &[PathBuf]) -> std::io::Result<()> {
    for path in paths {
        reject_symlink(path)?;
        let metadata = fs::symlink_metadata(path)?;
        if !metadata.is_file() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "session path is not a regular file",
            ));
        }
    }
    Ok(())
}

fn remove_non_canonical_session_paths(
    paths: Vec<PathBuf>,
    canonical: &Path,
    sessions_dir: &Path,
) -> std::io::Result<usize> {
    let mut removed = 0;
    for path in paths {
        if path == canonical {
            continue;
        }
        fs::remove_file(path)?;
        removed += 1;
    }
    if removed > 0 {
        fsync_dir(sessions_dir)?;
    }
    Ok(removed)
}

fn session_message_to_history(mut message: Value, include_timestamps: bool) -> Option<Value> {
    let object = message.as_object_mut()?;
    let role = object.get("role")?.clone();
    let role_name = role.as_str().unwrap_or_default();
    let mut content = object
        .get("content")
        .cloned()
        .unwrap_or(Value::String(String::new()));
    if let Some(media) = object.get("media").and_then(Value::as_array) {
        if !media.is_empty() {
            if let Some(text) = content.as_str() {
                let breadcrumbs = media
                    .iter()
                    .filter_map(Value::as_str)
                    .filter(|path| !path.is_empty())
                    .map(|path| format!("[image: {path}]"))
                    .collect::<Vec<_>>()
                    .join("\n");
                if !breadcrumbs.is_empty() {
                    content = Value::String(if text.is_empty() {
                        breadcrumbs
                    } else {
                        format!("{text}\n{breadcrumbs}")
                    });
                }
            }
        }
    }
    if include_timestamps && should_annotate_timestamp(object, role_name) {
        if let (Some(timestamp), Some(text)) = (
            object.get("timestamp").and_then(Value::as_str),
            content.as_str(),
        ) {
            content = Value::String(format!("[Message Time: {timestamp}]\n{text}"));
        }
    }
    let mut out = Map::from_iter([("role".to_owned(), role), ("content".to_owned(), content)]);
    for key in [
        "tool_calls",
        "tool_call_id",
        "name",
        "reasoning_content",
        "thinking_blocks",
    ] {
        if let Some(value) = object.get(key) {
            out.insert(key.to_owned(), value.clone());
        }
    }
    Some(Value::Object(out))
}

pub fn find_legal_message_start(messages: &[Value]) -> usize {
    let mut pending = HashSet::new();
    let mut start = 0;
    for (index, message) in messages.iter().enumerate() {
        match message.get("role").and_then(Value::as_str) {
            Some("assistant") => {
                pending.extend(assistant_tool_call_ids(message));
            }
            Some("tool") => {
                let tool_call_id = message.get("tool_call_id").and_then(Value::as_str);
                if tool_call_id.is_some_and(|id| pending.remove(id)) {
                    continue;
                } else {
                    start = index + 1;
                    pending.clear();
                }
            }
            _ => {
                pending.clear();
            }
        }
    }
    start
}

fn trim_history_to_tokens(history: Vec<Value>, max_tokens: usize) -> Vec<Value> {
    let mut kept = Vec::new();
    let mut used = 0;
    for message in history.iter().rev() {
        let tokens = estimate_message_tokens(message);
        if !kept.is_empty() && used + tokens > max_tokens {
            break;
        }
        kept.push(message.clone());
        used += tokens;
    }
    kept.reverse();
    if let Some(first_user) = kept
        .iter()
        .position(|message| message.get("role").and_then(Value::as_str) == Some("user"))
    {
        return kept[first_user..].to_vec();
    }
    if let Some(recovered_user) = history
        .iter()
        .rposition(|message| message.get("role").and_then(Value::as_str) == Some("user"))
    {
        return history[recovered_user..].to_vec();
    }
    kept
}

fn estimate_message_tokens(message: &Value) -> usize {
    let chars = message.to_string().chars().count();
    (chars / 4).max(1)
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
        .unwrap_or("session.jsonl");
    path.with_file_name(format!(".{file_name}.{process_id}.{nanos}.tmp"))
}

fn reject_symlink(path: &Path) -> std::io::Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() {
        Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "symlink paths are not allowed for session persistence",
        ))
    } else {
        Ok(())
    }
}

fn migrate_legacy_regular_file(legacy_path: &Path, path: &Path) -> std::io::Result<()> {
    let metadata = fs::symlink_metadata(legacy_path)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "legacy session path is not a regular file",
        ));
    }
    if let Some(parent) = path.parent() {
        reject_symlink(parent)?;
    }
    fs::rename(legacy_path, path).or_else(|_| {
        fs::copy(legacy_path, path).map(|_| {
            let _ = fs::remove_file(legacy_path);
        })
    })
}

fn add_digest_suffix(sanitized: &str, original: &str) -> String {
    let prefix = if sanitized.is_empty() || sanitized == "." || sanitized == ".." {
        "session"
    } else {
        sanitized
    };
    let digest = Sha256::digest(original.as_bytes());
    format!("{prefix}-{:x}", digest)[..prefix.len() + 1 + 12].to_owned()
}

fn legacy_nanobot_safe_key(key: &str) -> String {
    let sanitized = key
        .replace(':', "_")
        .chars()
        .map(|character| match character {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
            other if other.is_control() => '_',
            other => other,
        })
        .collect::<String>();
    if sanitized.is_empty() || sanitized == "." || sanitized == ".." {
        "session".to_owned()
    } else {
        sanitized
    }
}

fn should_annotate_timestamp(object: &Map<String, Value>, role: &str) -> bool {
    role == "user"
        || (role == "assistant"
            && object
                .get("_channel_delivery")
                .and_then(Value::as_bool)
                .unwrap_or(false))
}

fn drop_orphan_tool_results(messages: Vec<Value>) -> Vec<Value> {
    let mut pending_tool_ids = HashSet::new();
    let mut filtered = Vec::new();

    for message in messages {
        match message.get("role").and_then(Value::as_str) {
            Some("assistant") => {
                pending_tool_ids = assistant_tool_call_ids(&message);
                filtered.push(message);
            }
            Some("tool") => {
                let Some(tool_call_id) = message.get("tool_call_id").and_then(Value::as_str) else {
                    continue;
                };
                if pending_tool_ids.remove(tool_call_id) {
                    filtered.push(message);
                }
            }
            _ => {
                pending_tool_ids.clear();
                filtered.push(message);
            }
        }
    }

    filtered
}

fn assistant_tool_call_ids(message: &Value) -> HashSet<String> {
    message
        .get("tool_calls")
        .and_then(Value::as_array)
        .map(|tool_calls| {
            tool_calls
                .iter()
                .filter_map(|call| call.get("id").and_then(Value::as_str).map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

fn now_iso() -> String {
    Local::now().to_rfc3339()
}
