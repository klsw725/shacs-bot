use crate::{SkillRegistryEntry, SkillRegistryStatus};
use serde::{Deserialize, Serialize};
use shacs_config::{EnvFile, EnvSource};
use std::collections::BTreeMap;
use std::fmt;
use std::path::Path;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillCredentialStatus {
    pub required: Vec<String>,
    pub missing: Vec<String>,
    pub reserved: Vec<String>,
    pub overridden_empty: Vec<String>,
    pub diagnostics: Vec<String>,
    pub inactive_reason: Option<String>,
}

/// Candidate values are memory-only; public status and Debug never contain them.
pub struct SkillCredentialResolution {
    pub status: SkillCredentialStatus,
    values: BTreeMap<String, String>,
}

impl fmt::Debug for SkillCredentialResolution {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SkillCredentialResolution")
            .field("status", &self.status)
            .field("names", &self.values.keys().collect::<Vec<_>>())
            .finish()
    }
}

pub struct SkillCredentialSources<'a, E: ?Sized> {
    pub process: &'a E,
    pub file: &'a EnvFile,
    pub explicit: &'a BTreeMap<String, String>,
}

impl SkillCredentialResolution {
    pub fn values(&self) -> &BTreeMap<String, String> {
        &self.values
    }

    pub fn into_values(self) -> BTreeMap<String, String> {
        self.values
    }
}

impl SkillRegistryEntry {
    pub fn inactive_reason(&self, disabled: &[String]) -> Option<&'static str> {
        match self.status {
            SkillRegistryStatus::Shadowed => Some("shadowed"),
            SkillRegistryStatus::Conflicted => Some("conflicted"),
            SkillRegistryStatus::Malformed => Some("malformed"),
            SkillRegistryStatus::Active => {
                if self.descriptor.disabled {
                    Some("frontmatter disabled")
                } else if disabled.contains(&self.descriptor.name) {
                    Some("disabledSkills")
                } else {
                    None
                }
            }
        }
    }
}

pub fn resolve_skill_credentials<E: EnvSource + ?Sized>(
    entry: &SkillRegistryEntry,
    disabled: &[String],
    sources: SkillCredentialSources<'_, E>,
) -> SkillCredentialResolution {
    let declaration = &entry.descriptor.credentials;
    let mut status = SkillCredentialStatus {
        required: declaration.required.clone(),
        reserved: declaration.reserved.clone(),
        diagnostics: declaration.diagnostics.clone(),
        inactive_reason: entry.inactive_reason(disabled).map(str::to_owned),
        ..SkillCredentialStatus::default()
    };
    let mut values = BTreeMap::new();
    if status.inactive_reason.is_none() && status.diagnostics.is_empty() {
        for name in &status.required {
            let value = if let Some(value) = sources.explicit.get(name) {
                if value.is_empty() {
                    status.overridden_empty.push(name.clone());
                }
                Some(value.clone())
            } else {
                sources
                    .process
                    .var(name)
                    .filter(|value| !value.is_empty())
                    .or_else(|| sources.file.get(name).map(str::to_owned))
            };
            match value {
                Some(value) if !value.is_empty() => {
                    values.insert(name.clone(), value);
                }
                _ => status.missing.push(name.clone()),
            }
        }
    }
    SkillCredentialResolution { status, values }
}

impl SkillCredentialStatus {
    pub fn summary(&self) -> String {
        let mut parts = Vec::new();
        if let Some(reason) = &self.inactive_reason {
            parts.push(format!("inactive: {reason}"));
        }
        if !self.missing.is_empty() {
            parts.push(format!("missing env: {}", self.missing.join(", ")));
        }
        if !self.overridden_empty.is_empty() {
            parts.push(format!(
                "overridden_empty: {}",
                self.overridden_empty.join(", ")
            ));
        }
        if !self.reserved.is_empty() {
            parts.push(format!("reserved (excluded): {}", self.reserved.join(", ")));
        }
        parts.extend(self.diagnostics.clone());
        parts.join("; ")
    }

    pub fn guidance(&self, env_path: &Path) -> String {
        let mut text = if self.required.is_empty() {
            self.summary()
        } else {
            format!(
                "required env: {}\n{}",
                self.required.join(", "),
                self.summary()
            )
        };
        if !self.missing.is_empty() {
            text.push_str(&format!(
                "\nAsk the user to edit {} directly and restart shacs-bot. For overridden_empty names, correct the empty explicit env setting. In Docker, edit the host folder mounted at the displayed container data path. Never request secret values in chat, question tools or approval questions, or read the secret file with tools to check them. Noninteractive runs must report missing names or do only work possible without them; do not wait for input.",
                env_path.display()
            ));
        }
        text
    }
}
