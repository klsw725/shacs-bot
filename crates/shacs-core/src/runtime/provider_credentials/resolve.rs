use super::command::runtime_error;
use super::provider_credential_plan::{CredentialChoice, CredentialSlot, SlotSearch};
use super::{
    ProviderClientResolutionRequest, ProviderConfigResolutionRequest, ProviderCredentialInvocation,
    ProviderCredentialRuntime, ResolvedProviderConfig,
};
use shacs_config::{
    CredentialFamily, CredentialFingerprintStatus, CredentialResolutionInput, CredentialSource,
    CredentialSourceDeclaration, CredentialStatus, CredentialStatusSnapshot, RawCredential,
    RefreshSerializationStatus, ResolvedCredential,
};
use shacs_providers::{
    provider_client_from_config, ProviderConfig, ProviderError, ProviderRegistry,
    ResolvedProviderClient,
};

impl ProviderCredentialRuntime {
    pub fn resolve_provider_client(
        &self,
        request: ProviderClientResolutionRequest<'_>,
    ) -> Result<ResolvedProviderClient, ProviderError> {
        self.resolve_provider_client_for_invocation(
            request,
            &ProviderCredentialInvocation {
                runtime_override: None,
                command_abort: self.command_abort.clone(),
            },
        )
    }

    pub fn resolve_provider_client_for_invocation(
        &self,
        request: ProviderClientResolutionRequest<'_>,
        invocation: &ProviderCredentialInvocation,
    ) -> Result<ResolvedProviderClient, ProviderError> {
        let provider_match = self
            .select_provider(&request)
            .provider_match
            .ok_or_else(|| provider_not_found(request.registry, request.requested_provider))?;
        let resolved = self.resolve_provider_config_for_invocation(
            ProviderConfigResolutionRequest {
                registry: request.registry,
                provider_match,
                providers: request.providers,
            },
            invocation,
        )?;
        Ok(ResolvedProviderClient {
            provider_id: resolved.provider_id,
            model: resolved.model,
            client: provider_client_from_config(resolved.config, &resolved.spec)?,
        })
    }

    pub(crate) fn resolve_provider_config(
        &self,
        request: ProviderConfigResolutionRequest<'_>,
    ) -> Result<ResolvedProviderConfig, ProviderError> {
        self.resolve_provider_config_for_invocation(
            request,
            &ProviderCredentialInvocation {
                runtime_override: None,
                command_abort: self.command_abort.clone(),
            },
        )
    }

    pub(crate) fn resolve_provider_config_for_invocation(
        &self,
        request: ProviderConfigResolutionRequest<'_>,
        invocation: &ProviderCredentialInvocation,
    ) -> Result<ResolvedProviderConfig, ProviderError> {
        let provider_match = request.provider_match;
        let spec = request
            .registry
            .find_by_name(&provider_match.provider_id)
            .copied()
            .ok_or_else(|| provider_not_found(request.registry, &provider_match.provider_id))?;
        let config = request
            .providers
            .get(&provider_match.provider_id)
            .cloned()
            .unwrap_or_default();
        let declaration = self.declaration_for(&provider_match.provider_id, &config, &spec);
        let resolved = self.resolve_credential(
            &provider_match.provider_id,
            &config,
            &declaration,
            invocation,
        );
        match resolved {
            Ok(resolved) => {
                let status = resolved.status();
                let account_id = resolved.account_id().map(str::to_owned);
                let mut config = config;
                config.api_key = Some(resolved.transport().value().to_owned());
                if let Some(headers) = config.extra_headers.as_mut() {
                    headers.retain(|name, _| !name.eq_ignore_ascii_case("authorization"));
                }
                if provider_match.provider_id == "openai_codex" {
                    let headers = config.extra_headers.get_or_insert_with(Default::default);
                    headers.retain(|name, _| !name.eq_ignore_ascii_case("chatgpt-account-id"));
                    if status.source == Some(CredentialSource::LocalAuthStore) {
                        if let Some(account_id) = account_id {
                            headers.insert("ChatGPT-Account-Id".to_owned(), account_id);
                        }
                    }
                }
                self.facts
                    .record_credential_status(status)
                    .map_err(|_| runtime_error("credential fact update failed"))?;
                Ok(ResolvedProviderConfig {
                    provider_id: provider_match.provider_id,
                    model: provider_match.model,
                    spec,
                    config,
                })
            }
            Err(error) => {
                self.facts
                    .record_credential_status(error.status())
                    .map_err(|_| runtime_error("credential fact update failed"))?;
                Err(ProviderError::AuthRequired {
                    provider_id: provider_match.provider_id,
                })
            }
        }
    }

    fn resolve_credential(
        &self,
        provider_id: &str,
        config: &ProviderConfig,
        declaration: &CredentialSourceDeclaration,
        invocation: &ProviderCredentialInvocation,
    ) -> Result<ResolvedCredential, shacs_config::CredentialError> {
        self.resolve_credential_observed(provider_id, config, declaration, invocation, &|| {})
    }

    /// Chooses the place (Spec 052 PRD 000 scope 1) and reads the value only
    /// there. A local OAuth credential found stale under the store's lock (it
    /// changed after the place was chosen) is skipped like before, and the
    /// search goes on after the auth store. `after_choice` runs between the
    /// two steps, where a test changes the auth store.
    pub(super) fn resolve_credential_observed(
        &self,
        provider_id: &str,
        config: &ProviderConfig,
        declaration: &CredentialSourceDeclaration,
        invocation: &ProviderCredentialInvocation,
        after_choice: &dyn Fn(),
    ) -> Result<ResolvedCredential, shacs_config::CredentialError> {
        let choice = self.choose_slot(
            provider_id,
            config,
            declaration,
            invocation.runtime_override.as_ref(),
            SlotSearch::FromStart,
        );
        after_choice();
        let local_oauth = matches!(choice.slot, Some(CredentialSlot::LocalOAuth { .. }));
        match self.read_choice(provider_id, config, declaration, invocation, choice) {
            Err(error) if local_oauth && error.status().fingerprint_stale => {
                let rest = self.choose_slot(
                    provider_id,
                    config,
                    declaration,
                    None,
                    SlotSearch::AfterLocalAuth,
                );
                self.read_choice(provider_id, config, declaration, invocation, rest)
            }
            result => result,
        }
    }

    fn read_choice(
        &self,
        provider_id: &str,
        config: &ProviderConfig,
        declaration: &CredentialSourceDeclaration,
        invocation: &ProviderCredentialInvocation,
        choice: CredentialChoice,
    ) -> Result<ResolvedCredential, shacs_config::CredentialError> {
        let mut no_literal = config.clone();
        no_literal.api_key = None;
        match choice.slot {
            Some(CredentialSlot::RuntimeOverride) => declaration.resolve(
                &no_literal,
                CredentialResolutionInput {
                    runtime_override: choice.value,
                    ..CredentialResolutionInput::default()
                },
            ),
            Some(CredentialSlot::Environment { .. }) => declaration.resolve(
                &no_literal,
                CredentialResolutionInput {
                    environment: choice.value,
                    ..CredentialResolutionInput::default()
                },
            ),
            Some(CredentialSlot::LocalApiKey { .. }) => declaration.resolve(
                &ProviderConfig::default(),
                CredentialResolutionInput {
                    local_auth: choice.value,
                    local_auth_fingerprint: choice.local_fingerprint,
                    ..CredentialResolutionInput::default()
                },
            ),
            Some(CredentialSlot::LocalOAuth { .. }) => {
                self.read_local_oauth(provider_id, declaration)
            }
            Some(CredentialSlot::LocalAuthUnreadable) => Err(local_auth_unreadable()),
            Some(CredentialSlot::Command) => match declaration.command.as_deref() {
                Some(command) => {
                    self.read_command(provider_id, command, &no_literal, declaration, invocation)
                }
                None => declaration.resolve(config, CredentialResolutionInput::default()),
            },
            Some(CredentialSlot::ProviderLiteral) | None => {
                declaration.resolve(config, CredentialResolutionInput::default())
            }
        }
    }

    fn read_command(
        &self,
        provider_id: &str,
        command: &str,
        no_literal: &ProviderConfig,
        declaration: &CredentialSourceDeclaration,
        invocation: &ProviderCredentialInvocation,
    ) -> Result<ResolvedCredential, shacs_config::CredentialError> {
        let input = self
            .command_input(provider_id, command, &invocation.command_abort)
            .map_err(|_| {
                shacs_config::CredentialError::from_status(
                    CredentialStatusSnapshot {
                        status: CredentialStatus::Missing,
                        source: Some(CredentialSource::Command),
                        fingerprint: CredentialFingerprintStatus::Current,
                        fingerprint_stale: false,
                        refresh_serialization: RefreshSerializationStatus::Inactive,
                    },
                    "credential command execution failed",
                )
            })?;
        let result = declaration.resolve(
            no_literal,
            CredentialResolutionInput {
                command: input,
                ..CredentialResolutionInput::default()
            },
        );
        if let Ok(resolved) = &result {
            let raw = credential(declaration.family, resolved.transport().value().to_owned());
            if let Ok(mut cache) = self.command_cache.lock() {
                cache.insert(provider_id.to_owned(), raw);
            }
        }
        result
    }
}

pub(super) fn credential(family: CredentialFamily, access: String) -> RawCredential {
    match family {
        CredentialFamily::ApiKey => RawCredential::api_key(access),
        CredentialFamily::OAuth => RawCredential::oauth(access, None, None),
    }
}

fn local_auth_unreadable() -> shacs_config::CredentialError {
    shacs_config::CredentialError::from_status(
        CredentialStatusSnapshot {
            status: CredentialStatus::Missing,
            source: Some(CredentialSource::LocalAuthStore),
            fingerprint: CredentialFingerprintStatus::Unavailable,
            fingerprint_stale: false,
            refresh_serialization: RefreshSerializationStatus::Active,
        },
        "local auth store could not be read",
    )
}

fn provider_not_found(registry: &ProviderRegistry, provider_id: &str) -> ProviderError {
    ProviderError::ProviderNotFound {
        provider_id: provider_id.to_owned(),
        suggestions: registry
            .specs()
            .iter()
            .map(|spec| spec.name.to_owned())
            .collect(),
    }
}
