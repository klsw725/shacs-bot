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
    /// How long the browser stays open for the user's reply after their turn
    /// ends (Spec 050 PRD 000 scope 5).
    pub reply_wait_minutes: u64,
    /// How long one takeover lasts before the browser is taken back
    /// (Spec 050 PRD 010 scope 7).
    pub takeover_minutes: u64,
    /// Keep cookies and login state in `<data>/browser/profile` between
    /// browser runs (Spec 050 PRD 020 scope 1).
    pub persist_login: bool,
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
            reply_wait_minutes: 30,
            takeover_minutes: 15,
            persist_login: false,
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
    /// Base address shown to the user instead of the loopback default
    /// (Spec 050 PRD 010 scope 12). Display only: it opens no port and is
    /// never passed to a process. A value [`Self::base_url`] rejects is
    /// ignored rather than a config error.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub url: String,
}

impl BrowserViewConfig {
    /// The configured base address without a trailing `/`, when it has the
    /// accepted shape: `http://` or `https://`, a host, an optional port and
    /// an optional trailing `/`.
    pub fn base_url(&self) -> Option<&str> {
        let url = self.url.as_str();
        if url.is_empty() || url.len() > 200 || !url.is_ascii() {
            return None;
        }
        let trimmed = url.strip_suffix('/').unwrap_or(url);
        let authority = trimmed
            .strip_prefix("http://")
            .or_else(|| trimmed.strip_prefix("https://"))?;
        let (host, port) = match authority.strip_prefix('[') {
            Some(bracketed) => {
                let (address, rest) = bracketed.split_once(']')?;
                let literal = !address.is_empty()
                    && address
                        .bytes()
                        .all(|byte| byte.is_ascii_hexdigit() || byte == b':');
                match (literal, rest.strip_prefix(':')) {
                    (false, _) => return None,
                    (true, Some(port)) => ("v6", Some(port)),
                    (true, None) if rest.is_empty() => ("v6", None),
                    (true, None) => return None,
                }
            }
            None => match authority.rsplit_once(':') {
                Some((host, port)) => (host, Some(port)),
                None => (authority, None),
            },
        };
        let host_ok = !host.is_empty()
            && host
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'.' || byte == b'-');
        let port_ok = port.is_none_or(|port| {
            !port.is_empty()
                && port.len() <= 5
                && port.bytes().all(|byte| byte.is_ascii_digit())
                && port.parse::<u16>().is_ok_and(|port| port != 0)
        });
        (host_ok && port_ok).then_some(trimmed)
    }

    /// True when `url` is set but not usable, so the default address shows.
    pub fn url_ignored(&self) -> bool {
        !self.url.is_empty() && self.base_url().is_none()
    }
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
            .field("url", &self.url)
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
            reply_wait_minutes: Option<u64>,
            takeover_minutes: Option<u64>,
            #[serde(default)]
            persist_login: bool,
            #[serde(default)]
            view: BrowserViewConfig,
        }
        let fields = Fields::deserialize(deserializer)?;
        let defaults = Self::default();
        let config = Self {
            enable: fields.enable,
            screenshots: fields.screenshots,
            persist_login: fields.persist_login,
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
            reply_wait_minutes: fields
                .reply_wait_minutes
                .unwrap_or(defaults.reply_wait_minutes),
            takeover_minutes: fields.takeover_minutes.unwrap_or(defaults.takeover_minutes),
        };
        if !(5..=60).contains(&config.startup_timeout_seconds)
            || !(60..=1800).contains(&config.idle_timeout_seconds)
            || !(5..=120).contains(&config.page_timeout_seconds)
            || !(5..=120).contains(&config.reply_wait_minutes)
            || !(5..=60).contains(&config.takeover_minutes)
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
