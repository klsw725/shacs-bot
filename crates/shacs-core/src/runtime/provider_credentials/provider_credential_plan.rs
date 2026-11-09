//! Which provider the runtime picks and where it takes that provider's
//! credential from, decided from config, the credential declaration, the
//! environment and the auth store without running a command, refreshing or
//! writing anything (Spec 052 PRD 000). Runtime resolution goes through the
//! same two steps and then reads the value at the chosen place.

use super::{ProviderClientResolutionRequest, ProviderCredentialRuntime};
use shacs_config::{
    AuthStore, CredentialFamily, CredentialFingerprint, CredentialSourceDeclaration,
    LocalAuthStore, RawCredential,
};
use shacs_providers::{ProviderConfig, ProviderMatch, ProviderSpec, ProvidersConfig};

/// What [`ProviderCredentialRuntime::plan_provider_credential`] found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderCredentialPlan {
    /// `agents.defaults.provider` is neither `auto` nor a provider the
    /// registry knows.
    pub unknown_provider: bool,
    /// The provider the runtime would pick, if any.
    pub selected: Option<PlannedProvider>,
    /// The auth store could not be read, for picking or for the credential.
    pub auth_store_unreadable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedProvider {
    pub provider_id: String,
    pub model: String,
    pub family: CredentialFamily,
    /// The environment variable the declaration names.
    pub declared_environment: Option<String>,
    /// Whether the declaration lets the auth store supply the credential.
    pub local_auth_allowed: bool,
    /// Where the runtime takes the credential from; `None` when resolution
    /// fails for want of one.
    pub slot: Option<CredentialSlot>,
    /// Why the auth store did not supply it, when it was looked at.
    pub local_auth_skip: Option<LocalAuthSkip>,
    /// The auth store keeps a declaration fingerprint for this provider that
    /// differs from the current declaration, whatever its entry's kind.
    /// Logging in or importing again does not clear it.
    pub fingerprint_differs: bool,
}

/// A place the credential comes from, with only what judging it needs.
/// Empty means nothing is left after trimming.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CredentialSlot {
    RuntimeOverride,
    Environment {
        name: String,
        empty: bool,
    },
    LocalApiKey {
        empty: bool,
    },
    LocalOAuth {
        empty: bool,
        expired: bool,
        refreshable: bool,
    },
    /// The auth store could not be read; resolution fails here.
    LocalAuthUnreadable,
    /// Not run when planning.
    Command,
    ProviderLiteral,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocalAuthSkip {
    NoEntry,
    KindMismatch,
    FingerprintStale,
}

/// Where the slot search starts: at the top, or after the auth store when it
/// turned out stale under its lock.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SlotSearch {
    FromStart,
    AfterLocalAuth,
}

/// The chosen place with the values resolution reads there.
#[derive(Default)]
pub(super) struct CredentialChoice {
    pub(super) slot: Option<CredentialSlot>,
    /// The override, environment or saved API key credential.
    pub(super) value: Option<RawCredential>,
    pub(super) local_fingerprint: Option<CredentialFingerprint>,
    pub(super) local_auth_skip: Option<LocalAuthSkip>,
    pub(super) store_unreadable: bool,
}

impl CredentialChoice {
    fn at(mut self, slot: CredentialSlot, value: Option<RawCredential>) -> Self {
        self.slot = Some(slot);
        self.value = value;
        self
    }
}

/// The provider runtime selection picked and the auth store it read, `None`
/// when that read failed (the store then adds no candidates).
pub(super) struct ProviderSelection {
    pub(super) provider_match: Option<ProviderMatch>,
    pub(super) auth_store: Option<AuthStore>,
}

impl ProviderCredentialRuntime {
    /// The provider the runtime would pick for `request` and where it would
    /// take that provider's credential from. Reads the auth store without its
    /// lock; runs no command, refreshes nothing and writes nothing, not even
    /// a credential fact. A per-call runtime override is not looked at.
    pub fn plan_provider_credential(
        &self,
        request: ProviderClientResolutionRequest<'_>,
    ) -> ProviderCredentialPlan {
        let ProviderSelection {
            provider_match,
            auth_store,
        } = self.select_provider(&request);
        let unknown_provider = request.requested_provider != "auto"
            && request
                .registry
                .find_by_name(request.requested_provider)
                .is_none();
        let mut auth_store_unreadable = auth_store.is_none();
        let selected = provider_match.and_then(|provider_match| {
            let spec = request.registry.find_by_name(&provider_match.provider_id)?;
            let config = request
                .providers
                .get(&provider_match.provider_id)
                .cloned()
                .unwrap_or_default();
            let declaration = self.declaration_for(&provider_match.provider_id, &config, spec);
            let choice = self.choose_slot(
                &provider_match.provider_id,
                &config,
                &declaration,
                None,
                SlotSearch::FromStart,
            );
            auth_store_unreadable |= choice.store_unreadable;
            // From the selection's read: the slot search reads the store only
            // when it gets that far and the declaration allows it.
            let fingerprint_differs = auth_store
                .as_ref()
                .and_then(|store| store.fingerprint(&provider_match.provider_id))
                .is_some_and(|fingerprint| fingerprint != declaration.fingerprint());
            Some(PlannedProvider {
                provider_id: provider_match.provider_id,
                model: provider_match.model,
                family: declaration.family,
                declared_environment: declaration.environment,
                local_auth_allowed: declaration.local_auth,
                slot: choice.slot,
                local_auth_skip: choice.local_auth_skip,
                fingerprint_differs,
            })
        });
        ProviderCredentialPlan {
            unknown_provider,
            selected,
            auth_store_unreadable,
        }
    }

    pub(super) fn select_provider(
        &self,
        request: &ProviderClientResolutionRequest<'_>,
    ) -> ProviderSelection {
        let candidates = self.selection_candidates(request.providers);
        ProviderSelection {
            provider_match: request.registry.match_provider(
                request.requested_provider,
                request.model,
                &candidates.providers,
            ),
            auth_store: candidates.auth_store,
        }
    }

    pub(crate) fn providers_for_selection(&self, providers: &ProvidersConfig) -> ProvidersConfig {
        self.selection_candidates(providers).providers
    }

    fn selection_candidates(&self, providers: &ProvidersConfig) -> SelectionCandidates {
        let mut selection = providers.clone();
        let auth_store = LocalAuthStore::new(self.auth_path()).load().ok();
        if let Some(auth) = &auth_store {
            for provider_id in auth.providers.keys() {
                selection.entry(provider_id.clone()).or_default();
            }
        }
        for provider_id in self
            .runtime_overrides
            .keys()
            .chain(self.declarations.keys())
        {
            selection.entry(provider_id.clone()).or_default();
        }
        for config in selection.values_mut() {
            if config.api_key.is_none() {
                config.api_key = Some("credential-resolution-pending".to_owned());
            }
        }
        SelectionCandidates {
            providers: selection,
            auth_store,
        }
    }

    pub(super) fn declaration_for(
        &self,
        provider_id: &str,
        config: &ProviderConfig,
        spec: &ProviderSpec,
    ) -> CredentialSourceDeclaration {
        self.declarations
            .get(provider_id)
            .cloned()
            .unwrap_or_else(|| {
                config.credential_declaration(
                    if spec.is_oauth {
                        CredentialFamily::OAuth
                    } else {
                        CredentialFamily::ApiKey
                    },
                    spec.env_key,
                )
            })
    }

    /// The first place, in runtime order, that supplies the credential:
    /// override, declared environment variable (present even if empty), auth
    /// store, credential command, then a non-empty config `apiKey`.
    pub(super) fn choose_slot(
        &self,
        provider_id: &str,
        config: &ProviderConfig,
        declaration: &CredentialSourceDeclaration,
        invocation_override: Option<&RawCredential>,
        search: SlotSearch,
    ) -> CredentialChoice {
        let mut choice = CredentialChoice::default();
        if search == SlotSearch::FromStart {
            let runtime_override = invocation_override
                .cloned()
                .or_else(|| self.runtime_overrides.get(provider_id).cloned())
                .filter(|raw| family_of(raw) == declaration.family);
            if let Some(raw) = runtime_override {
                return choice.at(CredentialSlot::RuntimeOverride, Some(raw));
            }
            if let Some(name) = declaration.environment.as_deref() {
                if let Some(value) = self.environment_value(name) {
                    let slot = CredentialSlot::Environment {
                        name: name.to_owned(),
                        empty: value.trim().is_empty(),
                    };
                    return choice.at(
                        slot,
                        Some(super::resolve::credential(declaration.family, value)),
                    );
                }
            }
            if declaration.local_auth {
                let Ok(store) = LocalAuthStore::new(self.auth_path()).load() else {
                    choice.store_unreadable = true;
                    return choice.at(CredentialSlot::LocalAuthUnreadable, None);
                };
                let stored_fingerprint = store.fingerprint(provider_id);
                let fingerprint_differs = stored_fingerprint
                    .as_ref()
                    .is_some_and(|fingerprint| *fingerprint != declaration.fingerprint());
                match store.providers.get(provider_id) {
                    None => choice.local_auth_skip = Some(LocalAuthSkip::NoEntry),
                    Some(entry) => match (declaration.family, entry.kind.as_str()) {
                        (CredentialFamily::ApiKey, "apiKey")
                        | (CredentialFamily::OAuth, "oauth")
                            if fingerprint_differs =>
                        {
                            choice.local_auth_skip = Some(LocalAuthSkip::FingerprintStale);
                        }
                        (CredentialFamily::ApiKey, "apiKey") => {
                            let slot = CredentialSlot::LocalApiKey {
                                empty: entry.access.trim().is_empty(),
                            };
                            choice.local_fingerprint = stored_fingerprint;
                            return choice.at(slot, Some(RawCredential::api_key(&entry.access)));
                        }
                        (CredentialFamily::OAuth, "oauth") => {
                            let now = super::local::now_millis();
                            let slot = CredentialSlot::LocalOAuth {
                                empty: entry.access.trim().is_empty(),
                                expired: entry.expires.is_some_and(|expires| expires <= now),
                                refreshable: entry.refresh.is_some(),
                            };
                            return choice.at(slot, None);
                        }
                        _ => choice.local_auth_skip = Some(LocalAuthSkip::KindMismatch),
                    },
                }
            }
        }
        if declaration.command.is_some() {
            return choice.at(CredentialSlot::Command, None);
        }
        if config
            .api_key
            .as_deref()
            .is_some_and(|value| !value.trim().is_empty())
        {
            return choice.at(CredentialSlot::ProviderLiteral, None);
        }
        choice
    }

    fn environment_value(&self, name: &str) -> Option<String> {
        self.environment
            .get(name)
            .cloned()
            .or_else(|| std::env::var(name).ok())
    }
}

struct SelectionCandidates {
    providers: ProvidersConfig,
    auth_store: Option<AuthStore>,
}

fn family_of(raw: &RawCredential) -> CredentialFamily {
    match raw {
        RawCredential::ApiKey { .. } => CredentialFamily::ApiKey,
        RawCredential::OAuth { .. } => CredentialFamily::OAuth,
    }
}

#[cfg(test)]
#[path = "provider_credential_plan_tests.rs"]
mod tests;
