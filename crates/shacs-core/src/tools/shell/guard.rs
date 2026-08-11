use super::ExecTool;
use crate::tools::filesystem::raw_candidate_path;
use regex::Regex;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

impl ExecTool {
    pub(super) fn resolve_working_dir(&self, working_dir: Option<&str>) -> Result<PathBuf, String> {
        let cwd = if let Some(working_dir) = working_dir {
            raw_candidate_path(working_dir, &self.config.path_context)
        } else if let Some(working_dir) = &self.config.working_dir {
            working_dir.clone()
        } else {
            std::env::current_dir().map_err(|error| error.to_string())?
        };
        let cwd = std::fs::canonicalize(&cwd)
            .map_err(|_| "working_dir could not be resolved".to_owned())?;
        if self.workspace_guard_enabled() {
            let Some(workspace) = self.workspace_guard_root() else {
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

    pub(super) fn build_env(&self) -> HashMap<String, String> {
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

    pub(super) fn guard_command(&self, command: &str, cwd: &Path) -> Option<String> {
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
        if self.workspace_guard_enabled() {
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

    fn workspace_guard_enabled(&self) -> bool {
        self.config.restrict_to_workspace || self.config.sandbox.is_none()
    }

    fn workspace_guard_root(&self) -> Option<&PathBuf> {
        self.config
            .working_dir
            .as_ref()
            .or(self.config.path_context.workspace.as_ref())
    }
}

pub(super) fn default_deny_patterns() -> Vec<String> {
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
    for pattern in [
        r#"(?:^|[\s|>'"])(/[^\s"'>;|<]+)"#,
        r#"(?:^|[\s|>'"])(~[^\s"'>;|<]*)"#,
    ] {
        if let Ok(regex) = Regex::new(pattern) {
            paths.extend(
                regex
                    .captures_iter(command)
                    .filter_map(|captures| captures.get(1))
                    .map(|value| value.as_str().to_owned()),
            );
        }
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
        missing.push(current.file_name()?.to_owned());
        if let Ok(mut prefix) = std::fs::canonicalize(parent) {
            for component in missing.iter().rev() {
                prefix.push(component);
            }
            return Some(prefix);
        }
        current = parent;
    }
}
