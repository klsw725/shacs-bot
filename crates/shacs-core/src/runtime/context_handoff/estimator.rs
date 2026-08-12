use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenEstimatorSelection {
    pub name: String,
    pub provider: String,
    pub model: String,
    pub chars_per_token: usize,
    pub uncertainty_percent: u8,
}

pub fn select_token_estimator(provider: &str, model: &str) -> TokenEstimatorSelection {
    let normalized = provider.to_ascii_lowercase();
    let (name, chars_per_token, uncertainty_percent) = match normalized.as_str() {
        "anthropic" => ("estimator:anthropic_chars_v1", 3, 20),
        "openai" | "azure_openai" | "codex" => ("estimator:openai_chars_v1", 4, 25),
        "google" | "gemini" => ("estimator:google_chars_v1", 4, 30),
        _ => ("estimator:generic_chars_v1", 4, 50),
    };
    TokenEstimatorSelection {
        name: name.to_owned(),
        provider: provider.to_owned(),
        model: model.to_owned(),
        chars_per_token,
        uncertainty_percent,
    }
}

impl TokenEstimatorSelection {
    pub fn estimate(&self, content: &str) -> usize {
        content
            .chars()
            .count()
            .div_ceil(self.chars_per_token)
            .max(content.split_whitespace().count())
    }
}
