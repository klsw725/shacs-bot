use crate::{ProviderCredentialSourceConfig, TrustedRuntimeConfig};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfilesConfig {
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub providers: BTreeMap<String, ProviderProfileConfig>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub trusted_runtimes: BTreeMap<String, TrustedRuntimeConfig>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub contexts: BTreeMap<String, ContextProfileConfig>,
    #[serde(default, skip_serializing_if = "ProfileSelection::is_empty")]
    pub selection: ProfileSelection,
}

impl ProfilesConfig {
    pub fn is_default(&self) -> bool {
        self == &Self::default()
    }

    pub fn resolve(&self) -> Result<ResolvedProfiles<'_>, ProfileResolutionError> {
        let source = if self.selection.is_empty() {
            ProfileSelectionSource::Defaults
        } else {
            ProfileSelectionSource::Configured
        };
        Ok(ResolvedProfiles {
            provider: selected(
                &self.providers,
                self.selection.provider.as_deref(),
                ProfileKind::Provider,
            )?,
            trusted_runtime: selected(
                &self.trusted_runtimes,
                self.selection.trusted_runtime.as_deref(),
                ProfileKind::TrustedRuntime,
            )?,
            context: selected(
                &self.contexts,
                self.selection.context.as_deref(),
                ProfileKind::Context,
            )?,
            source,
        })
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderProfileConfig {
    pub provider: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credential_source: Option<ProviderCredentialSourceConfig>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextProfileConfig {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileSelection {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trusted_runtime: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<String>,
}

impl ProfileSelection {
    fn is_empty(&self) -> bool {
        self.provider.is_none() && self.trusted_runtime.is_none() && self.context.is_none()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProfileSelectionSource {
    Defaults,
    Configured,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProfileKind {
    Provider,
    TrustedRuntime,
    Context,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileResolutionError {
    pub kind: ProfileKind,
    pub name: String,
}

impl fmt::Display for ProfileResolutionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "selected {:?} profile '{}' does not exist",
            self.kind, self.name
        )
    }
}

impl std::error::Error for ProfileResolutionError {}

#[derive(Debug, Clone, Copy)]
pub struct SelectedProfile<'a, T> {
    pub name: &'a str,
    pub value: &'a T,
}

#[derive(Debug, Clone)]
pub struct ResolvedProfiles<'a> {
    pub provider: Option<SelectedProfile<'a, ProviderProfileConfig>>,
    pub trusted_runtime: Option<SelectedProfile<'a, TrustedRuntimeConfig>>,
    pub context: Option<SelectedProfile<'a, ContextProfileConfig>>,
    pub source: ProfileSelectionSource,
}

fn selected<'a, T>(
    profiles: &'a BTreeMap<String, T>,
    name: Option<&'a str>,
    kind: ProfileKind,
) -> Result<Option<SelectedProfile<'a, T>>, ProfileResolutionError> {
    name.map(|name| {
        profiles
            .get(name)
            .map(|value| SelectedProfile { name, value })
            .ok_or_else(|| ProfileResolutionError {
                kind,
                name: name.to_owned(),
            })
    })
    .transpose()
}
