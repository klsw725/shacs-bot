//! Spec 052 PRD 000 acceptance 1–3 and PRD 040 acceptance 1: the plan picks
//! the provider and the place the runtime resolves from (an empty declared
//! environment variable counts as unset), writes nothing, and the runtime
//! still skips a local sign-in that turns stale under the store's lock.

use super::super::{OAuthCredentialRefresher, ProviderCredentialInvocation};
use super::{CredentialSlot, LocalAuthSkip, ProviderCredentialPlan};
use crate::runtime::trusted_runtime::{
    LocalSpec030ProjectionProvider, Spec030FactStore, WorkspaceTrustObservation,
};
use crate::runtime::{ProviderClientResolutionRequest, ProviderCredentialRuntime};
use shacs_config::{
    save_auth_store_to_path, AuthStore, CredentialFamily, CredentialFingerprint, CredentialSource,
    CredentialSourceDeclaration, OAuthRefresh, OAuthRefreshRequest, ProviderAuth, ProviderConfig,
    ProvidersConfig,
};
use shacs_projection::Spec030ProjectionProvider;
use shacs_providers::{ProviderError, ProviderRegistry};
use std::error::Error;
use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

type TestResult = Result<(), Box<dyn Error>>;

const DEFAULT_MODEL: &str = "anthropic/claude-opus-4-5";
/// An environment variable no test machine sets, so a declaration naming it
/// leaves the environment slot empty whatever the real environment holds.
const UNSET_ENVIRONMENT: &str = "SHACS_SPEC052_TEST_UNSET_KEY";

struct CountingRefresher(AtomicUsize);

impl OAuthCredentialRefresher for CountingRefresher {
    fn refresh(
        &self,
        _provider_id: &str,
        _request: &OAuthRefreshRequest<'_>,
    ) -> Result<OAuthRefresh, String> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok(OAuthRefresh::new("oauth-renewed", None, u64::MAX))
    }
}

/// What the runtime does for a case: resolves at a place, or fails.
#[derive(Debug, PartialEq, Eq)]
enum Resolution {
    Resolved {
        provider: String,
        source: CredentialSource,
        value: String,
    },
    ProviderNotFound,
    AuthRequired,
}

struct Case {
    name: &'static str,
    requested: &'static str,
    model: &'static str,
    providers: ProvidersConfig,
    auth: AuthStore,
    runtime: fn(ProviderCredentialRuntime) -> ProviderCredentialRuntime,
    provider: Option<&'static str>,
    slot: Option<CredentialSlot>,
    skip: Option<LocalAuthSkip>,
    fingerprint_differs: bool,
    resolution: Resolution,
}

fn unset_declaration() -> CredentialSourceDeclaration {
    CredentialSourceDeclaration {
        family: CredentialFamily::ApiKey,
        environment: Some(UNSET_ENVIRONMENT.to_owned()),
        local_auth: true,
        command: None,
    }
}

fn with_unset_anthropic_environment(
    runtime: ProviderCredentialRuntime,
) -> ProviderCredentialRuntime {
    runtime.with_declaration("anthropic", unset_declaration())
}

fn oauth(access: &str, refresh: Option<&str>, expires: Option<u64>) -> ProviderAuth {
    ProviderAuth {
        kind: "oauth".to_owned(),
        access: access.to_owned(),
        refresh: refresh.map(str::to_owned),
        expires,
        account_id: None,
    }
}

fn auth_with(entries: &[(&str, ProviderAuth)]) -> AuthStore {
    let mut auth = AuthStore::default();
    for (provider_id, entry) in entries {
        auth.providers
            .insert((*provider_id).to_owned(), entry.clone());
    }
    auth
}

fn config_with(entries: &[(&str, Option<&str>)]) -> ProvidersConfig {
    entries
        .iter()
        .map(|(provider_id, api_key)| {
            (
                (*provider_id).to_owned(),
                ProviderConfig {
                    api_key: api_key.map(str::to_owned),
                    ..ProviderConfig::default()
                },
            )
        })
        .collect()
}

fn resolved(provider: &str, source: CredentialSource, value: &str) -> Resolution {
    Resolution::Resolved {
        provider: provider.to_owned(),
        source,
        value: value.to_owned(),
    }
}

fn cases() -> Vec<Case> {
    let mut current_fingerprint = auth_with(&[("anthropic", ProviderAuth::api_key("auth-key"))]);
    current_fingerprint.set_fingerprint("anthropic", unset_declaration().fingerprint());
    let mut stale_fingerprint = auth_with(&[("anthropic", ProviderAuth::api_key("auth-key"))]);
    stale_fingerprint.set_fingerprint("anthropic", CredentialFingerprint::from_descriptor("old"));
    let mut stale_kind = auth_with(&[("openai_codex", ProviderAuth::api_key("wrong-kind"))]);
    stale_kind.set_fingerprint(
        "openai_codex",
        CredentialFingerprint::from_descriptor("old"),
    );
    vec![
        Case {
            name: "codex sign-in",
            requested: "openai_codex",
            model: "gpt-5.4",
            providers: ProvidersConfig::new(),
            auth: auth_with(&[("openai_codex", oauth("oauth-access", None, None))]),
            runtime: |runtime| runtime,
            provider: Some("openai_codex"),
            slot: Some(CredentialSlot::LocalOAuth {
                empty: false,
                expired: false,
                refreshable: false,
            }),
            skip: None,
            fingerprint_differs: false,
            resolution: resolved(
                "openai_codex",
                CredentialSource::LocalAuthStore,
                "oauth-access",
            ),
        },
        Case {
            name: "expired codex sign-in with a refresh token",
            requested: "openai_codex",
            model: "gpt-5.4",
            providers: ProvidersConfig::new(),
            auth: auth_with(&[("openai_codex", oauth("oauth-old", Some("refresh"), Some(1)))]),
            runtime: |runtime| runtime,
            provider: Some("openai_codex"),
            slot: Some(CredentialSlot::LocalOAuth {
                empty: false,
                expired: true,
                refreshable: true,
            }),
            skip: None,
            fingerprint_differs: false,
            resolution: resolved(
                "openai_codex",
                CredentialSource::LocalAuthStore,
                "oauth-renewed",
            ),
        },
        Case {
            name: "default config and an imported Anthropic key",
            requested: "auto",
            model: DEFAULT_MODEL,
            providers: ProvidersConfig::new(),
            auth: auth_with(&[("anthropic", ProviderAuth::api_key("auth-key"))]),
            runtime: with_unset_anthropic_environment,
            provider: Some("anthropic"),
            slot: Some(CredentialSlot::LocalApiKey { empty: false }),
            skip: None,
            fingerprint_differs: false,
            resolution: resolved("anthropic", CredentialSource::LocalAuthStore, "auth-key"),
        },
        Case {
            name: "saved key with the current fingerprint",
            requested: "auto",
            model: DEFAULT_MODEL,
            providers: ProvidersConfig::new(),
            auth: current_fingerprint,
            runtime: with_unset_anthropic_environment,
            provider: Some("anthropic"),
            slot: Some(CredentialSlot::LocalApiKey { empty: false }),
            skip: None,
            fingerprint_differs: false,
            resolution: resolved("anthropic", CredentialSource::LocalAuthStore, "auth-key"),
        },
        Case {
            name: "config entry and a set environment variable",
            requested: "auto",
            model: DEFAULT_MODEL,
            providers: config_with(&[("anthropic", None)]),
            auth: AuthStore::default(),
            runtime: |runtime| runtime.with_environment("ANTHROPIC_API_KEY", "env-key"),
            provider: Some("anthropic"),
            slot: Some(CredentialSlot::Environment {
                name: "ANTHROPIC_API_KEY".to_owned(),
                empty: false,
            }),
            skip: None,
            fingerprint_differs: false,
            resolution: resolved("anthropic", CredentialSource::Environment, "env-key"),
        },
        Case {
            name: "config apiKey resolved from .env.json",
            requested: "auto",
            model: DEFAULT_MODEL,
            providers: config_with(&[("anthropic", Some("literal-key"))]),
            auth: AuthStore::default(),
            runtime: with_unset_anthropic_environment,
            provider: Some("anthropic"),
            slot: Some(CredentialSlot::ProviderLiteral),
            skip: Some(LocalAuthSkip::NoEntry),
            fingerprint_differs: false,
            resolution: resolved(
                "anthropic",
                CredentialSource::ProviderLiteral,
                "literal-key",
            ),
        },
        Case {
            name: "credential command",
            requested: "openai",
            model: "gpt-4o",
            providers: config_with(&[("openai", Some("literal-must-not-run"))]),
            auth: AuthStore::default(),
            runtime: |runtime| {
                runtime.with_declaration(
                    "openai",
                    CredentialSourceDeclaration {
                        family: CredentialFamily::ApiKey,
                        environment: None,
                        local_auth: false,
                        command: Some("printf command-key".to_owned()),
                    },
                )
            },
            provider: Some("openai"),
            slot: Some(CredentialSlot::Command),
            skip: None,
            fingerprint_differs: false,
            resolution: resolved("openai", CredentialSource::Command, "command-key"),
        },
        Case {
            name: "no credential at all",
            requested: "auto",
            model: DEFAULT_MODEL,
            providers: ProvidersConfig::new(),
            auth: AuthStore::default(),
            runtime: |runtime| runtime,
            provider: None,
            slot: None,
            skip: None,
            fingerprint_differs: false,
            resolution: Resolution::ProviderNotFound,
        },
        Case {
            name: "only an environment variable",
            requested: "auto",
            model: DEFAULT_MODEL,
            providers: ProvidersConfig::new(),
            auth: AuthStore::default(),
            runtime: |runtime| runtime.with_environment("ANTHROPIC_API_KEY", "env-key"),
            provider: None,
            slot: None,
            skip: None,
            fingerprint_differs: false,
            resolution: Resolution::ProviderNotFound,
        },
        Case {
            name: "a credential only for another provider",
            requested: "openai_codex",
            model: "gpt-5.4",
            providers: ProvidersConfig::new(),
            auth: auth_with(&[("anthropic", ProviderAuth::api_key("auth-key"))]),
            runtime: |runtime| runtime,
            provider: Some("openai_codex"),
            slot: None,
            skip: Some(LocalAuthSkip::NoEntry),
            fingerprint_differs: false,
            resolution: Resolution::AuthRequired,
        },
        Case {
            name: "saved key behind an empty environment variable",
            requested: "auto",
            model: DEFAULT_MODEL,
            providers: ProvidersConfig::new(),
            auth: auth_with(&[("anthropic", ProviderAuth::api_key("auth-key"))]),
            runtime: |runtime| runtime.with_environment("ANTHROPIC_API_KEY", ""),
            provider: Some("anthropic"),
            slot: Some(CredentialSlot::LocalApiKey { empty: false }),
            skip: None,
            fingerprint_differs: false,
            resolution: resolved("anthropic", CredentialSource::LocalAuthStore, "auth-key"),
        },
        Case {
            name: "config apiKey behind an empty environment variable",
            requested: "auto",
            model: DEFAULT_MODEL,
            providers: config_with(&[("anthropic", Some("literal-key"))]),
            auth: AuthStore::default(),
            runtime: |runtime| runtime.with_environment("ANTHROPIC_API_KEY", ""),
            provider: Some("anthropic"),
            slot: Some(CredentialSlot::ProviderLiteral),
            skip: Some(LocalAuthSkip::NoEntry),
            fingerprint_differs: false,
            resolution: resolved(
                "anthropic",
                CredentialSource::ProviderLiteral,
                "literal-key",
            ),
        },
        Case {
            name: "a whitespace environment variable before a saved key",
            requested: "auto",
            model: DEFAULT_MODEL,
            providers: ProvidersConfig::new(),
            auth: auth_with(&[("anthropic", ProviderAuth::api_key("auth-key"))]),
            runtime: |runtime| runtime.with_environment("ANTHROPIC_API_KEY", "  "),
            provider: Some("anthropic"),
            slot: Some(CredentialSlot::Environment {
                name: "ANTHROPIC_API_KEY".to_owned(),
                empty: true,
            }),
            skip: None,
            fingerprint_differs: false,
            resolution: resolved("anthropic", CredentialSource::Environment, "  "),
        },
        Case {
            name: "a set environment variable before a saved key",
            requested: "auto",
            model: DEFAULT_MODEL,
            providers: ProvidersConfig::new(),
            auth: auth_with(&[("anthropic", ProviderAuth::api_key("auth-key"))]),
            runtime: |runtime| runtime.with_environment("ANTHROPIC_API_KEY", "env-key"),
            provider: Some("anthropic"),
            slot: Some(CredentialSlot::Environment {
                name: "ANTHROPIC_API_KEY".to_owned(),
                empty: false,
            }),
            skip: None,
            fingerprint_differs: false,
            resolution: resolved("anthropic", CredentialSource::Environment, "env-key"),
        },
        Case {
            name: "a set environment variable before a saved key with another fingerprint",
            requested: "auto",
            model: DEFAULT_MODEL,
            providers: ProvidersConfig::new(),
            auth: stale_fingerprint.clone(),
            runtime: |runtime| {
                runtime
                    .with_declaration("anthropic", unset_declaration())
                    .with_environment(UNSET_ENVIRONMENT, "env-key")
            },
            provider: Some("anthropic"),
            slot: Some(CredentialSlot::Environment {
                name: UNSET_ENVIRONMENT.to_owned(),
                empty: false,
            }),
            skip: None,
            fingerprint_differs: true,
            resolution: resolved("anthropic", CredentialSource::Environment, "env-key"),
        },
        Case {
            name: "only a saved key with another fingerprint",
            requested: "auto",
            model: DEFAULT_MODEL,
            providers: ProvidersConfig::new(),
            auth: stale_fingerprint.clone(),
            runtime: with_unset_anthropic_environment,
            provider: Some("anthropic"),
            slot: None,
            skip: Some(LocalAuthSkip::FingerprintStale),
            fingerprint_differs: true,
            resolution: Resolution::AuthRequired,
        },
        Case {
            name: "expired codex sign-in without a refresh token",
            requested: "openai_codex",
            model: "gpt-5.4",
            providers: ProvidersConfig::new(),
            auth: auth_with(&[("openai_codex", oauth("oauth-old", None, Some(1)))]),
            runtime: |runtime| runtime,
            provider: Some("openai_codex"),
            slot: Some(CredentialSlot::LocalOAuth {
                empty: false,
                expired: true,
                refreshable: false,
            }),
            skip: None,
            fingerprint_differs: false,
            resolution: Resolution::AuthRequired,
        },
        Case {
            name: "a saved key with another fingerprint the declaration keeps out",
            requested: "auto",
            model: DEFAULT_MODEL,
            providers: ProvidersConfig::new(),
            auth: stale_fingerprint,
            runtime: |runtime| {
                runtime.with_declaration(
                    "anthropic",
                    CredentialSourceDeclaration {
                        local_auth: false,
                        ..unset_declaration()
                    },
                )
            },
            provider: Some("anthropic"),
            slot: None,
            skip: None,
            fingerprint_differs: true,
            resolution: Resolution::AuthRequired,
        },
        Case {
            name: "codex with an API key entry",
            requested: "openai_codex",
            model: "gpt-5.4",
            providers: ProvidersConfig::new(),
            auth: auth_with(&[("openai_codex", ProviderAuth::api_key("wrong-kind"))]),
            runtime: |runtime| runtime,
            provider: Some("openai_codex"),
            slot: None,
            skip: Some(LocalAuthSkip::KindMismatch),
            fingerprint_differs: false,
            resolution: Resolution::AuthRequired,
        },
        Case {
            name: "codex with an API key entry and another fingerprint",
            requested: "openai_codex",
            model: "gpt-5.4",
            providers: ProvidersConfig::new(),
            auth: stale_kind,
            runtime: |runtime| runtime,
            provider: Some("openai_codex"),
            slot: None,
            skip: Some(LocalAuthSkip::KindMismatch),
            fingerprint_differs: true,
            resolution: Resolution::AuthRequired,
        },
    ]
}

fn runtime_at(root: &Path, refresher: Arc<CountingRefresher>) -> ProviderCredentialRuntime {
    ProviderCredentialRuntime::new(
        root.join("auth.json"),
        root,
        Spec030FactStore::new(WorkspaceTrustObservation::Trusted),
    )
    .with_oauth_refresher(refresher)
}

fn request<'a>(
    registry: &'a ProviderRegistry,
    case: &'a Case,
) -> ProviderClientResolutionRequest<'a> {
    ProviderClientResolutionRequest {
        registry,
        requested_provider: case.requested,
        model: case.model,
        providers: &case.providers,
    }
}

/// The runtime's own selection and resolution for the case.
fn resolve(
    runtime: &ProviderCredentialRuntime,
    registry: &ProviderRegistry,
    case: &Case,
) -> Result<Resolution, Box<dyn Error>> {
    let provider_id = match runtime.resolve_provider_client(request(registry, case)) {
        Ok(client) => client.provider_id,
        Err(ProviderError::ProviderNotFound { .. }) => return Ok(Resolution::ProviderNotFound),
        Err(ProviderError::AuthRequired { .. }) => return Ok(Resolution::AuthRequired),
        Err(error) => return Err(error.into()),
    };
    let spec = registry.find_by_name(&provider_id).ok_or("provider spec")?;
    let config = case
        .providers
        .get(&provider_id)
        .cloned()
        .unwrap_or_default();
    let declaration = runtime.declaration_for(&provider_id, &config, spec);
    let credential = runtime
        .resolve_credential_observed(
            &provider_id,
            &config,
            &declaration,
            &ProviderCredentialInvocation::default(),
            &|| {},
        )
        .map_err(|error| format!("{error}"))?;
    Ok(Resolution::Resolved {
        provider: provider_id,
        source: credential.source(),
        value: credential.transport().value().to_owned(),
    })
}

#[test]
fn the_plan_matches_runtime_resolution_case_by_case() -> TestResult {
    let registry = ProviderRegistry::new();
    for case in cases() {
        // Given
        let root = tempfile::tempdir()?;
        save_auth_store_to_path(&case.auth, &root.path().join("auth.json"))?;
        let refresher = Arc::new(CountingRefresher(AtomicUsize::new(0)));
        let runtime = (case.runtime)(runtime_at(root.path(), refresher));

        // When
        let plan = runtime.plan_provider_credential(request(&registry, &case));
        let resolution = resolve(&runtime, &registry, &case)?;

        // Then
        assert!(!plan.unknown_provider, "{}", case.name);
        assert!(!plan.auth_store_unreadable, "{}", case.name);
        let selected = plan.selected.as_ref();
        assert_eq!(
            selected.map(|provider| provider.provider_id.as_str()),
            case.provider,
            "{}",
            case.name
        );
        assert_eq!(
            selected.and_then(|provider| provider.slot.clone()),
            case.slot,
            "{}",
            case.name
        );
        assert_eq!(
            selected.and_then(|provider| provider.local_auth_skip),
            case.skip,
            "{}",
            case.name
        );
        assert_eq!(
            selected.is_some_and(|provider| provider.fingerprint_differs),
            case.fingerprint_differs,
            "{}",
            case.name
        );
        assert_eq!(resolution, case.resolution, "{}", case.name);
    }
    Ok(())
}

#[test]
fn an_unknown_provider_and_an_unreadable_auth_store_are_reported() -> TestResult {
    // Given
    let root = tempfile::tempdir()?;
    fs::write(root.path().join("auth.json"), "{not json")?;
    let runtime = runtime_at(
        root.path(),
        Arc::new(CountingRefresher(AtomicUsize::new(0))),
    );
    let registry = ProviderRegistry::new();
    let providers = ProvidersConfig::new();

    // When
    let unknown = runtime.plan_provider_credential(ProviderClientResolutionRequest {
        registry: &registry,
        requested_provider: "no-such-provider",
        model: DEFAULT_MODEL,
        providers: &providers,
    });
    let codex = runtime.plan_provider_credential(ProviderClientResolutionRequest {
        registry: &registry,
        requested_provider: "openai_codex",
        model: "gpt-5.4",
        providers: &providers,
    });

    // Then
    assert!(unknown.unknown_provider);
    assert_eq!(unknown.selected, None);
    let slot = codex.selected.and_then(|provider| provider.slot);
    assert_eq!(slot, Some(CredentialSlot::LocalAuthUnreadable));
    assert!(codex.auth_store_unreadable);
    Ok(())
}

#[test]
fn planning_runs_no_command_refreshes_nothing_and_writes_nothing() -> TestResult {
    // Given: an expired sign-in to refresh and a command that leaves a mark.
    let root = tempfile::tempdir()?;
    let auth_path = root.path().join("auth.json");
    let marker = root.path().join("command-ran");
    save_auth_store_to_path(
        &auth_with(&[("openai_codex", oauth("oauth-old", Some("refresh"), Some(1)))]),
        &auth_path,
    )?;
    let auth_before = fs::read(&auth_path)?;
    let refresher = Arc::new(CountingRefresher(AtomicUsize::new(0)));
    let facts = Spec030FactStore::new(WorkspaceTrustObservation::Trusted);
    let runtime = ProviderCredentialRuntime::new(&auth_path, root.path(), facts.clone())
        .with_oauth_refresher(refresher.clone())
        .with_declaration(
            "openai",
            CredentialSourceDeclaration {
                family: CredentialFamily::ApiKey,
                environment: None,
                local_auth: false,
                command: Some(format!("touch '{}'; printf key", marker.display())),
            },
        );
    let credential_before = LocalSpec030ProjectionProvider::new(facts.clone())
        .projection()
        .credential()
        .clone();
    let registry = ProviderRegistry::new();
    let providers = ProvidersConfig::new();

    // When
    let plans: Vec<ProviderCredentialPlan> = [("openai_codex", "gpt-5.4"), ("openai", "gpt-4o")]
        .into_iter()
        .map(|(requested, model)| {
            runtime.plan_provider_credential(ProviderClientResolutionRequest {
                registry: &registry,
                requested_provider: requested,
                model,
                providers: &providers,
            })
        })
        .collect();

    // Then
    assert_eq!(plans.len(), 2);
    assert_eq!(fs::read(&auth_path)?, auth_before);
    assert!(!root.path().join("auth.json.lock").exists());
    assert!(!marker.exists());
    assert_eq!(refresher.0.load(Ordering::SeqCst), 0);
    let credential_after = LocalSpec030ProjectionProvider::new(facts)
        .projection()
        .credential()
        .clone();
    assert_eq!(credential_after, credential_before);
    Ok(())
}

#[test]
fn a_credential_command_still_runs_once_for_repeated_resolutions() -> TestResult {
    // Given
    let root = tempfile::tempdir()?;
    let marker = root.path().join("command-runs");
    let runtime = runtime_at(
        root.path(),
        Arc::new(CountingRefresher(AtomicUsize::new(0))),
    )
    .with_declaration(
        "openai",
        CredentialSourceDeclaration {
            family: CredentialFamily::ApiKey,
            environment: None,
            local_auth: false,
            command: Some(format!(
                "echo ran >> '{}'; printf command-key",
                marker.display()
            )),
        },
    );
    let config = ProviderConfig::default();
    let spec = ProviderRegistry::new()
        .find_by_name("openai")
        .copied()
        .ok_or("openai spec")?;
    let declaration = runtime.declaration_for("openai", &config, &spec);

    // When
    let sources = (0..2)
        .map(|_| {
            runtime
                .resolve_credential_observed(
                    "openai",
                    &config,
                    &declaration,
                    &ProviderCredentialInvocation::default(),
                    &|| {},
                )
                .map(|credential| credential.source())
                .map_err(|error| format!("{error}"))
        })
        .collect::<Result<Vec<_>, _>>()?;

    // Then
    assert_eq!(
        sources,
        [CredentialSource::Command, CredentialSource::Command]
    );
    assert_eq!(fs::read_to_string(&marker)?.lines().count(), 1);
    Ok(())
}

#[test]
fn a_sign_in_that_turns_stale_under_the_lock_falls_through_to_the_next_place() -> TestResult {
    // Given: a current Codex sign-in when the place is chosen, made stale
    // before it is read.
    let root = tempfile::tempdir()?;
    let auth_path = root.path().join("auth.json");
    let current = auth_with(&[("openai_codex", oauth("oauth-access", None, None))]);
    save_auth_store_to_path(&current, &auth_path)?;
    let runtime = runtime_at(
        root.path(),
        Arc::new(CountingRefresher(AtomicUsize::new(0))),
    );
    let config = ProviderConfig {
        api_key: Some("literal-key".to_owned()),
        ..ProviderConfig::default()
    };
    let spec = ProviderRegistry::new()
        .find_by_name("openai_codex")
        .copied()
        .ok_or("codex spec")?;
    let declaration = runtime.declaration_for("openai_codex", &config, &spec);
    let mut stale = current.clone();
    stale.set_fingerprint(
        "openai_codex",
        CredentialFingerprint::from_descriptor("old"),
    );
    let make_stale = || {
        save_auth_store_to_path(&stale, &auth_path).expect("auth store is writable");
    };

    // When
    let credential = runtime
        .resolve_credential_observed(
            "openai_codex",
            &config,
            &declaration,
            &ProviderCredentialInvocation::default(),
            &make_stale,
        )
        .map_err(|error| format!("{error}"))?;

    // Then
    assert_eq!(credential.source(), CredentialSource::ProviderLiteral);
    assert_eq!(credential.transport().value(), "literal-key");
    Ok(())
}
