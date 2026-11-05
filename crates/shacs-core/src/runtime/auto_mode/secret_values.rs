//! `.env.json` values that what people and providers see shows by name only
//! (Spec 048 PRD 010 scope 10). Decisions use the text before masking.

use serde_json::Value;
use shacs_config::EnvFile;
use std::collections::BTreeSet;
use std::fmt;

/// Shorter values would also hide common words and spoil what they mask.
const MIN_MASKED_CHARS: usize = 8;
/// How a masked value starts: `[secret NAME]`.
pub(crate) const MASK_PREFIX: &str = "[secret ";

#[derive(Clone, Default, PartialEq, Eq)]
pub struct SecretValues {
    /// Value and name, longest value first; a value under several names
    /// keeps the first name.
    entries: Vec<(String, String)>,
}

impl fmt::Debug for SecretValues {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SecretValues")
            .field(
                "names",
                &self
                    .entries
                    .iter()
                    .map(|(_, name)| name)
                    .collect::<Vec<_>>(),
            )
            .finish()
    }
}

impl SecretValues {
    pub fn new(env_file: &EnvFile) -> Self {
        let mut seen = BTreeSet::new();
        let mut entries = env_file
            .entries()
            .filter(|(_, value)| value.chars().count() >= MIN_MASKED_CHARS)
            .filter(|(_, value)| seen.insert(*value))
            .map(|(name, value)| (value.to_owned(), name.to_owned()))
            .collect::<Vec<_>>();
        entries.sort_by_key(|(value, _)| std::cmp::Reverse(value.len()));
        Self { entries }
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Replaces each value with `[secret NAME]` in one pass, taking the
    /// longest value that starts at each place and never looking at
    /// replaced text again.
    pub fn mask(&self, text: &str) -> String {
        if self.entries.is_empty() {
            return text.to_owned();
        }
        let mut masked = String::with_capacity(text.len());
        let mut rest = text;
        while let Some(character) = rest.chars().next() {
            if let Some((value, name)) = self
                .entries
                .iter()
                .find(|(value, _)| rest.starts_with(value.as_str()))
            {
                masked.push_str(MASK_PREFIX);
                masked.push_str(name);
                masked.push(']');
                rest = &rest[value.len()..];
            } else {
                masked.push(character);
                rest = &rest[character.len_utf8()..];
            }
        }
        masked
    }

    /// Masks every text in a JSON value, keys included.
    pub fn mask_value(&self, value: &Value) -> Value {
        if self.entries.is_empty() {
            return value.clone();
        }
        match value {
            Value::String(text) => Value::String(self.mask(text)),
            Value::Array(items) => {
                Value::Array(items.iter().map(|item| self.mask_value(item)).collect())
            }
            Value::Object(object) => Value::Object(
                object
                    .iter()
                    .map(|(key, item)| (self.mask(key), self.mask_value(item)))
                    .collect(),
            ),
            Value::Null | Value::Bool(_) | Value::Number(_) => value.clone(),
        }
    }
}

#[cfg(test)]
#[path = "secret_values_tests.rs"]
mod tests;
