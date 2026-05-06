use crate::tools::filesystem::{
    entry_is_symlink, ignored_name, resolve_path, sorted_read_dir, PathContext,
};
use crate::tools::SchemaFragment;
use crate::tools::{
    BooleanSchema, IntegerSchema, JsonMap, StringSchema, Tool, ToolParameters, ToolResult,
};
use glob::Pattern;
use regex::RegexBuilder;
use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

const DEFAULT_HEAD_LIMIT: usize = 250;
const MAX_RESULT_CHARS: usize = 128_000;
const MAX_FILE_BYTES: u64 = 2_000_000;

#[derive(Clone)]
pub struct GlobTool {
    context: PathContext,
}

impl GlobTool {
    pub fn new(context: PathContext) -> Self {
        Self { context }
    }
}

impl Tool for GlobTool {
    fn name(&self) -> &str {
        "glob"
    }

    fn description(&self) -> &str {
        "Find files matching a glob pattern, sorted by modification time newest first."
    }

    fn parameters(&self) -> Value {
        ToolParameters::new()
            .property(
                "pattern",
                StringSchema::new("Glob pattern to match, e.g. '*.rs' or 'tests/**/test_*.rs'")
                    .min_length(1),
            )
            .property(
                "path",
                StringSchema::new("Directory to search from (default '.')"),
            )
            .property(
                "max_results",
                IntegerSchema::new("Legacy alias for head_limit")
                    .minimum(1)
                    .maximum(1000),
            )
            .property(
                "head_limit",
                IntegerSchema::new("Maximum number of matches to return (default 250)")
                    .minimum(0)
                    .maximum(1000),
            )
            .property(
                "offset",
                IntegerSchema::new("Skip the first N matching entries before returning results")
                    .minimum(0)
                    .maximum(100_000),
            )
            .raw_property(
                "entry_type",
                json!({
                    "type": "string",
                    "enum": ["files", "dirs", "both"],
                    "description": "Whether to match files, directories, or both (default files)"
                }),
            )
            .required(["pattern"])
            .to_json_schema()
    }

    fn read_only(&self) -> bool {
        true
    }

    fn execute(&self, params: JsonMap) -> ToolResult {
        let pattern = params
            .get("pattern")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let path = params.get("path").and_then(Value::as_str).unwrap_or(".");
        let head_limit = params
            .get("head_limit")
            .and_then(Value::as_u64)
            .and_then(|value| usize::try_from(value).ok());
        let max_results = params
            .get("max_results")
            .and_then(Value::as_u64)
            .and_then(|value| usize::try_from(value).ok());
        let offset = params
            .get("offset")
            .and_then(Value::as_u64)
            .and_then(|value| usize::try_from(value).ok())
            .unwrap_or(0);
        let entry_type = params
            .get("entry_type")
            .and_then(Value::as_str)
            .unwrap_or("files");

        match self.glob(pattern, path, head_limit, max_results, offset, entry_type) {
            Ok(text) => text.into(),
            Err(error) => format!("Error finding files: {error}").into(),
        }
    }
}

impl GlobTool {
    fn glob(
        &self,
        pattern: &str,
        path: &str,
        head_limit: Option<usize>,
        max_results: Option<usize>,
        offset: usize,
        entry_type: &str,
    ) -> Result<String, String> {
        let root = resolve_path(path, &self.context)?;
        if !root.exists() {
            return Ok(format!("Error: Path not found: {path}"));
        }
        if !root.is_dir() {
            return Ok(format!("Error: Not a directory: {path}"));
        }
        let matcher =
            Pattern::new(&normalize_pattern(pattern)).map_err(|error| error.to_string())?;
        let include_files = matches!(entry_type, "files" | "both");
        let include_dirs = matches!(entry_type, "dirs" | "both");
        let limit = limit_from(head_limit, max_results, DEFAULT_HEAD_LIMIT);

        let mut matches = Vec::new();
        for entry in collect_entries(&root, include_files, include_dirs)? {
            let relative = entry
                .strip_prefix(&root)
                .map_err(|error| error.to_string())?
                .to_string_lossy()
                .replace('\\', "/");
            let Some(name) = entry.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            if match_glob(&matcher, &relative, name, pattern) {
                let mut display = display_path(&entry, &root, &self.context);
                if entry.is_dir() {
                    display.push('/');
                }
                let modified = entry
                    .metadata()
                    .and_then(|metadata| metadata.modified())
                    .unwrap_or(SystemTime::UNIX_EPOCH);
                matches.push((display, modified));
            }
        }
        if matches.is_empty() {
            return Ok(format!("No paths matched pattern '{pattern}' in {path}"));
        }
        matches.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(&right.0)));
        let names = matches
            .into_iter()
            .map(|(name, _)| name)
            .collect::<Vec<_>>();
        Ok(render_page(names, limit, offset))
    }
}

#[derive(Clone)]
pub struct GrepTool {
    context: PathContext,
}

impl GrepTool {
    pub fn new(context: PathContext) -> Self {
        Self { context }
    }
}

impl Tool for GrepTool {
    fn name(&self) -> &str {
        "grep"
    }

    fn description(&self) -> &str {
        "Search UTF-8 file contents with a regex or fixed string pattern."
    }

    fn parameters(&self) -> Value {
        ToolParameters::new()
            .property("pattern", StringSchema::new("Regex or plain text pattern to search for").min_length(1))
            .property("path", StringSchema::new("File or directory to search in (default '.')"))
            .property("glob", StringSchema::new("Optional file filter, e.g. '*.rs'"))
            .property("type", StringSchema::new("Optional file type shorthand, e.g. 'rs', 'md', 'json'"))
            .property("case_insensitive", BooleanSchema::new("Case-insensitive search (default false)"))
            .property("fixed_strings", BooleanSchema::new("Treat pattern as plain text instead of regex"))
            .raw_property("output_mode", json!({
                "type": "string",
                "enum": ["content", "files_with_matches", "count"],
                "description": "content, files_with_matches, or count; default files_with_matches"
            }))
            .property("context_before", IntegerSchema::new("Number of lines before each match").minimum(0).maximum(20))
            .property("context_after", IntegerSchema::new("Number of lines after each match").minimum(0).maximum(20))
            .property("max_matches", IntegerSchema::new("Legacy alias for head_limit in content mode").minimum(1).maximum(1000))
            .property("max_results", IntegerSchema::new("Legacy alias for head_limit in non-content modes").minimum(1).maximum(1000))
            .property("head_limit", IntegerSchema::new("Maximum number of results to return").minimum(0).maximum(1000))
            .property("offset", IntegerSchema::new("Skip the first N results before applying head_limit").minimum(0).maximum(100_000))
            .required(["pattern"])
            .to_json_schema()
    }

    fn read_only(&self) -> bool {
        true
    }

    fn execute(&self, params: JsonMap) -> ToolResult {
        match self.grep(params) {
            Ok(text) => text.into(),
            Err(error) => format!("Error searching files: {error}").into(),
        }
    }
}

impl GrepTool {
    fn grep(&self, params: JsonMap) -> Result<String, String> {
        let pattern = params
            .get("pattern")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let path = params.get("path").and_then(Value::as_str).unwrap_or(".");
        let output_mode = params
            .get("output_mode")
            .and_then(Value::as_str)
            .unwrap_or("files_with_matches");
        let case_insensitive = params
            .get("case_insensitive")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let fixed_strings = params
            .get("fixed_strings")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let context_before = usize_param(&params, "context_before", 0);
        let context_after = usize_param(&params, "context_after", 0);
        let offset = usize_param(&params, "offset", 0);
        let head_limit = params
            .get("head_limit")
            .and_then(Value::as_u64)
            .and_then(|value| usize::try_from(value).ok());
        let max_matches = params
            .get("max_matches")
            .and_then(Value::as_u64)
            .and_then(|value| usize::try_from(value).ok());
        let max_results = params
            .get("max_results")
            .and_then(Value::as_u64)
            .and_then(|value| usize::try_from(value).ok());
        let limit = if output_mode == "content" {
            limit_from(head_limit, max_matches, DEFAULT_HEAD_LIMIT)
        } else {
            limit_from(head_limit, max_results, DEFAULT_HEAD_LIMIT)
        };
        let glob_filter = params.get("glob").and_then(Value::as_str);
        let type_filter = params.get("type").and_then(Value::as_str);

        let target = resolve_path(path, &self.context)?;
        if !target.exists() {
            return Ok(format!("Error: Path not found: {path}"));
        }
        if !(target.is_dir() || target.is_file()) {
            return Ok(format!("Error: Unsupported path: {path}"));
        }
        let regex_pattern = if fixed_strings {
            regex::escape(pattern)
        } else {
            pattern.to_owned()
        };
        let regex = RegexBuilder::new(&regex_pattern)
            .case_insensitive(case_insensitive)
            .build()
            .map_err(|error| format!("invalid regex pattern: {error}"))?;

        let root = if target.is_dir() {
            target.clone()
        } else {
            target
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_else(|| target.clone())
        };
        let glob_pattern = glob_filter
            .map(normalize_pattern)
            .map(|filter| Pattern::new(&filter).map_err(|error| error.to_string()))
            .transpose()?;
        let mut skipped_binary = 0usize;
        let mut skipped_large = 0usize;
        let mut matching_files = Vec::new();
        let mut counts = Vec::new();
        let mut blocks = Vec::new();
        let mut seen_content_matches = 0usize;
        let mut result_chars = 0usize;
        let mut content_truncated = false;

        for file_path in collect_files(&target)? {
            let relative = file_path
                .strip_prefix(&root)
                .map_err(|error| error.to_string())?
                .to_string_lossy()
                .replace('\\', "/");
            let name = file_path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or_default();
            if let Some(pattern) = &glob_pattern {
                if !match_glob(pattern, &relative, name, glob_filter.unwrap_or_default()) {
                    continue;
                }
            }
            if !matches_type(name, type_filter) {
                continue;
            }
            let metadata = file_path.metadata().map_err(|error| error.to_string())?;
            if metadata.len() > MAX_FILE_BYTES {
                skipped_large += 1;
                continue;
            }
            let raw = fs::read(&file_path).map_err(|error| error.to_string())?;
            if is_binary(&raw) {
                skipped_binary += 1;
                continue;
            }
            let Ok(content) = String::from_utf8(raw) else {
                skipped_binary += 1;
                continue;
            };
            let lines: Vec<&str> = content.lines().collect();
            let display = display_path(&file_path, &root, &self.context);
            let mut count = 0usize;

            for (index, line) in lines.iter().enumerate() {
                if !regex.is_match(line) {
                    continue;
                }
                count += 1;
                if output_mode == "content" {
                    seen_content_matches += 1;
                    if seen_content_matches <= offset {
                        continue;
                    }
                    if let Some(limit) = limit {
                        if blocks.len() >= limit {
                            content_truncated = true;
                            break;
                        }
                    }
                    let block =
                        format_block(&display, &lines, index + 1, context_before, context_after);
                    if result_chars + block.len() > MAX_RESULT_CHARS {
                        content_truncated = true;
                        break;
                    }
                    result_chars += block.len() + 2;
                    blocks.push(block);
                }
            }

            if count > 0 {
                let modified = metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH);
                matching_files.push((display.clone(), modified));
                counts.push((display, count, modified));
            }
            if content_truncated {
                break;
            }
        }

        let mut result = match output_mode {
            "content" => {
                if blocks.is_empty() {
                    format!("No matches found for pattern '{pattern}' in {path}")
                } else {
                    blocks.join("\n\n")
                }
            }
            "count" => render_counts(counts, limit, offset, pattern, path),
            _ => render_matching_files(matching_files, limit, offset, pattern, path),
        };

        let mut notes = Vec::new();
        if content_truncated {
            notes.push("(pagination or output truncation reached)".to_owned());
        }
        if skipped_binary > 0 {
            notes.push(format!(
                "(skipped {skipped_binary} binary/unreadable files)"
            ));
        }
        if skipped_large > 0 {
            notes.push(format!("(skipped {skipped_large} large files)"));
        }
        if !notes.is_empty() {
            result.push_str("\n\n");
            result.push_str(&notes.join("\n"));
        }
        Ok(result)
    }
}

fn collect_entries(
    root: &Path,
    include_files: bool,
    include_dirs: bool,
) -> Result<Vec<PathBuf>, String> {
    let mut entries = Vec::new();
    collect_entries_inner(root, include_files, include_dirs, &mut entries)?;
    Ok(entries)
}

fn collect_entries_inner(
    current: &Path,
    include_files: bool,
    include_dirs: bool,
    entries: &mut Vec<PathBuf>,
) -> Result<(), String> {
    for entry in sorted_read_dir(current)? {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if entry_is_symlink(&entry) || ignored_name(&name) {
            continue;
        }
        if path.is_dir() {
            if include_dirs {
                entries.push(path.clone());
            }
            collect_entries_inner(&path, include_files, include_dirs, entries)?;
        } else if include_files {
            entries.push(path);
        }
    }
    Ok(())
}

fn collect_files(root: &Path) -> Result<Vec<PathBuf>, String> {
    if root.is_file() {
        return Ok(vec![root.to_path_buf()]);
    }
    collect_entries(root, true, false)
}

fn normalize_pattern(pattern: &str) -> String {
    pattern.trim().replace('\\', "/")
}

fn match_glob(pattern: &Pattern, rel_path: &str, name: &str, raw_pattern: &str) -> bool {
    let normalized = normalize_pattern(raw_pattern);
    if normalized.contains('/') || normalized.starts_with("**") {
        pattern.matches(rel_path)
    } else {
        pattern.matches(name)
    }
}

fn display_path(target: &Path, root: &Path, context: &PathContext) -> String {
    context
        .workspace
        .as_ref()
        .and_then(|workspace| target.strip_prefix(workspace).ok())
        .or_else(|| target.strip_prefix(root).ok())
        .map(|path| path.to_string_lossy().replace('\\', "/"))
        .unwrap_or_else(|| target.to_string_lossy().replace('\\', "/"))
}

fn limit_from(
    head_limit: Option<usize>,
    legacy_limit: Option<usize>,
    default_limit: usize,
) -> Option<usize> {
    match (head_limit, legacy_limit) {
        (Some(0), _) => None,
        (Some(value), _) => Some(value),
        (None, Some(value)) => Some(value),
        (None, None) => Some(default_limit),
    }
}

fn render_page(items: Vec<String>, limit: Option<usize>, offset: usize) -> String {
    let truncated = limit.is_some_and(|limit| items.len() > offset.saturating_add(limit));
    let end = limit.map_or(items.len(), |limit| {
        offset.saturating_add(limit).min(items.len())
    });
    let start = offset.min(items.len());
    let mut result = items[start..end].join("\n");
    if truncated {
        if let Some(limit) = limit {
            result.push_str(&format!("\n\n(pagination: limit={limit}, offset={offset})"));
        }
    } else if offset > 0 {
        result.push_str(&format!("\n\n(pagination: offset={offset})"));
    }
    result
}

fn matches_type(name: &str, file_type: Option<&str>) -> bool {
    let Some(file_type) = file_type.map(str::trim).filter(|value| !value.is_empty()) else {
        return true;
    };
    let patterns: &[&str] = match file_type.to_ascii_lowercase().as_str() {
        "py" | "python" => &["*.py", "*.pyi"],
        "js" => &["*.js", "*.jsx", "*.mjs", "*.cjs"],
        "ts" => &["*.ts", "*.tsx", "*.mts", "*.cts"],
        "tsx" => &["*.tsx"],
        "jsx" => &["*.jsx"],
        "json" => &["*.json"],
        "md" | "markdown" => &["*.md", "*.mdx"],
        "go" => &["*.go"],
        "rs" | "rust" => &["*.rs"],
        "java" => &["*.java"],
        "sh" => &["*.sh", "*.bash"],
        "yaml" | "yml" => &["*.yaml", "*.yml"],
        "toml" => &["*.toml"],
        "sql" => &["*.sql"],
        "html" => &["*.html", "*.htm"],
        "css" => &["*.css", "*.scss", "*.sass"],
        _ => return name.ends_with(&format!(".{file_type}")),
    };
    patterns.iter().any(|pattern| {
        Pattern::new(pattern)
            .map(|pattern| pattern.matches(name))
            .unwrap_or(false)
    })
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

fn format_block(
    path: &str,
    lines: &[&str],
    match_line: usize,
    before: usize,
    after: usize,
) -> String {
    let start = match_line.saturating_sub(before).max(1);
    let end = (match_line + after).min(lines.len());
    let mut block = vec![format!("{path}:{match_line}")];
    for line_no in start..=end {
        let marker = if line_no == match_line { '>' } else { ' ' };
        block.push(format!("{marker} {line_no}| {}", lines[line_no - 1]));
    }
    block.join("\n")
}

fn usize_param(params: &JsonMap, name: &str, default: usize) -> usize {
    params
        .get(name)
        .and_then(Value::as_u64)
        .and_then(|value| usize::try_from(value).ok())
        .unwrap_or(default)
}

fn render_matching_files(
    mut files: Vec<(String, SystemTime)>,
    limit: Option<usize>,
    offset: usize,
    pattern: &str,
    path: &str,
) -> String {
    if files.is_empty() {
        return format!("No matches found for pattern '{pattern}' in {path}");
    }
    files.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(&right.0)));
    render_page(
        files.into_iter().map(|(name, _)| name).collect(),
        limit,
        offset,
    )
}

fn render_counts(
    mut counts: Vec<(String, usize, SystemTime)>,
    limit: Option<usize>,
    offset: usize,
    pattern: &str,
    path: &str,
) -> String {
    if counts.is_empty() {
        return format!("No matches found for pattern '{pattern}' in {path}");
    }
    counts.sort_by(|left, right| right.2.cmp(&left.2).then_with(|| left.0.cmp(&right.0)));
    let total_matches = counts.iter().map(|(_, count, _)| count).sum::<usize>();
    let total_files = counts.len();
    let mut result = render_page(
        counts
            .into_iter()
            .map(|(name, count, _)| format!("{name}: {count}"))
            .collect(),
        limit,
        offset,
    );
    result.push_str(&format!(
        "\n\n(total matches: {total_matches} in {total_files} files)"
    ));
    result
}
