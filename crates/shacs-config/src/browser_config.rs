use crate::{ConfigError, PermissionRuleLists, PermissionsConfig, ProcessEnv};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserConfig {
    pub enable: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub screenshots: Option<bool>,
    pub startup_timeout_seconds: u64,
    pub idle_timeout_seconds: u64,
    pub page_timeout_seconds: u64,
    pub view: BrowserViewConfig,
}

impl Default for BrowserConfig {
    fn default() -> Self {
        Self {
            enable: false,
            screenshots: None,
            startup_timeout_seconds: 20,
            idle_timeout_seconds: 300,
            page_timeout_seconds: 30,
            view: BrowserViewConfig::default(),
        }
    }
}

/// `tools.browser.view`: the view-only screen viewer (Spec 049 PRD 040).
/// The password is a secret like other `password` keys: `${NAME}` in the
/// file resolves through the usual process-environment-then-`.env.json`
/// order, the value never appears in `Debug` output, and the viewer rejects
/// an unusable value at browser start instead of failing the config. Unlike
/// every other field, a reference with no value anywhere resolves to the
/// empty string (the viewer's `password_missing`) so the recommended
/// `"${SHACS_BROWSER_VIEW_PASSWORD}"` never blocks startup, even with the
/// viewer disabled (`resolve_config_env_refs`).
#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserViewConfig {
    #[serde(default)]
    pub enable: bool,
    #[serde(default)]
    pub password: String,
}

impl std::fmt::Debug for BrowserViewConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BrowserViewConfig")
            .field("enable", &self.enable)
            .field(
                "password",
                &if self.password.is_empty() {
                    ""
                } else {
                    "[REDACTED]"
                },
            )
            .finish()
    }
}

impl<'de> Deserialize<'de> for BrowserConfig {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        use serde::de::Error;
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Fields {
            #[serde(default)]
            enable: bool,
            screenshots: Option<bool>,
            startup_timeout_seconds: Option<u64>,
            idle_timeout_seconds: Option<u64>,
            page_timeout_seconds: Option<u64>,
            #[serde(default)]
            view: BrowserViewConfig,
        }
        let fields = Fields::deserialize(deserializer)?;
        let defaults = Self::default();
        let config = Self {
            enable: fields.enable,
            screenshots: fields.screenshots,
            view: fields.view,
            startup_timeout_seconds: fields
                .startup_timeout_seconds
                .unwrap_or(defaults.startup_timeout_seconds),
            idle_timeout_seconds: fields
                .idle_timeout_seconds
                .unwrap_or(defaults.idle_timeout_seconds),
            page_timeout_seconds: fields
                .page_timeout_seconds
                .unwrap_or(defaults.page_timeout_seconds),
        };
        if !(5..=60).contains(&config.startup_timeout_seconds)
            || !(60..=1800).contains(&config.idle_timeout_seconds)
            || !(5..=120).contains(&config.page_timeout_seconds)
        {
            return Err(D::Error::custom(
                "tools.browser timeout outside allowed range",
            ));
        }
        Ok(config)
    }
}

/// Evidence from the selected user-local file, never from merged/model configuration.
/// Its private source bytes also invalidate evidence after any config edit.
#[derive(Clone, PartialEq, Eq)]
pub struct UserLocalBrowserEnable {
    selected_config_path: PathBuf,
    selected_workspace: PathBuf,
    config_path: PathBuf,
    workspace: PathBuf,
    source: String,
    permission_rules: PermissionRuleLists,
}

impl std::fmt::Debug for UserLocalBrowserEnable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UserLocalBrowserEnable")
            .finish_non_exhaustive()
    }
}

impl UserLocalBrowserEnable {
    pub fn read(config_path: &Path, workspace: &Path) -> Result<Option<Self>, ConfigError> {
        if !config_path.exists() {
            return Ok(None);
        }
        let selected_config_path = config_path.to_path_buf();
        let selected_workspace = workspace.to_path_buf();
        let config_path = fs::canonicalize(config_path)?;
        let workspace = fs::canonicalize(workspace)?;
        if config_path.starts_with(&workspace) {
            return Ok(None);
        }
        let source = fs::read_to_string(&config_path)?;
        let value: Value = serde_json::from_str(&source)?;
        if value.pointer("/tools/browser/enable") != Some(&Value::Bool(true)) {
            return Ok(None);
        }
        let permission_rules = resolved_permission_rules(&source, &selected_config_path)?;
        Ok(Some(Self {
            selected_config_path,
            selected_workspace,
            config_path,
            workspace,
            source,
            permission_rules,
        }))
    }

    pub fn config_path(&self) -> &Path {
        &self.config_path
    }

    pub fn workspace(&self) -> &Path {
        &self.workspace
    }

    pub fn permission_rules(&self) -> &PermissionRuleLists {
        &self.permission_rules
    }

    pub fn is_current(&self) -> bool {
        fs::canonicalize(&self.selected_config_path).is_ok_and(|path| path == self.config_path)
            && fs::canonicalize(&self.selected_workspace).is_ok_and(|path| path == self.workspace)
            && fs::read_to_string(&self.config_path).is_ok_and(|text| text == self.source)
            && resolved_permission_rules(&self.source, &self.selected_config_path)
                .is_ok_and(|rules| rules == self.permission_rules)
    }
}

fn resolved_permission_rules(
    source: &str,
    config_path: &Path,
) -> Result<PermissionRuleLists, ConfigError> {
    let value: Value = serde_json::from_str(source)?;
    let mut permissions = value
        .get("permissions")
        .cloned()
        .unwrap_or_else(|| Value::Object(Default::default()));
    let environment = crate::config_env(config_path, &ProcessEnv)?;
    crate::resolve_env_value(&mut permissions, &environment).map_err(ConfigError::Env)?;
    let permissions: PermissionsConfig = serde_json::from_value(permissions)?;
    Ok(permissions.user_rules)
}
