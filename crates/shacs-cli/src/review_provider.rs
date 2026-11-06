//! What `permissions auto-mode critique` and `setup` share (Spec 048 PRD
//! 050 scopes 6 and 7): one request to the provider and model `ask` uses,
//! and request text cut to fixed shares after 022 redaction and `.env.json`
//! masking.

use crate::{CliError, RuntimeConfigOptions};
use serde_json::json;
use shacs_core::runtime::SecretValues;
use shacs_providers::{GenerationSettings, ProviderClient, ProviderInvocation, ProviderRequest};
use shacs_redaction::redact_string;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// The longest written entry a request carries, as JSON text.
pub(crate) const MAX_ENTRY_CHARS: usize = 1_000;
const CUT: &str = " [cut]";
const DEADLINE: Duration = Duration::from_secs(60);

pub(crate) struct ReviewProvider {
    pub(crate) client: Arc<dyn ProviderClient>,
    pub(crate) model: String,
}

impl ReviewProvider {
    /// The provider settings and credential `ask` uses; the model is
    /// `classifierModel` when it is written, with `${NAME}` resolved like
    /// every config text.
    pub(crate) fn for_config(
        config_path: Option<PathBuf>,
        workspace_override: Option<PathBuf>,
    ) -> Result<Self, CliError> {
        let bundle = crate::load_config_for_reading(RuntimeConfigOptions {
            config_path,
            workspace_override,
            resolve_env: true,
        })?;
        let (client, model) = crate::read_only_provider_client(&bundle);
        let classifier_model = bundle.config.permissions.auto_mode.classifier_model;
        Ok(Self {
            client,
            model: classifier_model.unwrap_or(model),
        })
    }

    /// One request under a 60-second limit. A provider error, a timeout or
    /// an empty answer is an error.
    pub(crate) fn answer(
        &self,
        system: &str,
        user: String,
        max_tokens: u32,
    ) -> Result<String, CliError> {
        let request = ProviderRequest {
            messages: vec![
                json!({"role": "system", "content": system}),
                json!({"role": "user", "content": user}),
            ],
            tools: Vec::new(),
            model: self.model.clone(),
            settings: GenerationSettings {
                temperature: 0.0,
                max_tokens,
                reasoning_effort: None,
            },
            tool_choice: None,
        };
        let invocation =
            ProviderInvocation::uncancelled(None).with_deadline(Instant::now() + DEADLINE);
        let response = self
            .client
            .chat_with_invocation(request, &invocation)
            .map_err(CliError::Provider)?;
        if response.finish_reason == "error" {
            return Err(CliError::Runtime(format!(
                "the model request failed: {}",
                redact_string(response.content.as_deref().unwrap_or("no reason given"))
            )));
        }
        response
            .content
            .filter(|content| !content.trim().is_empty())
            .ok_or_else(|| CliError::Runtime("the model gave no answer".to_owned()))
    }
}

/// Text the provider may see. `.env.json` values are masked before 022
/// redaction, which could otherwise cut a value and leave part of it.
pub(crate) fn shown(secrets: &SecretValues, text: &str) -> String {
    redact_string(&secrets.mask(text))
}

/// The characters `text` takes as a JSON string, quotes included.
pub(crate) fn json_chars(text: &str) -> usize {
    2 + text.chars().map(escaped_chars).sum::<usize>()
}

fn escaped_chars(character: char) -> usize {
    match character {
        '"' | '\\' | '\n' | '\r' | '\t' | '\u{8}' | '\u{c}' => 2,
        '\u{0}'..='\u{1f}' => 6,
        _ => 1,
    }
}

/// `text` cut at the end so that its JSON string takes at most `limit`
/// characters, ` [cut]` included.
pub(crate) fn cut_entry(text: &str, limit: usize) -> String {
    if json_chars(text) <= limit {
        return text.to_owned();
    }
    let mut kept = front(text, limit.saturating_sub(json_chars(CUT)));
    kept.push_str(CUT);
    kept
}

/// The front of `text` whose JSON string takes at most `limit` characters,
/// and whether anything was cut.
pub(crate) fn fit_text(text: &str, limit: usize) -> (String, bool) {
    if json_chars(text) <= limit {
        return (text.to_owned(), false);
    }
    (front(text, limit.saturating_sub(2)), true)
}

fn front(text: &str, mut budget: usize) -> String {
    let mut kept = String::new();
    for character in text.chars() {
        let cost = escaped_chars(character);
        if cost > budget {
            break;
        }
        budget -= cost;
        kept.push(character);
    }
    kept
}

/// Entries from the front while the list's JSON takes at most `limit`
/// characters; the ones left out are counted in a last entry that fits too.
pub(crate) fn fit_list(entries: Vec<String>, limit: usize) -> Vec<String> {
    let whole = 2
        + entries.iter().map(|entry| json_chars(entry)).sum::<usize>()
        + entries.len().saturating_sub(1);
    if whole <= limit {
        return entries;
    }
    let count = entries.len();
    let mut kept = Vec::new();
    let mut used = 2;
    for entry in entries {
        let with_entry = used + usize::from(!kept.is_empty()) + json_chars(&entry);
        let left_out = count - kept.len() - 1;
        let marker = (left_out > 0).then(|| 1 + json_chars(&more_entries(left_out)));
        if with_entry + marker.unwrap_or(0) > limit {
            break;
        }
        used = with_entry;
        kept.push(entry);
    }
    let left_out = count - kept.len();
    if left_out > 0 {
        kept.push(more_entries(left_out));
    }
    kept
}

fn more_entries(count: usize) -> String {
    format!("[{count} more entries not shown]")
}

#[cfg(test)]
#[path = "review_provider_tests.rs"]
mod tests;

#[cfg(test)]
pub(crate) struct ScriptedClient {
    answer: Result<String, String>,
    requests: std::sync::Mutex<Vec<ProviderRequest>>,
    before_answer: std::sync::Mutex<Option<Box<dyn FnOnce() + Send>>>,
}

#[cfg(test)]
impl ScriptedClient {
    pub(crate) fn new(answer: Result<&str, &str>) -> Arc<Self> {
        Self::with_hook(answer, None)
    }

    pub(crate) fn with_hook(
        answer: Result<&str, &str>,
        before_answer: Option<Box<dyn FnOnce() + Send>>,
    ) -> Arc<Self> {
        Arc::new(Self {
            answer: answer.map(str::to_owned).map_err(str::to_owned),
            requests: std::sync::Mutex::new(Vec::new()),
            before_answer: std::sync::Mutex::new(before_answer),
        })
    }

    pub(crate) fn provider(self: &Arc<Self>) -> ReviewProvider {
        ReviewProvider {
            client: self.clone(),
            model: "review-model".to_owned(),
        }
    }

    pub(crate) fn user_texts(&self) -> Vec<String> {
        self.requests
            .lock()
            .expect("requests lock poisoned")
            .iter()
            .filter_map(|request| {
                request.messages.get(1)?["content"]
                    .as_str()
                    .map(str::to_owned)
            })
            .collect()
    }
}

#[cfg(test)]
impl ProviderClient for ScriptedClient {
    fn chat(
        &self,
        request: ProviderRequest,
    ) -> Result<shacs_providers::LlmResponse, shacs_providers::ProviderError> {
        self.requests
            .lock()
            .expect("requests lock poisoned")
            .push(request);
        let hook = self
            .before_answer
            .lock()
            .expect("hook lock poisoned")
            .take();
        if let Some(hook) = hook {
            hook();
        }
        match &self.answer {
            Ok(content) => Ok(shacs_providers::LlmResponse {
                content: Some(content.clone()),
                finish_reason: "stop".to_owned(),
                ..shacs_providers::LlmResponse::default()
            }),
            Err(message) => Err(shacs_providers::ProviderError::Api {
                status: None,
                message: message.clone(),
                retryable: false,
                headers: std::collections::BTreeMap::new(),
                body: None,
            }),
        }
    }

    fn chat_stream(
        &self,
        request: ProviderRequest,
        _on_event: &mut dyn FnMut(shacs_providers::ProviderEvent),
    ) -> Result<shacs_providers::LlmResponse, shacs_providers::ProviderError> {
        self.chat(request)
    }
}
