use crate::runtime::{ToolSearchMode, ToolSearchRuntimeInput};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

const BRIDGE_TOOL_NAMES: [&str; 3] = ["tool_search", "tool_describe", "tool_call"];

#[derive(Debug, Clone, PartialEq)]
pub struct ToolSurfaceAssemblyInput {
    pub definitions: Vec<Value>,
    pub runtime: ToolSearchRuntimeInput,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ToolSurfaceAssembly {
    pub provider_tools: Vec<Value>,
    pub activation_state: ActivationState,
    pub catalog: Option<DeferredToolCatalog>,
    pub deferrable_schema_tokens: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActivationState {
    PassThrough,
    Activated,
    CollisionPassThrough {
        tool_name: String,
    },
    ThresholdPassThrough {
        estimated_tokens: usize,
        threshold_tokens: usize,
    },
    UnknownContextPassThrough {
        estimated_tokens: usize,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct DeferredToolCatalog {
    pub entries: Vec<DeferredToolCatalogEntry>,
    pub scope_digest: String,
    pub default_limit: usize,
    pub max_limit: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DeferredToolCatalogEntry {
    pub name: String,
    pub description: String,
    pub parameter_names: Vec<String>,
    pub full_schema: Value,
    pub source_kind: String,
    pub source_name: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ToolSearchMatch {
    pub name: String,
    pub short_description: String,
    pub source_kind: String,
    pub source_name: String,
    pub rank: usize,
    pub score: usize,
}

pub fn assemble_tool_surface(input: ToolSurfaceAssemblyInput) -> ToolSurfaceAssembly {
    let deferrable_schema_tokens = estimate_deferrable_schema_tokens(&input.definitions);
    if input.runtime.config.enabled == ToolSearchMode::Off {
        return pass_through(
            input.definitions,
            ActivationState::PassThrough,
            deferrable_schema_tokens,
        );
    }

    if let Some(tool_name) = bridge_name_collision(&input.definitions) {
        return pass_through(
            input.definitions,
            ActivationState::CollisionPassThrough { tool_name },
            deferrable_schema_tokens,
        );
    }

    let mut visible = Vec::new();
    let mut deferred = Vec::new();
    for definition in &input.definitions {
        if is_deferrable(definition) {
            deferred.push(catalog_entry(definition));
        } else {
            visible.push(definition.clone());
        }
    }

    if deferred.is_empty() {
        return pass_through(
            input.definitions,
            ActivationState::PassThrough,
            deferrable_schema_tokens,
        );
    }

    let activation_state = match input.runtime.config.enabled {
        ToolSearchMode::Off => ActivationState::PassThrough,
        ToolSearchMode::On => ActivationState::Activated,
        ToolSearchMode::Auto => {
            let Some(context_window_tokens) = input.runtime.context_window_tokens else {
                return pass_through(
                    input.definitions,
                    ActivationState::UnknownContextPassThrough {
                        estimated_tokens: deferrable_schema_tokens,
                    },
                    deferrable_schema_tokens,
                );
            };
            let threshold_tokens = threshold_tokens(
                context_window_tokens,
                input.runtime.config.threshold_pct as usize,
            );
            if deferrable_schema_tokens < threshold_tokens {
                return pass_through(
                    input.definitions,
                    ActivationState::ThresholdPassThrough {
                        estimated_tokens: deferrable_schema_tokens,
                        threshold_tokens,
                    },
                    deferrable_schema_tokens,
                );
            }
            ActivationState::Activated
        }
    };

    let catalog = DeferredToolCatalog::new(
        deferred,
        input.runtime.config.search_default_limit,
        input.runtime.config.max_search_limit,
    );
    visible.extend(bridge_tool_schemas());
    ToolSurfaceAssembly {
        provider_tools: visible,
        activation_state,
        catalog: Some(catalog),
        deferrable_schema_tokens,
    }
}

impl DeferredToolCatalog {
    pub fn new(
        entries: Vec<DeferredToolCatalogEntry>,
        default_limit: usize,
        max_limit: usize,
    ) -> Self {
        let scope_digest = scope_digest(&entries);
        Self {
            entries,
            scope_digest,
            default_limit,
            max_limit,
        }
    }

    pub fn search(&self, query: &str, limit: Option<usize>) -> Vec<ToolSearchMatch> {
        let query = query.trim();
        if query.is_empty() {
            return Vec::new();
        }
        let effective_limit = limit
            .unwrap_or(self.default_limit)
            .min(self.max_limit)
            .max(1);
        let tokens = query_tokens(query);
        let mut scored = self
            .entries
            .iter()
            .enumerate()
            .filter_map(|(index, entry)| {
                let score = score_entry(entry, &tokens);
                (score > 0).then_some((index, entry, score))
            })
            .collect::<Vec<_>>();

        if scored.is_empty() {
            let lowered = query.to_ascii_lowercase();
            scored = self
                .entries
                .iter()
                .enumerate()
                .filter(|(_, entry)| entry.name.to_ascii_lowercase().contains(&lowered))
                .map(|(index, entry)| (index, entry, 1))
                .collect();
        }

        scored.sort_by(|left, right| right.2.cmp(&left.2).then_with(|| left.0.cmp(&right.0)));
        scored
            .into_iter()
            .take(effective_limit)
            .enumerate()
            .map(|(rank, (_, entry, score))| ToolSearchMatch {
                name: entry.name.clone(),
                short_description: short_description(&entry.description),
                source_kind: entry.source_kind.clone(),
                source_name: entry.source_name.clone(),
                rank: rank + 1,
                score,
            })
            .collect()
    }

    pub fn source_kind_counts(&self) -> BTreeMap<String, usize> {
        let mut counts = BTreeMap::new();
        for entry in &self.entries {
            *counts.entry(entry.source_kind.clone()).or_insert(0) += 1;
        }
        counts
    }
}

pub fn bridge_tool_schemas() -> Vec<Value> {
    vec![
        tool_search_schema(),
        tool_describe_schema(),
        tool_call_schema(),
    ]
}

pub fn bridge_tool_names() -> &'static [&'static str; 3] {
    &BRIDGE_TOOL_NAMES
}

pub fn estimate_serialized_schema_tokens(schema: &Value) -> usize {
    schema.to_string().chars().count().saturating_add(3) / 4
}

fn pass_through(
    definitions: Vec<Value>,
    activation_state: ActivationState,
    deferrable_schema_tokens: usize,
) -> ToolSurfaceAssembly {
    ToolSurfaceAssembly {
        provider_tools: definitions,
        activation_state,
        catalog: None,
        deferrable_schema_tokens,
    }
}

fn bridge_name_collision(definitions: &[Value]) -> Option<String> {
    definitions.iter().find_map(|definition| {
        let name = schema_name(definition)?;
        BRIDGE_TOOL_NAMES.contains(&name.as_str()).then_some(name)
    })
}

fn estimate_deferrable_schema_tokens(definitions: &[Value]) -> usize {
    definitions
        .iter()
        .filter(|definition| is_deferrable(definition))
        .map(estimate_serialized_schema_tokens)
        .sum()
}

fn is_deferrable(definition: &Value) -> bool {
    schema_name(definition).is_some_and(|name| name.starts_with("mcp_"))
}

fn catalog_entry(definition: &Value) -> DeferredToolCatalogEntry {
    let name = schema_name(definition).unwrap_or_default();
    let description = schema_description(definition).unwrap_or_default();
    let parameter_names = parameter_names(definition);
    let (source_kind, source_name) = source_from_name(&name);
    DeferredToolCatalogEntry {
        name,
        description,
        parameter_names,
        full_schema: definition.clone(),
        source_kind,
        source_name,
    }
}

fn schema_name(definition: &Value) -> Option<String> {
    definition
        .get("function")
        .and_then(Value::as_object)
        .and_then(|function| function.get("name"))
        .and_then(Value::as_str)
        .or_else(|| definition.get("name").and_then(Value::as_str))
        .map(str::to_owned)
}

fn schema_description(definition: &Value) -> Option<String> {
    definition
        .get("function")
        .and_then(Value::as_object)
        .and_then(|function| function.get("description"))
        .and_then(Value::as_str)
        .or_else(|| definition.get("description").and_then(Value::as_str))
        .map(str::to_owned)
}

fn parameter_names(definition: &Value) -> Vec<String> {
    definition
        .get("function")
        .and_then(Value::as_object)
        .and_then(|function| function.get("parameters"))
        .or_else(|| definition.get("parameters"))
        .and_then(|parameters| parameters.get("properties"))
        .and_then(Value::as_object)
        .map(|properties| properties.keys().cloned().collect())
        .unwrap_or_default()
}

fn source_from_name(name: &str) -> (String, String) {
    let Some(rest) = name.strip_prefix("mcp_") else {
        return ("unknown".to_owned(), "unknown".to_owned());
    };
    if let Some(index) = rest.find("_resource_") {
        return ("mcp_resource".to_owned(), rest[..index].to_owned());
    }
    if let Some(index) = rest.find("_prompt_") {
        return ("mcp_prompt".to_owned(), rest[..index].to_owned());
    }
    let source_name = rest.split('_').next().unwrap_or("unknown").to_owned();
    ("mcp_tool".to_owned(), source_name)
}

fn scope_digest(entries: &[DeferredToolCatalogEntry]) -> String {
    let mut hasher = Sha256::new();
    for entry in entries {
        hasher.update(entry.name.as_bytes());
        hasher.update([0]);
    }
    format!("sha256:{:x}", hasher.finalize())
}

fn threshold_tokens(context_window_tokens: usize, threshold_pct: usize) -> usize {
    context_window_tokens
        .saturating_mul(threshold_pct)
        .saturating_add(99)
        / 100
}

fn query_tokens(query: &str) -> Vec<String> {
    let tokens = query
        .split(|character: char| !character.is_ascii_alphanumeric())
        .filter(|token| !token.is_empty())
        .map(str::to_ascii_lowercase)
        .collect::<Vec<_>>();
    if tokens.is_empty() {
        vec![query.to_ascii_lowercase()]
    } else {
        tokens
    }
}

fn score_entry(entry: &DeferredToolCatalogEntry, tokens: &[String]) -> usize {
    let name = entry.name.to_ascii_lowercase();
    let name_tokens = query_tokens(&entry.name);
    let description = entry.description.to_ascii_lowercase();
    let parameters = entry
        .parameter_names
        .iter()
        .map(|parameter| parameter.to_ascii_lowercase())
        .collect::<Vec<_>>();
    tokens
        .iter()
        .map(|token| {
            let mut score = 0;
            if name == *token {
                score += 100;
            }
            if name_tokens.iter().any(|name_token| name_token == token) {
                score += 20;
            }
            if parameters.iter().any(|parameter| parameter == token) {
                score += 10;
            }
            if parameters.iter().any(|parameter| parameter.contains(token)) {
                score += 5;
            }
            if description.contains(token) {
                score += 5;
            }
            score
        })
        .sum()
}

fn short_description(description: &str) -> String {
    let collapsed = description.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.chars().count() <= 160 {
        return collapsed;
    }
    let mut short = collapsed.chars().take(157).collect::<String>();
    short.push_str("...");
    short
}

fn tool_search_schema() -> Value {
    json!({
        "type": "function",
        "function": {
            "name": "tool_search",
            "description": "Search deferred tools by name, description, and top-level parameter names.",
            "parameters": {
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "Search query." },
                    "limit": { "type": "integer", "description": "Maximum number of matches to return.", "minimum": 1 }
                },
                "required": ["query"]
            }
        }
    })
}

fn tool_describe_schema() -> Value {
    json!({
        "type": "function",
        "function": {
            "name": "tool_describe",
            "description": "Return the full schema for one deferred tool by name.",
            "parameters": {
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "Deferred tool name." }
                },
                "required": ["name"]
            }
        }
    })
}

fn tool_call_schema() -> Value {
    json!({
        "type": "function",
        "function": {
            "name": "tool_call",
            "description": "Call one deferred tool by name with JSON arguments.",
            "parameters": {
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "Deferred tool name." },
                    "arguments": { "type": "object", "description": "Arguments for the deferred tool." }
                },
                "required": ["name", "arguments"]
            }
        }
    })
}
