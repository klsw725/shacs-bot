//! `shacs-bot provider status` and the gateway's start line about the
//! provider credential (Spec 052 PRD 000 scopes 3 and 4). Both judge the
//! runtime's own plan (`ProviderCredentialRuntime::plan_provider_credential`)
//! and show no secret value.

use super::{
    default_config_path, local_provider_credential_runtime, runtime_containment_inspect, take_path,
    ArgParser, CliCommand, CliError, ProviderCommand, RuntimeContainmentInspect,
};
use shacs_config::{
    config_migration_marker_path, load_config_with_env, secrets_migration_marker_path,
    AgentDefaults, ConfigBundle, CredentialFamily, LoadOptions, ProcessEnv,
};
use shacs_core::runtime::trusted_runtime::{Spec030FactStore, WorkspaceTrustObservation};
use shacs_core::runtime::{
    CredentialSlot, LocalAuthSkip, PlannedProvider, ProviderClientResolutionRequest,
    ProviderCredentialPlan,
};
use shacs_providers::ProviderRegistry;
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderStatusOptions {
    pub config_path: Option<PathBuf>,
}

pub(super) fn parse(
    mut parser: ArgParser,
    global_config: Option<PathBuf>,
) -> Result<CliCommand, CliError> {
    let mut config_path = global_config;
    while let Some(arg) = parser.next() {
        match arg.as_str() {
            "--config" | "-c" => config_path = Some(take_path(&mut parser, &arg)?),
            "--help" | "-h" => return Ok(CliCommand::Help),
            other => {
                return Err(CliError::InvalidArguments(format!(
                    "unknown provider status argument `{other}`"
                )))
            }
        }
    }
    Ok(CliCommand::Provider(ProviderCommand::Status(
        ProviderStatusOptions { config_path },
    )))
}

/// Reads config with `${NAME}` resolved and writes nothing; a migration
/// marker does not stop it, it only narrows the advice to recovery.
pub(super) fn run(options: ProviderStatusOptions) -> Result<String, CliError> {
    let config_path = options.config_path.unwrap_or_else(default_config_path);
    let bundle = load_config_with_env(
        LoadOptions {
            config_path: Some(config_path.clone()),
            workspace_override: None,
            resolve_env: true,
            write_back_migrations: false,
        },
        &ProcessEnv,
    )?;
    let plan = plan(&bundle);
    if plan.auth_store_unreadable {
        return Err(CliError::Runtime(format!(
            "the auth store could not be read: {}",
            bundle.context.auth_path().display()
        )));
    }
    let migration_open = [
        config_migration_marker_path(&config_path),
        secrets_migration_marker_path(&config_path),
    ]
    .iter()
    .any(|marker| marker.exists());
    let form = CommandForm::of(&runtime_containment_inspect(&bundle));
    let readiness = judge(&plan, &bundle.config.agents.defaults, form);
    Ok(render(&readiness, migration_open, form))
}

/// The line the gateway adds after its channel plan when the provider
/// credential is missing or could not be checked; `None` when it is ready.
pub(super) fn startup_line(bundle: &ConfigBundle) -> Option<String> {
    let plan = plan(bundle);
    let form = CommandForm::of(&runtime_containment_inspect(bundle));
    if plan.auth_store_unreadable {
        return Some(format!(
            "provider: not checked (the auth store could not be read); next: {}",
            form.command("provider status")
        ));
    }
    match judge(&plan, &bundle.config.agents.defaults, form) {
        ProviderReadiness::Ready { .. } => None,
        ProviderReadiness::Missing {
            provider,
            reason,
            next,
        } => Some(format!(
            "provider: missing {provider} ({reason}); next: {next}"
        )),
    }
}

/// The plan of the gateway's credential runtime for `bundle`, built the way
/// the gateway builds it; facts stay in memory and the plan records none.
fn plan(bundle: &ConfigBundle) -> ProviderCredentialPlan {
    let runtime = local_provider_credential_runtime(
        bundle,
        Spec030FactStore::new(WorkspaceTrustObservation::Untrusted),
    );
    let defaults = &bundle.config.agents.defaults;
    runtime.plan_provider_credential(ProviderClientResolutionRequest {
        registry: &ProviderRegistry::new(),
        requested_provider: &defaults.provider,
        model: &defaults.model,
        providers: &bundle.config.providers,
    })
}

/// How advice writes commands: directly, or through the Compose `shacs-cli`
/// service when running in the official container.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CommandForm {
    Host,
    OfficialContainer,
}

impl CommandForm {
    fn of(inspect: &RuntimeContainmentInspect) -> Self {
        let official = inspect.contained == Some(true)
            && inspect
                .backend
                .as_deref()
                .and_then(|backend| backend.split('+').next())
                == Some("official-container");
        if official {
            Self::OfficialContainer
        } else {
            Self::Host
        }
    }

    fn command(self, arguments: &str) -> String {
        match self {
            Self::Host => format!("shacs-bot {arguments}"),
            Self::OfficialContainer => format!("docker compose run --rm shacs-cli {arguments}"),
        }
    }

    fn codex_login(self) -> String {
        match self {
            Self::Host => self.command("provider codex login"),
            Self::OfficialContainer => self.command("provider codex login --headless"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ProviderReadiness {
    Ready {
        provider: String,
        credential: String,
    },
    Missing {
        provider: String,
        reason: String,
        next: String,
    },
}

fn ready(provider: &str, credential: &str) -> ProviderReadiness {
    ProviderReadiness::Ready {
        provider: provider.to_owned(),
        credential: credential.to_owned(),
    }
}

fn missing(provider: &str, reason: String, next: String) -> ProviderReadiness {
    ProviderReadiness::Missing {
        provider: provider.to_owned(),
        reason,
        next,
    }
}

fn judge(
    plan: &ProviderCredentialPlan,
    defaults: &AgentDefaults,
    form: CommandForm,
) -> ProviderReadiness {
    let Some(provider) = &plan.selected else {
        if plan.unknown_provider {
            return missing(
                "none",
                format!(
                    "agents.defaults.provider `{}` is not a known provider",
                    defaults.provider
                ),
                "set agents.defaults.provider in config to a known provider or \"auto\"".to_owned(),
            );
        }
        return missing(
            "none",
            format!(
                "no provider in config or the auth store can serve agents.defaults.model `{}`",
                defaults.model
            ),
            format!(
                "log in with a ChatGPT account: {}; or set up an API key provider (docs/USAGE.md)",
                form.codex_login()
            ),
        );
    };
    let id = provider.provider_id.as_str();
    match &provider.slot {
        Some(CredentialSlot::RuntimeOverride) => ready(id, "runtime override"),
        Some(CredentialSlot::Environment { name, empty: false }) => {
            ready(id, &format!("environment variable {name}"))
        }
        Some(CredentialSlot::Environment { name, empty: true }) => missing(
            id,
            format!("{name} is set but empty, and the runtime reads it before any saved key"),
            match form {
                CommandForm::Host => format!("set {name} to the key, or unset it"),
                CommandForm::OfficialContainer => format!(
                    "set {name}=<key> in the .env file next to docker-compose.yml, then run `docker compose up -d shacs-gateway`"
                ),
            },
        ),
        Some(CredentialSlot::LocalApiKey { empty: false }) => {
            ready(id, "API key saved in the auth store")
        }
        Some(CredentialSlot::LocalApiKey { empty: true }) => missing(
            id,
            format!("the API key saved for {id} is empty"),
            add_credential(provider, form),
        ),
        Some(CredentialSlot::LocalOAuth { empty: true, .. }) => missing(
            id,
            format!("the sign-in saved for {id} is empty"),
            add_credential(provider, form),
        ),
        Some(CredentialSlot::LocalOAuth {
            expired: true,
            refreshable: false,
            ..
        }) => missing(
            id,
            format!("the sign-in saved for {id} has expired and has no refresh token"),
            add_credential(provider, form),
        ),
        Some(CredentialSlot::LocalOAuth { expired: true, .. }) => ready(
            id,
            "sign-in saved in the auth store (expired; the runtime refreshes it on first use)",
        ),
        Some(CredentialSlot::LocalOAuth { .. }) => ready(id, "sign-in saved in the auth store"),
        Some(CredentialSlot::LocalAuthUnreadable) => missing(
            id,
            "the auth store could not be read".to_owned(),
            form.command("provider status"),
        ),
        Some(CredentialSlot::Command) => ready(
            id,
            "credential command (not run here; the runtime runs it on first use)",
        ),
        Some(CredentialSlot::ProviderLiteral) => {
            ready(id, &format!("providers.{id}.apiKey in config"))
        }
        None if provider.fingerprint_differs => missing(
            id,
            format!(
                "the sign-in or key for {id} was saved under a different credential setting (providers.{id}.credentialSource or apiKeyRef)"
            ),
            format!(
                "restore providers.{id}.credentialSource and apiKeyRef to what they were when it was saved"
            ),
        ),
        None if !provider.local_auth_allowed => missing(
            id,
            format!(
                "no credential for {id}, and providers.{id}.credentialSource does not allow the auth store"
            ),
            match &provider.declared_environment {
                Some(name) => format!("set {name} or providers.{id}.apiKey"),
                None => format!("set providers.{id}.apiKey or a credential command"),
            },
        ),
        None => missing(
            id,
            match (provider.local_auth_skip, provider.family) {
                (Some(LocalAuthSkip::KindMismatch), CredentialFamily::OAuth) => {
                    format!("no credential for {id}; the entry saved for it is not a sign-in")
                }
                (Some(LocalAuthSkip::KindMismatch), CredentialFamily::ApiKey) => {
                    format!("no credential for {id}; the entry saved for it is not an API key")
                }
                _ => format!("no credential for {id}"),
            },
            add_credential(provider, form),
        ),
    }
}

/// The command that saves a new credential for `provider`, and for an API
/// key provider run directly also its environment variable.
fn add_credential(provider: &PlannedProvider, form: CommandForm) -> String {
    let id = provider.provider_id.as_str();
    match (id, provider.family) {
        ("openai_codex", _) => form.codex_login(),
        ("github_copilot", _) => form.command("provider copilot import-token --token-stdin"),
        (_, CredentialFamily::OAuth) => format!("sign in to {id} (docs/USAGE.md)"),
        (_, CredentialFamily::ApiKey) => {
            let import = form.command(&format!("provider {id} import-key --token-stdin"));
            match (&provider.declared_environment, form) {
                (Some(name), CommandForm::Host) => format!("{import}, or set {name}"),
                _ => import,
            }
        }
    }
}

fn render(readiness: &ProviderReadiness, migration_open: bool, form: CommandForm) -> String {
    let recover = migration_open.then(|| {
        format!(
            "finish the interrupted config migration first: {}",
            form.command("runtime config-migrate --recover")
        )
    });
    let mut lines = match readiness {
        ProviderReadiness::Ready {
            provider,
            credential,
        } => vec![
            format!("provider: ready {provider}"),
            format!("credential: {credential}"),
        ],
        ProviderReadiness::Missing {
            provider,
            reason,
            next,
        } => vec![
            format!("provider: missing {provider}"),
            format!("reason: {reason}"),
            format!("next: {}", recover.as_deref().unwrap_or(next)),
        ],
    };
    if let (ProviderReadiness::Ready { .. }, Some(recover)) = (readiness, &recover) {
        lines.push(format!("next: {recover}"));
    }
    lines.join("\n")
}

#[cfg(test)]
#[path = "provider_status_cli_tests.rs"]
mod tests;
