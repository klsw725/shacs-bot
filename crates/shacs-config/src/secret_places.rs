//! Where config keeps secret values, the `.env.json` names they move to, and
//! when a move must stop (Spec 048 PRD 010 scopes 3–5).

use crate::env_file::EnvFile;
use crate::EnvSource;
use serde::Serialize;
use serde_json::Value;
use shacs_redaction::is_sensitive_key;
use std::collections::BTreeMap;
use std::fmt;

const NAME_PREFIX: &str = "SHACS";
const SECRET_REF_KIND: &str = "secret_ref";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MovedSecret {
    pub field: String,
    pub name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum KeptReason {
    Placeholder,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct KeptSecret {
    pub field: String,
    pub reason: KeptReason,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ConflictReason {
    SameNameInConfig,
    EnvFileDiffers,
    ProcessEnvDiffers,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SecretConflict {
    pub field: String,
    pub name: String,
    pub reason: ConflictReason,
}

impl fmt::Display for SecretConflict {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let reason = match self.reason {
            ConflictReason::SameNameInConfig => "another field gives this name a different value",
            ConflictReason::EnvFileDiffers => ".env.json has a different value",
            ConflictReason::ProcessEnvDiffers => {
                "the process environment has a different non-empty value"
            }
        };
        write!(formatter, "{} -> {} ({reason})", self.field, self.name)
    }
}

/// What a move would do: the config with `${NAME}` in each moved field and
/// the `.env.json` that holds the moved values.
pub(crate) struct SecretsPlan {
    pub(crate) moved: Vec<MovedSecret>,
    pub(crate) kept: Vec<KeptSecret>,
    pub(crate) conflicts: Vec<SecretConflict>,
    pub(crate) config: Value,
    pub(crate) env_file: EnvFile,
}

#[derive(Debug, Clone)]
enum Segment {
    Key(String),
    Index(usize),
}

struct Place {
    path: Vec<Segment>,
    field: String,
    name: String,
    value: String,
}

#[derive(Default)]
struct Found {
    places: Vec<Place>,
    kept: Vec<KeptSecret>,
}

pub(crate) fn plan(
    config: &Value,
    env_file: &EnvFile,
    env: &(impl EnvSource + ?Sized),
) -> SecretsPlan {
    let mut found = Found::default();
    walk(config, &mut Vec::new(), false, &mut found);
    let mut by_name = BTreeMap::<&str, Vec<&Place>>::new();
    for place in &found.places {
        by_name.entry(&place.name).or_default().push(place);
    }
    let mut conflicts = Vec::new();
    for (name, places) in &by_name {
        let value = &places[0].value;
        let reason = if places.iter().any(|place| &place.value != value) {
            Some(ConflictReason::SameNameInConfig)
        } else if env_file.get(name).is_some_and(|existing| existing != value) {
            Some(ConflictReason::EnvFileDiffers)
        } else if env
            .var(name)
            .is_some_and(|process| !process.is_empty() && &process != value)
        {
            Some(ConflictReason::ProcessEnvDiffers)
        } else {
            None
        };
        if let Some(reason) = reason {
            conflicts.extend(places.iter().map(|place| SecretConflict {
                field: place.field.clone(),
                name: place.name.clone(),
                reason,
            }));
        }
    }
    let mut result = config.clone();
    let mut result_env = env_file.clone();
    for place in &found.places {
        set(
            &mut result,
            &place.path,
            Value::String(format!("${{{}}}", place.name)),
        );
        result_env.insert(place.name.clone(), place.value.clone());
    }
    SecretsPlan {
        moved: found
            .places
            .iter()
            .map(|place| MovedSecret {
                field: place.field.clone(),
                name: place.name.clone(),
            })
            .collect(),
        kept: found.kept,
        conflicts,
        config: result,
        env_file: result_env,
    }
}

/// Visits every value; `secret` says whether a text found here is a secret
/// place (a sensitive key, an array under one, or an env block entry).
fn walk(value: &Value, path: &mut Vec<Segment>, secret: bool, found: &mut Found) {
    match value {
        Value::String(text) if secret => consider(path, text, found),
        Value::Array(items) => {
            for (index, item) in items.iter().enumerate() {
                path.push(Segment::Index(index));
                walk(item, path, secret, found);
                path.pop();
            }
        }
        Value::Object(object) => {
            if object.get("kind").and_then(Value::as_str) == Some(SECRET_REF_KIND) {
                return;
            }
            let env_block = is_env_block(path);
            for (key, item) in object {
                path.push(Segment::Key(key.clone()));
                walk(item, path, env_block || is_sensitive_key(key), found);
                path.pop();
            }
        }
        _ => {}
    }
}

fn consider(path: &[Segment], text: &str, found: &mut Found) {
    if text.is_empty() || is_single_placeholder(text) {
        return;
    }
    if text.contains("${") {
        found.kept.push(KeptSecret {
            field: field(path),
            reason: KeptReason::Placeholder,
        });
        return;
    }
    found.places.push(Place {
        path: path.to_vec(),
        field: field(path),
        name: env_name(path),
        value: text.to_owned(),
    });
}

fn is_single_placeholder(text: &str) -> bool {
    text.strip_prefix("${")
        .and_then(|rest| rest.strip_suffix('}'))
        .is_some_and(|inner| !inner.contains(['$', '{', '}']))
}

/// The top-level `env`, `tools.exec.env` and `tools.mcpServers.<server>.env`.
fn is_env_block(path: &[Segment]) -> bool {
    let keys = path
        .iter()
        .map(|segment| match segment {
            Segment::Key(key) => Some(key.as_str()),
            Segment::Index(_) => None,
        })
        .collect::<Option<Vec<_>>>();
    matches!(
        keys.as_deref(),
        Some(["env"] | ["tools", "exec", "env"] | ["tools", "mcpServers", _, "env"])
    )
}

fn field(path: &[Segment]) -> String {
    let mut field = String::new();
    for segment in path {
        match segment {
            Segment::Key(key) => {
                if !field.is_empty() {
                    field.push('.');
                }
                field.push_str(key);
            }
            Segment::Index(index) => field.push_str(&format!("[{index}]")),
        }
    }
    field
}

/// The `.env.json` name a secret at the object field path `keys` moves to,
/// as `runtime config-migrate` names it: `["channels", "discord", "token"]`
/// gives `SHACS_CHANNELS_DISCORD_TOKEN`.
pub fn secret_env_name(keys: &[&str]) -> String {
    let path: Vec<Segment> = keys
        .iter()
        .map(|key| Segment::Key((*key).to_owned()))
        .collect();
    env_name(&path)
}

fn env_name(path: &[Segment]) -> String {
    let mut name = String::from(NAME_PREFIX);
    for segment in path {
        name.push('_');
        match segment {
            Segment::Key(key) => push_key(&mut name, key),
            Segment::Index(index) => name.push_str(&index.to_string()),
        }
    }
    let mut collapsed = String::with_capacity(name.len());
    for character in name.chars() {
        if !(character == '_' && collapsed.ends_with('_')) {
            collapsed.push(character);
        }
    }
    collapsed.trim_end_matches('_').to_owned()
}

/// Splits camelCase with `_`, turns anything but ASCII letters and digits
/// into `_`, and upper-cases the rest.
fn push_key(name: &mut String, key: &str) {
    let mut previous = None::<char>;
    for character in key.chars() {
        if character.is_ascii_uppercase()
            && previous
                .is_some_and(|previous| previous.is_ascii_lowercase() || previous.is_ascii_digit())
        {
            name.push('_');
        }
        if character.is_ascii_alphanumeric() {
            name.push(character.to_ascii_uppercase());
        } else {
            name.push('_');
        }
        previous = Some(character);
    }
}

fn set(value: &mut Value, path: &[Segment], replacement: Value) {
    let Some((last, parents)) = path.split_last() else {
        return;
    };
    let mut current = value;
    for segment in parents {
        let next = match segment {
            Segment::Key(key) => current.get_mut(key.as_str()),
            Segment::Index(index) => current.get_mut(*index),
        };
        let Some(next) = next else {
            return;
        };
        current = next;
    }
    let slot = match last {
        Segment::Key(key) => current.get_mut(key.as_str()),
        Segment::Index(index) => current.get_mut(*index),
    };
    if let Some(slot) = slot {
        *slot = replacement;
    }
}

#[cfg(test)]
#[path = "secret_places_tests.rs"]
mod tests;
