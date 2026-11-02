use super::{take_path, take_value, ArgParser, CliCommand, CliError};
use serde_json::Value;
use shacs_config::{load_config, ConfigBundle, LoadOptions};
use shacs_core::memory_search::{
    self, EmbedderLoader, HuggingFaceSource, IndexError, IndexStatus, InstalledFile, MemoryIndex,
    MemorySearch, MemorySearchPaths, MemorySearchSettings, ModelManifest, ReconcileError,
    VerifiedModelLoader, DEFAULT_MEMORY_MODEL_ID, DEFAULT_SEARCH_RESULTS, MAX_SEARCH_RESULTS,
    MODEL_MANIFESTS,
};
use shacs_core::tools::NetworkGuard;
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MemoryOptions {
    ModelInstall {
        config_path: Option<PathBuf>,
        model: String,
    },
    Index {
        config_path: Option<PathBuf>,
        workspace_override: Option<PathBuf>,
    },
    Search {
        config_path: Option<PathBuf>,
        workspace_override: Option<PathBuf>,
        query: String,
        limit: usize,
        json: bool,
    },
}

pub(super) fn parse(
    mut parser: ArgParser,
    global_config: Option<PathBuf>,
) -> Result<CliCommand, CliError> {
    match parser.next().as_deref() {
        Some("model") => match parser.next().as_deref() {
            Some("install") => parse_model_install(parser, global_config),
            Some(other) => Err(CliError::InvalidArguments(format!(
                "unknown memory model action `{other}`"
            ))),
            None => Err(CliError::InvalidArguments(
                "memory model requires `install`".to_owned(),
            )),
        },
        Some("index") => parse_index(parser, global_config),
        Some("search") => parse_search(parser, global_config),
        Some("--help" | "-h") => Ok(CliCommand::Help),
        Some(other) => Err(CliError::InvalidArguments(format!(
            "unknown memory action `{other}`"
        ))),
        None => Err(CliError::InvalidArguments(
            "memory requires `model install`, `index` or `search`".to_owned(),
        )),
    }
}

fn parse_model_install(
    mut parser: ArgParser,
    global_config: Option<PathBuf>,
) -> Result<CliCommand, CliError> {
    let mut config_path = global_config;
    let mut model = DEFAULT_MEMORY_MODEL_ID.to_owned();
    while let Some(arg) = parser.next() {
        match arg.as_str() {
            "--config" | "-c" => config_path = Some(take_path(&mut parser, &arg)?),
            "--model" => model = take_value(&mut parser, &arg)?,
            other => {
                return Err(CliError::InvalidArguments(format!(
                    "unknown memory model install argument `{other}`"
                )))
            }
        }
    }
    Ok(CliCommand::Memory(MemoryOptions::ModelInstall {
        config_path,
        model,
    }))
}

fn parse_index(
    mut parser: ArgParser,
    global_config: Option<PathBuf>,
) -> Result<CliCommand, CliError> {
    let mut config_path = global_config;
    let mut workspace_override = None;
    while let Some(arg) = parser.next() {
        match arg.as_str() {
            "--config" | "-c" => config_path = Some(take_path(&mut parser, &arg)?),
            "--workspace" | "-w" => workspace_override = Some(take_path(&mut parser, &arg)?),
            other => {
                return Err(CliError::InvalidArguments(format!(
                    "unknown memory index argument `{other}`"
                )))
            }
        }
    }
    Ok(CliCommand::Memory(MemoryOptions::Index {
        config_path,
        workspace_override,
    }))
}

fn parse_search(
    mut parser: ArgParser,
    global_config: Option<PathBuf>,
) -> Result<CliCommand, CliError> {
    let mut config_path = global_config;
    let mut workspace_override = None;
    let mut query = None;
    let mut limit = DEFAULT_SEARCH_RESULTS;
    let mut json = false;
    while let Some(arg) = parser.next() {
        match arg.as_str() {
            "--config" | "-c" => config_path = Some(take_path(&mut parser, &arg)?),
            "--workspace" | "-w" => workspace_override = Some(take_path(&mut parser, &arg)?),
            "--limit" => limit = parse_limit(&take_value(&mut parser, &arg)?)?,
            "--json" => json = true,
            other if query.is_none() && !other.starts_with('-') => query = Some(other.to_owned()),
            other => {
                return Err(CliError::InvalidArguments(format!(
                    "unknown memory search argument `{other}`"
                )))
            }
        }
    }
    Ok(CliCommand::Memory(MemoryOptions::Search {
        config_path,
        workspace_override,
        query: query.ok_or_else(|| {
            CliError::InvalidArguments("memory search requires a query".to_owned())
        })?,
        limit,
        json,
    }))
}

fn parse_limit(value: &str) -> Result<usize, CliError> {
    value
        .parse::<usize>()
        .ok()
        .filter(|limit| (1..=MAX_SEARCH_RESULTS).contains(limit))
        .ok_or_else(|| {
            CliError::InvalidArguments(format!(
                "`--limit` must be a number from 1 to {MAX_SEARCH_RESULTS}, not `{value}`"
            ))
        })
}

pub(super) fn run(options: MemoryOptions) -> Result<String, CliError> {
    match options {
        MemoryOptions::ModelInstall { config_path, model } => install(config_path, &model),
        MemoryOptions::Index {
            config_path,
            workspace_override,
        } => index(&load(config_path, workspace_override)?),
        MemoryOptions::Search {
            config_path,
            workspace_override,
            query,
            limit,
            json,
        } => {
            let result = search(&load(config_path, workspace_override)?, &query, limit)?;
            if json {
                Ok(result)
            } else {
                readable_result(&result)
            }
        }
    }
}

fn load(
    config_path: Option<PathBuf>,
    workspace_override: Option<PathBuf>,
) -> Result<ConfigBundle, CliError> {
    Ok(load_config(LoadOptions {
        config_path,
        workspace_override,
        ..LoadOptions::default()
    })?)
}

fn install(config_path: Option<PathBuf>, model: &str) -> Result<String, CliError> {
    let manifest = known_model(model)?;
    let bundle = load(config_path, None)?;
    let paths = memory_search_paths(&bundle.context.data_dir)?;
    let source = HuggingFaceSource::new(NetworkGuard::with_ssrf_whitelist(
        bundle
            .config
            .tools
            .ssrf_whitelist
            .iter()
            .map(String::as_str),
    ));
    let report = memory_search::install_model(&paths, manifest, &source)
        .map_err(|error| CliError::Runtime(error.to_string()))?;
    let mut lines = vec![format!("model {}@{}", manifest.id, manifest.revision)];
    for (file, state) in &report.files {
        let state = match state {
            InstalledFile::AlreadyVerified => "already verified",
            InstalledFile::Downloaded => "downloaded and verified",
        };
        lines.push(format!("  {file}: {state}"));
    }
    lines.push(format!("folder: {}", report.model_dir.display()));
    Ok(lines.join("\n"))
}

fn index(bundle: &ConfigBundle) -> Result<String, CliError> {
    let config = &bundle.config.agents.defaults.memory_search;
    let manifest = memory_search::configured_model(config).map_err(CliError::Config)?;
    let paths = memory_search_paths(&bundle.context.data_dir)?;
    let loader = VerifiedModelLoader::new(paths.clone(), manifest);
    let mut index = MemoryIndex::reconcile(&paths, &bundle.context.workspace, &loader.model())
        .map_err(reconcile_failure)?;
    if !config.enable {
        return Ok(format!(
            "{}\nsemantic search is off (agents.defaults.memorySearch.enable is false), so only \
             the text index was updated",
            status_lines(&current_status(&index)?)
        ));
    }
    let (status, unembedded) = match loader.load() {
        Ok(embedder) => match index.complete_vectors(embedder.as_ref()) {
            Ok(status) => (status, None),
            Err(IndexError::Reconcile(error)) => return Err(reconcile_failure(*error)),
            Err(error) => (current_status(&index)?, Some(error.to_string())),
        },
        Err(error) => (
            current_status(&index)?,
            Some(format!(
                "{error}; run `shacs-bot memory model install --model {}` first",
                manifest.id
            )),
        ),
    };
    match unembedded {
        None => Ok(status_lines(&status)),
        Some(reason) => Err(CliError::Runtime(format!(
            "{}\nvectors were not computed: {reason}",
            status_lines(&status)
        ))),
    }
}

fn search(bundle: &ConfigBundle, query: &str, limit: usize) -> Result<String, CliError> {
    let settings = MemorySearchSettings::from_config(&bundle.config.agents.defaults)
        .map_err(CliError::Config)?;
    let paths = memory_search_paths(&bundle.context.data_dir)?;
    Ok(
        MemorySearch::configured(&settings, paths, bundle.context.workspace.clone())
            .search(query, limit),
    )
}

fn readable_result(json: &str) -> Result<String, CliError> {
    let result: Value = serde_json::from_str(json)
        .map_err(|error| CliError::Runtime(format!("memory search result is not JSON: {error}")))?;
    let text = |value: &Value, key: &str| -> String {
        match value.get(key) {
            Some(Value::String(text)) => text.clone(),
            Some(other) => other.to_string(),
            None => String::new(),
        }
    };
    let mut lines = Vec::new();
    let results = result.get("results").and_then(Value::as_array);
    for item in results.into_iter().flatten() {
        let source = item.get("source").unwrap_or(&Value::Null);
        let place = if source.get("cursor").is_some() {
            format!(
                "{} cursor {} ({}) item {}",
                text(source, "file"),
                text(source, "cursor"),
                text(source, "timestamp"),
                text(source, "item")
            )
        } else {
            format!(
                "{} lines {}-{}",
                text(source, "file"),
                text(source, "first_line"),
                text(source, "last_line")
            )
        };
        lines.push(format!(
            "{}. [{}] {place} (key {})",
            text(item, "rank"),
            text(item, "matched_by"),
            text(item, "key")
        ));
        lines.extend(text(item, "text").lines().map(|line| format!("   {line}")));
        if let Some(original) = item.pointer("/text_truncated/original_chars") {
            lines.push(format!(
                "   (text cut short; {original} characters in full)"
            ));
        }
    }
    if results.is_none_or(Vec::is_empty) {
        lines.push("no results".to_owned());
    }
    let semantic = result.get("semantic").unwrap_or(&Value::Null);
    let mut semantic_line = format!("semantic: {}", text(semantic, "status"));
    if semantic.get("reason").is_some() {
        semantic_line.push_str(&format!(": {}", text(semantic, "reason")));
    }
    if semantic.get("detail").is_some() {
        semantic_line.push_str(&format!(" ({})", text(semantic, "detail")));
    }
    lines.push(semantic_line);
    let index = result.get("index").unwrap_or(&Value::Null);
    let mut index_line = format!("index: {}", text(index, "status"));
    for (key, label) in [
        ("reason", "reason"),
        ("vectors_remaining", "vectors remaining"),
        ("vectors_failed", "vectors failed"),
        ("skipped_history_lines", "skipped history lines"),
    ] {
        if index.get(key).is_some() {
            index_line.push_str(&format!("; {label}: {}", text(index, key)));
        }
    }
    lines.push(index_line);
    if let Some(error) = result.get("error") {
        lines.push(format!(
            "error: {}: {}",
            text(error, "code"),
            text(error, "detail")
        ));
    }
    Ok(lines.join("\n"))
}

fn status_lines(status: &IndexStatus) -> String {
    [
        format!("model: {}@{}", status.model.id, status.model.revision),
        format!("items: {}", status.items),
        format!("vectors remaining: {}", status.vectors_remaining),
        format!("vectors failed: {}", status.vectors_failed),
        format!("skipped history lines: {}", status.skipped_history_lines),
        format!("index generation: {}", status.generation),
    ]
    .join("\n")
}

fn reconcile_failure(error: ReconcileError) -> CliError {
    match error {
        ReconcileError::SourceUnreadable(error) => CliError::Runtime(error.to_string()),
        ReconcileError::IndexUnavailable { snapshot, reason } => CliError::Runtime(format!(
            "memory index is unavailable: {reason}; the memory files hold {} items",
            snapshot.items.len()
        )),
    }
}

fn current_status(index: &MemoryIndex) -> Result<IndexStatus, CliError> {
    index
        .status()
        .map_err(|error| CliError::Runtime(error.to_string()))
}

fn known_model(id: &str) -> Result<&'static ModelManifest, CliError> {
    memory_search::model_manifest(id).ok_or_else(|| {
        let known: Vec<&str> = MODEL_MANIFESTS.iter().map(|manifest| manifest.id).collect();
        CliError::InvalidArguments(format!(
            "unknown memory model `{id}`; choose one of {}",
            known.join(", ")
        ))
    })
}

fn memory_search_paths(data_dir: &std::path::Path) -> Result<MemorySearchPaths, CliError> {
    fs::create_dir_all(data_dir)?;
    Ok(MemorySearchPaths::new(data_dir)?)
}

#[cfg(test)]
#[path = "memory_cli_tests.rs"]
mod tests;
