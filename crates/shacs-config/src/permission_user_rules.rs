//! The user's `permissions.allow|ask|deny` rule lists and `permissions.autoMode`
//! (Spec 048 PRD 000 scopes 4 and 13). Only the user-local config file is
//! read, so a workspace file cannot add them.

use serde::Serialize;
use serde_json::{Map, Value};
use std::fmt;

const MAX_AUTO_MODE_ENTRIES: usize = 64;
const MAX_AUTO_MODE_ENTRY_CHARS: usize = 1_000;
const MAX_CLASSIFIER_MODEL_CHARS: usize = 200;
const DEFAULTS_MARKER: &str = "$defaults";
const RULE_LISTS: [&str; 3] = ["allow", "ask", "deny"];
const AUTO_MODE_KEY: &str = "autoMode";

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct PermissionRuleLists {
    pub allow: Vec<String>,
    pub ask: Vec<String>,
    pub deny: Vec<String>,
}

/// The classifier settings; a list left out keeps the built-in default list.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AutoModeConfig {
    pub environment: Option<Vec<String>>,
    pub hard_deny: Option<Vec<String>>,
    pub soft_deny: Option<Vec<String>>,
    pub allow: Option<Vec<String>>,
    pub classifier_model: Option<String>,
    pub classify_all_shell: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "problem", content = "field", rename_all = "snake_case")]
pub enum AutoModeProblem {
    NotAnObject,
    NotAListOfStrings(String),
    TooManyEntries(String),
    EntryTooLong(String),
    DefaultsRepeated(String),
    WrittenTwice(String),
    ClassifierModelNotText,
    ClassifierModelTooLong,
    ClassifyAllShellNotBoolean,
}

impl fmt::Display for AutoModeProblem {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotAnObject => write!(formatter, "autoMode is not an object"),
            Self::NotAListOfStrings(field) => write!(formatter, "{field} is not a list of strings"),
            Self::TooManyEntries(field) => write!(
                formatter,
                "{field} has more than {MAX_AUTO_MODE_ENTRIES} entries"
            ),
            Self::EntryTooLong(field) => write!(
                formatter,
                "an entry of {field} is longer than {MAX_AUTO_MODE_ENTRY_CHARS} characters"
            ),
            Self::DefaultsRepeated(field) => {
                write!(
                    formatter,
                    "{field} lists \"{DEFAULTS_MARKER}\" more than once"
                )
            }
            Self::WrittenTwice(field) => {
                write!(formatter, "{field} is written under both of its spellings")
            }
            Self::ClassifierModelNotText => write!(formatter, "classifierModel is not text"),
            Self::ClassifierModelTooLong => write!(
                formatter,
                "classifierModel is longer than {MAX_CLASSIFIER_MODEL_CHARS} characters"
            ),
            Self::ClassifyAllShellNotBoolean => {
                write!(formatter, "classifyAllShell is not true or false")
            }
        }
    }
}

/// Reads the three rule lists. A list that is not an array of strings is
/// dropped whole, and its name is returned.
pub(crate) fn rule_lists(object: &Map<String, Value>) -> (PermissionRuleLists, Vec<&'static str>) {
    let mut lists = PermissionRuleLists::default();
    let mut malformed = Vec::new();
    for name in RULE_LISTS {
        let Some(value) = object.get(name) else {
            continue;
        };
        let Some(rules) = strings(value) else {
            malformed.push(name);
            continue;
        };
        match name {
            "allow" => lists.allow = rules,
            "ask" => lists.ask = rules,
            _ => lists.deny = rules,
        }
    }
    (lists, malformed)
}

pub(crate) fn auto_mode(value: Option<&Value>) -> Result<AutoModeConfig, AutoModeProblem> {
    let Some(value) = value else {
        return Ok(AutoModeConfig::default());
    };
    let object = value.as_object().ok_or(AutoModeProblem::NotAnObject)?;
    let classifier_model = match object.get("classifierModel") {
        None => None,
        Some(Value::String(model)) if model.chars().count() > MAX_CLASSIFIER_MODEL_CHARS => {
            return Err(AutoModeProblem::ClassifierModelTooLong);
        }
        Some(Value::String(model)) => Some(model.clone()),
        Some(_) => return Err(AutoModeProblem::ClassifierModelNotText),
    };
    let classify_all_shell = match object.get("classifyAllShell") {
        None => false,
        Some(Value::Bool(flag)) => *flag,
        Some(_) => return Err(AutoModeProblem::ClassifyAllShellNotBoolean),
    };
    Ok(AutoModeConfig {
        environment: slot_list(object, "environment", None)?,
        hard_deny: slot_list(object, "hardDeny", Some("hard_deny"))?,
        soft_deny: slot_list(object, "softDeny", Some("soft_deny"))?,
        allow: slot_list(object, "allow", None)?,
        classifier_model,
        classify_all_shell,
    })
}

/// The rule and `autoMode` values exactly as written, so saving the config
/// keeps them.
pub(crate) fn written_fields(object: &Map<String, Value>) -> Map<String, Value> {
    RULE_LISTS
        .iter()
        .chain([&AUTO_MODE_KEY])
        .filter_map(|key| {
            object
                .get(*key)
                .map(|value| ((*key).to_owned(), value.clone()))
        })
        .collect()
}

fn slot_list(
    object: &Map<String, Value>,
    name: &str,
    other_spelling: Option<&str>,
) -> Result<Option<Vec<String>>, AutoModeProblem> {
    let written = object.get(name);
    let other = other_spelling.and_then(|spelling| object.get(spelling));
    let value = match (written, other) {
        (Some(_), Some(_)) => return Err(AutoModeProblem::WrittenTwice(name.to_owned())),
        (Some(value), None) | (None, Some(value)) => value,
        (None, None) => return Ok(None),
    };
    let entries =
        strings(value).ok_or_else(|| AutoModeProblem::NotAListOfStrings(name.to_owned()))?;
    if entries.len() > MAX_AUTO_MODE_ENTRIES {
        return Err(AutoModeProblem::TooManyEntries(name.to_owned()));
    }
    if entries
        .iter()
        .any(|entry| entry.chars().count() > MAX_AUTO_MODE_ENTRY_CHARS)
    {
        return Err(AutoModeProblem::EntryTooLong(name.to_owned()));
    }
    if entries
        .iter()
        .filter(|entry| *entry == DEFAULTS_MARKER)
        .count()
        > 1
    {
        return Err(AutoModeProblem::DefaultsRepeated(name.to_owned()));
    }
    Ok(Some(entries))
}

fn strings(value: &Value) -> Option<Vec<String>> {
    value
        .as_array()?
        .iter()
        .map(|item| item.as_str().map(str::to_owned))
        .collect()
}

#[cfg(test)]
#[path = "permission_user_rules_tests.rs"]
mod tests;
