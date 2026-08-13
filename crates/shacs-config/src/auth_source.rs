use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderCredentialSourceConfig {
    pub schema_version: u32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sources: Vec<AuthSourceConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub environment: Option<String>,
    #[serde(default = "crate::default_true")]
    pub local_auth: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AuthSourceConfig {
    Environment { name: String },
    LocalAuthEntry { entry: String },
    Literal,
    Command { command: String },
}

impl ProviderCredentialSourceConfig {
    pub fn environment_name(&self) -> Option<&str> {
        self.sources
            .iter()
            .find_map(|source| match source {
                AuthSourceConfig::Environment { name } => Some(name.as_str()),
                AuthSourceConfig::LocalAuthEntry { .. }
                | AuthSourceConfig::Literal
                | AuthSourceConfig::Command { .. } => None,
            })
            .or(self.environment.as_deref())
    }

    pub fn local_auth_entry(&self) -> Option<&str> {
        self.sources.iter().find_map(|source| match source {
            AuthSourceConfig::LocalAuthEntry { entry } => Some(entry.as_str()),
            AuthSourceConfig::Environment { .. }
            | AuthSourceConfig::Literal
            | AuthSourceConfig::Command { .. } => None,
        })
    }

    pub fn command_line(&self) -> Option<&str> {
        self.sources
            .iter()
            .find_map(|source| match source {
                AuthSourceConfig::Command { command } => Some(command.as_str()),
                AuthSourceConfig::Environment { .. }
                | AuthSourceConfig::LocalAuthEntry { .. }
                | AuthSourceConfig::Literal => None,
            })
            .or(self.command.as_deref())
    }

    pub fn literal_enabled(&self) -> bool {
        self.sources
            .iter()
            .any(|source| matches!(source, AuthSourceConfig::Literal))
    }
}
