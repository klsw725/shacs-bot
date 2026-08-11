mod client;
mod command;
mod local;
mod resolve;

use shacs_config::{CredentialSourceDeclaration, OAuthRefresh, OAuthRefreshRequest, RawCredential};
use shacs_providers::{ProviderRegistry, ProvidersConfig};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::trusted_runtime::Spec030FactStore;
use crate::controlled_child::ControlledChildAbort;

pub use client::CredentialResolvingProviderClient;
pub(crate) use client::ProviderInvocationClient;

pub trait OAuthCredentialRefresher: Send + Sync {
    fn refresh(
        &self,
        provider_id: &str,
        request: &OAuthRefreshRequest<'_>,
    ) -> Result<OAuthRefresh, String>;
}

pub struct ProviderClientResolutionRequest<'a> {
    pub registry: &'a ProviderRegistry,
    pub requested_provider: &'a str,
    pub model: &'a str,
    pub providers: &'a ProvidersConfig,
}

#[derive(Debug, Clone, Default)]
pub struct ProviderCredentialInvocation {
    runtime_override: Option<RawCredential>,
    command_abort: ControlledChildAbort,
}

impl ProviderCredentialInvocation {
    pub fn new(
        runtime_override: Option<RawCredential>,
        command_abort: ControlledChildAbort,
    ) -> Self {
        Self {
            runtime_override,
            command_abort,
        }
    }
}

pub struct ProviderCredentialRuntime {
    auth_path: PathBuf,
    cwd: PathBuf,
    facts: Spec030FactStore,
    environment: BTreeMap<String, String>,
    runtime_overrides: BTreeMap<String, RawCredential>,
    declarations: BTreeMap<String, CredentialSourceDeclaration>,
    command_cache: Mutex<BTreeMap<String, RawCredential>>,
    command_timeout: Duration,
    command_abort: ControlledChildAbort,
    oauth_refresher: Option<Arc<dyn OAuthCredentialRefresher>>,
}

impl ProviderCredentialRuntime {
    pub fn new(
        auth_path: impl Into<PathBuf>,
        cwd: impl Into<PathBuf>,
        facts: Spec030FactStore,
    ) -> Self {
        Self {
            auth_path: auth_path.into(),
            cwd: cwd.into(),
            facts,
            environment: BTreeMap::new(),
            runtime_overrides: BTreeMap::new(),
            declarations: BTreeMap::new(),
            command_cache: Mutex::new(BTreeMap::new()),
            command_timeout: Duration::from_secs(10),
            command_abort: ControlledChildAbort::new(),
            oauth_refresher: None,
        }
    }

    pub fn with_environment(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.environment.insert(name.into(), value.into());
        self
    }

    pub fn with_runtime_override(
        mut self,
        provider_id: impl Into<String>,
        credential: RawCredential,
    ) -> Self {
        self.runtime_overrides
            .insert(provider_id.into(), credential);
        self
    }

    pub fn with_declaration(
        mut self,
        provider_id: impl Into<String>,
        declaration: CredentialSourceDeclaration,
    ) -> Self {
        self.declarations.insert(provider_id.into(), declaration);
        self
    }

    pub fn with_command_timeout(mut self, timeout: Duration) -> Self {
        self.command_timeout = timeout;
        self
    }

    pub fn with_command_abort(mut self, abort: ControlledChildAbort) -> Self {
        self.command_abort = abort;
        self
    }

    pub fn with_oauth_refresher(mut self, refresher: Arc<dyn OAuthCredentialRefresher>) -> Self {
        self.oauth_refresher = Some(refresher);
        self
    }

    pub fn facts(&self) -> Spec030FactStore {
        self.facts.clone()
    }

    fn auth_path(&self) -> &Path {
        &self.auth_path
    }
}
