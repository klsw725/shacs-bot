use shacs_config::{AgentDefaults, ConfigError, MemorySearchConfig};

use super::model_manifest::{self, ModelManifest, MODEL_MANIFESTS};

/// The smallest result limit other than 0 (no limit) a search accepts: its fixed result for an
/// overlong search always fits in it.
pub const MIN_RESULT_CHARS: usize = 1_000;

/// `agents.defaults.memorySearch` and `agents.defaults.maxToolResultChars`, checked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MemorySearchSettings {
    pub manifest: &'static ModelManifest,
    pub semantic: bool,
    pub max_result_chars: usize,
}

impl MemorySearchSettings {
    pub fn from_config(defaults: &AgentDefaults) -> Result<Self, ConfigError> {
        let manifest = configured_model(&defaults.memory_search)?;
        let max_result_chars = defaults.max_tool_result_chars;
        if max_result_chars != 0 && max_result_chars < MIN_RESULT_CHARS {
            return Err(ConfigError::Invalid(format!(
                "agents.defaults.maxToolResultChars is {max_result_chars}; memory_search needs 0 \
                 (no limit) or at least {MIN_RESULT_CHARS}"
            )));
        }
        Ok(Self {
            manifest,
            semantic: defaults.memory_search.enable,
            max_result_chars,
        })
    }
}

/// The manifest of `agents.defaults.memorySearch.model`, which must be a model shacs-bot pins.
pub fn configured_model(
    config: &MemorySearchConfig,
) -> Result<&'static ModelManifest, ConfigError> {
    model_manifest::model_manifest(&config.model).ok_or_else(|| {
        let known: Vec<&str> = MODEL_MANIFESTS.iter().map(|manifest| manifest.id).collect();
        ConfigError::Invalid(format!(
            "agents.defaults.memorySearch.model `{}` is not a known model; choose one of {}",
            config.model,
            known.join(", ")
        ))
    })
}
