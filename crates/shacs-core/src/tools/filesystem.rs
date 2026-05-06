use crate::tools::SchemaFragment;
use crate::tools::{
    BooleanSchema, FileState, IntegerSchema, JsonMap, StringSchema, Tool, ToolParameters,
    ToolResult,
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex};

const MAX_CHARS: usize = 128_000;
const DEFAULT_LIMIT: usize = 2_000;
const DEFAULT_HASH_LEN: usize = 8;
const MIN_HASH_LEN: usize = 2;
const MAX_HASH_LEN: usize = 64;
const MAX_EDIT_FILE_SIZE: u64 = 1024 * 1024 * 1024;
const BLOCKED_DEVICE_PATHS: &[&str] = &[
    "/dev/zero",
    "/dev/random",
    "/dev/urandom",
    "/dev/full",
    "/dev/stdin",
    "/dev/stdout",
    "/dev/stderr",
    "/dev/tty",
    "/dev/console",
    "/dev/fd/0",
    "/dev/fd/1",
    "/dev/fd/2",
];

#[derive(Debug, Clone, Default)]
pub struct PathContext {
    pub workspace: Option<PathBuf>,
    pub allowed_dir: Option<PathBuf>,
    pub media_dir: Option<PathBuf>,
    pub extra_allowed_dirs: Vec<PathBuf>,
}

impl PathContext {
    pub fn workspace(path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        Self {
            workspace: Some(path.clone()),
            allowed_dir: Some(path),
            media_dir: None,
            extra_allowed_dirs: Vec::new(),
        }
    }
}

pub fn resolve_path(path: &str, context: &PathContext) -> Result<PathBuf, String> {
    let expanded = expand_tilde(path);
    let candidate = if expanded.is_absolute() {
        expanded
    } else if let Some(workspace) = &context.workspace {
        workspace.join(expanded)
    } else {
        expanded
    };

    let resolved = fs::canonicalize(&candidate).map_err(|error| format!("{error}"))?;
    if let Some(allowed_dir) = &context.allowed_dir {
        let mut allowed_dirs = vec![allowed_dir.clone()];
        if let Some(media_dir) = &context.media_dir {
            allowed_dirs.push(media_dir.clone());
        }
        allowed_dirs.extend(context.extra_allowed_dirs.clone());
        let allowed = allowed_dirs.iter().any(|directory| {
            fs::canonicalize(directory)
                .map(|directory| resolved.starts_with(directory))
                .unwrap_or(false)
        });
        if !allowed {
            return Err(format!(
                "Path {path} is outside allowed directory {}",
                allowed_dir.display()
            ));
        }
    }
    Ok(resolved)
}

pub(crate) fn resolve_creatable_path(path: &str, context: &PathContext) -> Result<PathBuf, String> {
    let candidate = raw_candidate_path(path, context);
    reject_existing_symlink_components(&candidate)?;
    if candidate.exists() {
        return resolve_path(path, context);
    }

    let normalized = normalize_lexical(&candidate)?;
    let mut current = normalized.parent();
    let existing_parent = loop {
        match current {
            Some(parent) if parent.exists() => break parent,
            Some(parent) => current = parent.parent(),
            None => return Err(format!("Path {path} has no existing parent")),
        }
    };
    let canonical_parent = fs::canonicalize(existing_parent).map_err(|error| error.to_string())?;
    let suffix = normalized
        .strip_prefix(existing_parent)
        .map_err(|error| error.to_string())?;
    let canonical_target = canonical_parent.join(suffix);

    if let Some(allowed_dir) = &context.allowed_dir {
        let allowed_dirs = allowed_roots(context);
        let allowed_parent = allowed_dirs
            .iter()
            .any(|directory| canonical_parent.starts_with(directory));
        let allowed_target = allowed_dirs
            .iter()
            .any(|directory| canonical_target.starts_with(directory));
        if !(allowed_parent && allowed_target) {
            return Err(format!(
                "Path {path} is outside allowed directory {}",
                allowed_dir.display()
            ));
        }
    }
    Ok(canonical_target)
}

pub(crate) fn raw_candidate_path(path: &str, context: &PathContext) -> PathBuf {
    let expanded = expand_tilde(path);
    if expanded.is_absolute() {
        expanded
    } else if let Some(workspace) = &context.workspace {
        fs::canonicalize(workspace)
            .unwrap_or_else(|_| workspace.clone())
            .join(expanded)
    } else {
        expanded
    }
}

fn allowed_roots(context: &PathContext) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(allowed_dir) = &context.allowed_dir {
        if let Ok(path) = fs::canonicalize(allowed_dir) {
            roots.push(path);
        }
    }
    if let Some(media_dir) = &context.media_dir {
        if let Ok(path) = fs::canonicalize(media_dir) {
            roots.push(path);
        }
    }
    roots.extend(
        context
            .extra_allowed_dirs
            .iter()
            .filter_map(|path| fs::canonicalize(path).ok()),
    );
    roots
}

fn normalize_lexical(path: &Path) -> Result<PathBuf, String> {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => normalized.push(prefix.as_os_str()),
            Component::RootDir => normalized.push(component.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => {
                if !normalized.pop() {
                    return Err(format!("Path {} escapes its root", path.display()));
                }
            }
            Component::Normal(value) => normalized.push(value),
        }
    }
    Ok(normalized)
}

#[derive(Clone)]
pub struct ReadFileTool {
    context: PathContext,
    file_state: Arc<Mutex<FileState>>,
}

impl ReadFileTool {
    pub fn new(context: PathContext) -> Self {
        Self {
            context,
            file_state: Arc::new(Mutex::new(FileState::new())),
        }
    }

    pub fn with_file_state(context: PathContext, file_state: Arc<Mutex<FileState>>) -> Self {
        Self {
            context,
            file_state,
        }
    }
}

impl Tool for ReadFileTool {
    fn name(&self) -> &str {
        "read_file"
    }

    fn description(&self) -> &str {
        "Read a UTF-8 text file with line-numbered pagination. Images are identified by MIME; PDF/Office extraction is deferred."
    }

    fn parameters(&self) -> Value {
        ToolParameters::new()
            .property("path", StringSchema::new("The file path to read"))
            .property(
                "offset",
                IntegerSchema::new("Line number to start reading from (1-indexed, default 1)")
                    .minimum(1),
            )
            .property(
                "limit",
                IntegerSchema::new("Maximum number of lines to read (default 2000)").minimum(1),
            )
            .property(
                "pages",
                StringSchema::new("Page range for PDF files; PDF extraction is deferred"),
            )
            .property(
                "hashlines",
                BooleanSchema::new("If true, prefix each returned line with L{line}#{hash}|"),
            )
            .property(
                "hash_len",
                IntegerSchema::new("Hashline hash length (default 8)")
                    .minimum(MIN_HASH_LEN as i64)
                    .maximum(MAX_HASH_LEN as i64),
            )
            .required(["path"])
            .to_json_schema()
    }

    fn read_only(&self) -> bool {
        true
    }

    fn execute(&self, params: JsonMap) -> ToolResult {
        let path = params
            .get("path")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if path.is_empty() {
            return "Error reading file: Unknown path".into();
        }
        if is_blocked_device(path) {
            return format!(
                "Error: Reading {path} is blocked (device path that could hang or produce infinite output)."
            )
            .into();
        }
        let offset = params
            .get("offset")
            .and_then(Value::as_u64)
            .unwrap_or(1)
            .max(1);
        let limit = params
            .get("limit")
            .and_then(Value::as_u64)
            .and_then(|value| usize::try_from(value).ok());
        let hashlines = params
            .get("hashlines")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let hash_len = params
            .get("hash_len")
            .and_then(Value::as_u64)
            .and_then(|value| usize::try_from(value).ok())
            .map(normalize_hash_len)
            .unwrap_or(DEFAULT_HASH_LEN);

        match self.read_file(path, offset, limit, hashlines, hash_len) {
            Ok(text) => text.into(),
            Err(error) => format!("Error reading file: {error}").into(),
        }
    }
}

impl ReadFileTool {
    fn read_file(
        &self,
        path: &str,
        offset: u64,
        limit: Option<usize>,
        hashlines: bool,
        hash_len: usize,
    ) -> Result<String, String> {
        let fp = resolve_path(path, &self.context)?;
        if is_blocked_device(&fp) {
            return Ok(format!(
                "Error: Reading {} is blocked (device path that could hang or produce infinite output).",
                fp.display()
            ));
        }
        if !fp.exists() {
            return Ok(format!("Error: File not found: {path}"));
        }
        if !fp.is_file() {
            return Ok(format!("Error: Not a file: {path}"));
        }
        if matches!(
            fp.extension().and_then(|value| value.to_str()),
            Some("pdf" | "docx" | "xlsx" | "pptx")
        ) {
            return Ok(format!(
                "Error: Document extraction for {} is not implemented in this migration slice.",
                fp.display()
            ));
        }

        let raw = fs::read(&fp).map_err(|error| error.to_string())?;
        if raw.is_empty() {
            return Ok(format!("(Empty file: {path})"));
        }
        if let Some(mime) = detect_image_mime(&raw) {
            return Ok(format!("(Image file: {path}, MIME: {mime})"));
        }
        if is_binary(&raw) {
            return Ok(format!(
                "Error: Cannot read binary file {path} (MIME: unknown). Only UTF-8 text and images are supported."
            ));
        }

        if !hashlines {
            let mut state = self
                .file_state
                .lock()
                .map_err(|_| "file state lock poisoned".to_owned())?;
            if state.is_unchanged(&fp, offset, limit) {
                return Ok(format!("[File unchanged since last read: {path}]"));
            }
        }

        let text = String::from_utf8(raw).map_err(|_| {
            format!("Cannot read binary file {path} (MIME: unknown). Only UTF-8 text and images are supported.")
        })?;
        let text = text.replace("\r\n", "\n");
        let lines: Vec<&str> = text.lines().collect();
        let total = lines.len();
        if total == 0 {
            return Ok(format!("(Empty file: {path})"));
        }
        let start = usize::try_from(offset.saturating_sub(1)).map_err(|error| error.to_string())?;
        if start >= total {
            return Ok(format!(
                "Error: offset {offset} is beyond end of file ({total} lines)"
            ));
        }
        let cap = limit.unwrap_or(DEFAULT_LIMIT);
        let end = start.saturating_add(cap).min(total);
        let mut numbered = Vec::new();
        let mut chars = 0usize;
        for (index, line) in lines[start..end].iter().enumerate() {
            let line_number = start + index + 1;
            let numbered_line = if hashlines {
                let tag = make_hashline_tag(line_number, line, &fp, hash_len);
                format!("{tag}| {line}")
            } else {
                format!("{line_number}| {line}")
            };
            chars = chars.saturating_add(numbered_line.len() + 1);
            if chars > MAX_CHARS {
                break;
            }
            numbered.push(numbered_line);
        }
        let actual_end = start + numbered.len();
        let mut result = numbered.join("\n");
        if actual_end < total {
            result.push_str(&format!(
                "\n\n(Showing lines {offset}-{actual_end} of {total}. Use offset={} to continue.)",
                actual_end + 1
            ));
        } else {
            result.push_str(&format!("\n\n(End of file — {total} lines total)"));
        }
        self.file_state
            .lock()
            .map_err(|_| "file state lock poisoned".to_owned())?
            .record_read(&fp, offset, limit);
        Ok(result)
    }
}

#[derive(Clone)]
pub struct WriteFileTool {
    context: PathContext,
    file_state: Arc<Mutex<FileState>>,
}

impl WriteFileTool {
    pub fn new(context: PathContext) -> Self {
        Self {
            context,
            file_state: Arc::new(Mutex::new(FileState::new())),
        }
    }

    pub fn with_file_state(context: PathContext, file_state: Arc<Mutex<FileState>>) -> Self {
        Self {
            context,
            file_state,
        }
    }
}

impl Tool for WriteFileTool {
    fn name(&self) -> &str {
        "write_file"
    }

    fn description(&self) -> &str {
        "Write UTF-8 text content to a file, creating parent directories as needed."
    }

    fn parameters(&self) -> Value {
        ToolParameters::new()
            .property("path", StringSchema::new("The file path to write to"))
            .property("content", StringSchema::new("The content to write"))
            .required(["path", "content"])
            .to_json_schema()
    }

    fn execute(&self, params: JsonMap) -> ToolResult {
        let path = params
            .get("path")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let Some(content) = params.get("content").and_then(Value::as_str) else {
            return "Error writing file: Unknown content".into();
        };
        match self.write_file(path, content) {
            Ok(message) => message.into(),
            Err(error) => format!("Error writing file: {error}").into(),
        }
    }
}

impl WriteFileTool {
    fn write_file(&self, path: &str, content: &str) -> Result<String, String> {
        if path.is_empty() {
            return Err("Unknown path".to_owned());
        }
        if is_blocked_device(path) {
            return Err(format!(
                "Writing {path} is blocked (device path that could hang or corrupt streams)."
            ));
        }
        let fp = resolve_creatable_path(path, &self.context)?;
        reject_symlink_target(&fp)?;
        if fp.exists() && !fp.is_file() {
            return Err(format!("Not a file: {path}"));
        }
        if let Some(parent) = fp.parent() {
            fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        fs::write(&fp, content).map_err(|error| error.to_string())?;
        self.file_state
            .lock()
            .map_err(|_| "file state lock poisoned".to_owned())?
            .record_write(&fp);
        Ok(format!(
            "Successfully wrote {} characters to {}",
            content.chars().count(),
            fp.display()
        ))
    }
}

#[derive(Clone)]
pub struct EditFileTool {
    context: PathContext,
    file_state: Arc<Mutex<FileState>>,
}

impl EditFileTool {
    pub fn new(context: PathContext) -> Self {
        Self {
            context,
            file_state: Arc::new(Mutex::new(FileState::new())),
        }
    }

    pub fn with_file_state(context: PathContext, file_state: Arc<Mutex<FileState>>) -> Self {
        Self {
            context,
            file_state,
        }
    }
}

impl Tool for EditFileTool {
    fn name(&self) -> &str {
        "edit_file"
    }

    fn description(&self) -> &str {
        "Edit a UTF-8 text file using verified Hashline tags from read_file(hashlines=true)."
    }

    fn parameters(&self) -> Value {
        ToolParameters::new()
            .property("path", StringSchema::new("The file path to edit"))
            .raw_property(
                "op",
                json!({
                    "type": "string",
                    "enum": ["replace_line", "insert_before", "insert_after", "delete_line", "delete_range", "replace_range"],
                    "description": "Hashline edit operation"
                }),
            )
            .property(
                "hash_len",
                IntegerSchema::new("Hashline hash length used by read_file")
                    .minimum(MIN_HASH_LEN as i64)
                    .maximum(MAX_HASH_LEN as i64),
            )
            .property("line_tag", StringSchema::new("Target line tag, e.g. L12#9f86d081"))
            .property("start_tag", StringSchema::new("Inclusive range start tag"))
            .property("end_tag", StringSchema::new("Inclusive range end tag"))
            .property("text", StringSchema::new("Text to insert or replace with"))
            .required(["path", "op"])
            .to_json_schema()
    }

    fn execute(&self, params: JsonMap) -> ToolResult {
        let path = params
            .get("path")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let op = params.get("op").and_then(Value::as_str).unwrap_or_default();
        let hash_len = params
            .get("hash_len")
            .and_then(Value::as_u64)
            .and_then(|value| usize::try_from(value).ok())
            .map(normalize_hash_len)
            .unwrap_or(DEFAULT_HASH_LEN);
        let line_tag = params.get("line_tag").and_then(Value::as_str);
        let start_tag = params.get("start_tag").and_then(Value::as_str);
        let end_tag = params.get("end_tag").and_then(Value::as_str);
        let text = params.get("text").and_then(Value::as_str);

        let request = EditRequest {
            path,
            op,
            hash_len,
            line_tag,
            start_tag,
            end_tag,
            text,
        };

        match self.edit_file(request) {
            Ok(message) => message.into(),
            Err(error) => format!("Error editing file: {error}").into(),
        }
    }
}

struct EditRequest<'a> {
    path: &'a str,
    op: &'a str,
    hash_len: usize,
    line_tag: Option<&'a str>,
    start_tag: Option<&'a str>,
    end_tag: Option<&'a str>,
    text: Option<&'a str>,
}

impl EditFileTool {
    fn edit_file(&self, request: EditRequest<'_>) -> Result<String, String> {
        let EditRequest {
            path,
            op,
            hash_len,
            line_tag,
            start_tag,
            end_tag,
            text,
        } = request;
        if path.is_empty() {
            return Err("Unknown path".to_owned());
        }
        if path.ends_with(".ipynb") {
            return Ok("Error: This is a Jupyter notebook. Use the notebook_edit tool instead of edit_file.".to_owned());
        }
        reject_existing_symlink_components(&raw_candidate_path(path, &self.context))?;
        let fp = resolve_path(path, &self.context)?;
        reject_symlink_target(&fp)?;
        if !fp.exists() {
            return Ok(format!("Error: File not found: {path}"));
        }
        if !fp.is_file() {
            return Ok(format!("Error: Not a file: {path}"));
        }
        let file_size = fp.metadata().map_err(|error| error.to_string())?.len();
        if file_size > MAX_EDIT_FILE_SIZE {
            return Ok(format!(
                "Error: File too large to edit ({:.1} GiB). Maximum is 1 GiB.",
                file_size as f64 / (1024.0 * 1024.0 * 1024.0)
            ));
        }
        let warning = self
            .file_state
            .lock()
            .map_err(|_| "file state lock poisoned".to_owned())?
            .check_read(&fp);

        let raw = fs::read(&fp).map_err(|error| error.to_string())?;
        let uses_crlf = raw.windows(2).any(|pair| pair == b"\r\n");
        let content = String::from_utf8(raw)
            .map_err(|_| format!("Cannot edit binary or non-UTF-8 file: {path}"))?;
        let normalized = content.replace("\r\n", "\n");
        let mut lines = split_keepends(&normalized);
        let salt_path = fp.to_string_lossy().replace('\\', "/");

        match op {
            "replace_line" => {
                let line =
                    verify_line_tag(&lines, &salt_path, require(line_tag, "line_tag")?, hash_len)?;
                let replacement = preserve_line_ending(require(text, "text")?, &lines[line]);
                lines[line] = replacement;
            }
            "insert_before" => {
                let line =
                    verify_line_tag(&lines, &salt_path, require(line_tag, "line_tag")?, hash_len)?;
                let inserts = split_keepends(&normalize_block(require(text, "text")?, true));
                lines.splice(line..line, inserts);
            }
            "insert_after" => {
                let line =
                    verify_line_tag(&lines, &salt_path, require(line_tag, "line_tag")?, hash_len)?;
                let inserts = split_keepends(&normalize_block(require(text, "text")?, true));
                lines.splice((line + 1)..(line + 1), inserts);
            }
            "delete_line" => {
                let line =
                    verify_line_tag(&lines, &salt_path, require(line_tag, "line_tag")?, hash_len)?;
                lines.remove(line);
            }
            "delete_range" => {
                let (start, end) = verify_range(
                    &lines,
                    &salt_path,
                    require(start_tag, "start_tag")?,
                    require(end_tag, "end_tag")?,
                    hash_len,
                )?;
                lines.drain(start..=end);
            }
            "replace_range" => {
                let (start, end) = verify_range(
                    &lines,
                    &salt_path,
                    require(start_tag, "start_tag")?,
                    require(end_tag, "end_tag")?,
                    hash_len,
                )?;
                let inserts = split_keepends(&normalize_block(require(text, "text")?, true));
                lines.splice(start..=end, inserts);
            }
            _ => return Ok(format!("Error: Unsupported edit operation: {op}")),
        }

        let mut new_content = lines.join("");
        if uses_crlf {
            new_content = new_content.replace('\n', "\r\n");
        }
        fs::write(&fp, new_content).map_err(|error| error.to_string())?;
        self.file_state
            .lock()
            .map_err(|_| "file state lock poisoned".to_owned())?
            .record_write(&fp);
        let mut message = format!("Successfully edited {} (op={op})", fp.display());
        if let Some(warning) = warning {
            message = format!("{warning}\n{message}");
        }
        Ok(message)
    }
}

#[derive(Clone)]
pub struct ListDirTool {
    context: PathContext,
}

impl ListDirTool {
    pub const IGNORE_DIRS: &'static [&'static str] = &[
        ".git",
        "node_modules",
        "__pycache__",
        ".venv",
        "venv",
        "dist",
        "build",
        ".tox",
        ".mypy_cache",
        ".pytest_cache",
        ".ruff_cache",
        ".coverage",
        "htmlcov",
    ];

    pub fn new(context: PathContext) -> Self {
        Self { context }
    }
}

impl Tool for ListDirTool {
    fn name(&self) -> &str {
        "list_dir"
    }

    fn description(&self) -> &str {
        "List directory contents with optional recursion, skipping common noise directories."
    }

    fn parameters(&self) -> Value {
        ToolParameters::new()
            .property("path", StringSchema::new("The directory path to list"))
            .property(
                "recursive",
                crate::tools::BooleanSchema::new("Recursively list all files (default false)"),
            )
            .property(
                "max_entries",
                IntegerSchema::new("Maximum entries to return (default 200)").minimum(1),
            )
            .required(["path"])
            .to_json_schema()
    }

    fn read_only(&self) -> bool {
        true
    }

    fn execute(&self, params: JsonMap) -> ToolResult {
        let path = params
            .get("path")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let recursive = params
            .get("recursive")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let max_entries = params
            .get("max_entries")
            .and_then(Value::as_u64)
            .and_then(|value| usize::try_from(value).ok())
            .unwrap_or(200);

        match self.list_dir(path, recursive, max_entries) {
            Ok(text) => text.into(),
            Err(error) => format!("Error listing directory: {error}").into(),
        }
    }
}

impl ListDirTool {
    fn list_dir(&self, path: &str, recursive: bool, max_entries: usize) -> Result<String, String> {
        if path.is_empty() {
            return Err("Unknown path".to_owned());
        }
        let root = resolve_path(path, &self.context)?;
        if !root.exists() {
            return Ok(format!("Error: Directory not found: {path}"));
        }
        if !root.is_dir() {
            return Ok(format!("Error: Not a directory: {path}"));
        }

        let mut entries = Vec::new();
        if recursive {
            collect_recursive(&root, &root, max_entries, &mut entries)?;
        } else {
            for entry in sorted_read_dir(&root)? {
                if entry_is_symlink(&entry) || ignored_name(&entry.file_name().to_string_lossy()) {
                    continue;
                }
                if entries.len() < max_entries {
                    let name = entry.file_name().to_string_lossy().into_owned();
                    let prefix = if entry.path().is_dir() {
                        "📁 "
                    } else {
                        "📄 "
                    };
                    entries.push(format!("{prefix}{name}"));
                }
            }
        }
        if entries.is_empty() {
            Ok(format!("Directory {path} is empty"))
        } else {
            Ok(entries.join("\n"))
        }
    }
}

pub(crate) fn ignored_name(name: &str) -> bool {
    ListDirTool::IGNORE_DIRS.contains(&name)
}

pub(crate) fn sorted_read_dir(path: &Path) -> Result<Vec<fs::DirEntry>, String> {
    let mut entries = fs::read_dir(path)
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    entries.sort_by_key(|entry| entry.path());
    Ok(entries)
}

fn collect_recursive(
    root: &Path,
    current: &Path,
    max_entries: usize,
    entries: &mut Vec<String>,
) -> Result<(), String> {
    for entry in sorted_read_dir(current)? {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if entry_is_symlink(&entry) || ignored_name(&name) {
            continue;
        }
        if entries.len() < max_entries {
            let relative = path
                .strip_prefix(root)
                .map_err(|error| error.to_string())?
                .to_string_lossy()
                .replace('\\', "/");
            entries.push(if path.is_dir() {
                format!("{relative}/")
            } else {
                relative
            });
        }
        if path.is_dir() {
            collect_recursive(root, &path, max_entries, entries)?;
        }
    }
    Ok(())
}

pub(crate) fn entry_is_symlink(entry: &fs::DirEntry) -> bool {
    entry
        .file_type()
        .map(|file_type| file_type.is_symlink())
        .unwrap_or(true)
}

fn reject_symlink_target(path: &Path) -> Result<(), String> {
    if fs::symlink_metadata(path)
        .map(|metadata| metadata.file_type().is_symlink())
        .unwrap_or(false)
    {
        return Err(format!(
            "Refusing to write through symlink: {}",
            path.display()
        ));
    }
    Ok(())
}

pub(crate) fn reject_existing_symlink_components(path: &Path) -> Result<(), String> {
    let mut current = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => current.push(prefix.as_os_str()),
            Component::RootDir => current.push(component.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => current.push(component.as_os_str()),
            Component::Normal(value) => {
                current.push(value);
                match fs::symlink_metadata(&current) {
                    Ok(metadata) if metadata.file_type().is_symlink() => {
                        return Err(format!(
                            "Refusing to write through symlink: {}",
                            current.display()
                        ));
                    }
                    Ok(_) => {}
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    Err(error) => return Err(error.to_string()),
                }
            }
        }
    }
    Ok(())
}

fn normalize_hash_len(value: usize) -> usize {
    value.clamp(MIN_HASH_LEN, MAX_HASH_LEN)
}

fn short_hash(text: &str, salt: &str, length: usize) -> String {
    let length = normalize_hash_len(length);
    let mut hasher = Sha256::new();
    hasher.update(salt.as_bytes());
    hasher.update(b"\n");
    hasher.update(text.as_bytes());
    let digest = format!("{:x}", hasher.finalize());
    digest[..length.min(digest.len())].to_owned()
}

fn make_hashline_tag(line_number: usize, line_text: &str, path: &Path, hash_len: usize) -> String {
    let salt_path = path.to_string_lossy().replace('\\', "/");
    let salt = format!("path:{salt_path}");
    make_hashline_tag_with_salt(line_number, line_text, &salt, hash_len)
}

fn make_hashline_tag_with_salt(
    line_number: usize,
    line_text: &str,
    salt: &str,
    hash_len: usize,
) -> String {
    let stripped = line_text.trim_end_matches('\n');
    format!("L{line_number}#{}", short_hash(stripped, salt, hash_len))
}

fn parse_hashline_tag(tag: &str) -> Result<(usize, &str), String> {
    let Some(rest) = tag.strip_prefix('L') else {
        return Err(format!("Invalid hashline tag: {tag}"));
    };
    let Some((line_number, hash)) = rest.split_once('#') else {
        return Err(format!("Invalid hashline tag: {tag}"));
    };
    let line_number = line_number
        .parse::<usize>()
        .map_err(|_| format!("Invalid hashline line number: {tag}"))?;
    if hash.is_empty() {
        return Err(format!("Invalid empty hashline hash: {tag}"));
    }
    if !hash.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("Invalid non-hex hashline hash: {tag}"));
    }
    Ok((line_number, hash))
}

fn verify_line_tag(
    lines: &[String],
    salt_path: &str,
    tag: &str,
    hash_len: usize,
) -> Result<usize, String> {
    let (line_number, expected_hash) = parse_hashline_tag(tag)?;
    if expected_hash.len() != hash_len {
        return Err(format!(
            "Hashline tag length {} does not match hash_len {hash_len}: {tag}",
            expected_hash.len()
        ));
    }
    if line_number == 0 || line_number > lines.len() {
        return Err(format!("Hashline tag line number out of range: {tag}"));
    }
    let salt = format!("path:{salt_path}");
    let current_tag =
        make_hashline_tag_with_salt(line_number, &lines[line_number - 1], &salt, hash_len);
    let (_, current_hash) = parse_hashline_tag(&current_tag)?;
    if current_hash != expected_hash {
        return Err(
            "Hashline tag does not match current file content. Re-read with read_file(hashlines=true) and retry."
                .to_owned(),
        );
    }
    Ok(line_number - 1)
}

fn verify_range(
    lines: &[String],
    salt_path: &str,
    start_tag: &str,
    end_tag: &str,
    hash_len: usize,
) -> Result<(usize, usize), String> {
    let start = verify_line_tag(lines, salt_path, start_tag, hash_len)?;
    let end = verify_line_tag(lines, salt_path, end_tag, hash_len)?;
    if start > end {
        return Err(format!(
            "Invalid hashline range: start_tag is after end_tag ({} > {})",
            start + 1,
            end + 1
        ));
    }
    Ok((start, end))
}

fn split_keepends(text: &str) -> Vec<String> {
    if text.is_empty() {
        return Vec::new();
    }
    let mut lines = Vec::new();
    for segment in text.split_inclusive('\n') {
        lines.push(segment.to_owned());
    }
    if !text.ends_with('\n') {
        if let Some(last) = lines.last() {
            if last.ends_with('\n') {
                lines.push(String::new());
            }
        }
    }
    lines
}

fn normalize_block(text: &str, keep_trailing_newline: bool) -> String {
    if text.is_empty() {
        return String::new();
    }
    if keep_trailing_newline && !text.ends_with('\n') {
        format!("{text}\n")
    } else {
        text.to_owned()
    }
}

fn preserve_line_ending(text: &str, current_line: &str) -> String {
    if current_line.ends_with('\n') && !text.ends_with('\n') {
        format!("{text}\n")
    } else {
        text.to_owned()
    }
}

fn require<'a>(value: Option<&'a str>, name: &str) -> Result<&'a str, String> {
    value.ok_or_else(|| format!("Missing required parameter: {name}"))
}

fn expand_tilde(path: &str) -> PathBuf {
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

fn is_blocked_device(path: impl AsRef<Path>) -> bool {
    let raw = path.as_ref().to_string_lossy();
    if BLOCKED_DEVICE_PATHS.contains(&raw.as_ref()) {
        return true;
    }
    fs::canonicalize(path.as_ref())
        .map(|resolved| {
            let resolved = resolved.to_string_lossy();
            resolved.starts_with("/dev/") || BLOCKED_DEVICE_PATHS.contains(&resolved.as_ref())
        })
        .unwrap_or(false)
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

fn is_binary(raw: &[u8]) -> bool {
    if raw.contains(&0) {
        return true;
    }
    let sample = &raw[..raw.len().min(4096)];
    if sample.is_empty() {
        return false;
    }
    let non_text = sample
        .iter()
        .filter(|byte| **byte < 9 || (13 < **byte && **byte < 32))
        .count();
    (non_text as f64 / sample.len() as f64) > 0.2
}
