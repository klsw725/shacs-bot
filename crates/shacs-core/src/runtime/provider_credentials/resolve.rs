use super::command::runtime_error;
use super::{
    ProviderClientResolutionRequest, ProviderConfigResolutionRequest, ProviderCredentialInvocation,
    ProviderCredentialRuntime, ResolvedProviderConfig,
};
use shacs_config::{
    CredentialFamily, CredentialFingerprintStatus, CredentialResolutionInput, CredentialSource,
    CredentialSourceDeclaration, CredentialStatus, CredentialStatusSnapshot, LocalAuthStore,
    RawCredential, RefreshSerializationStatus, ResolvedCredential,
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
        let selection = self.providers_for_selection(request.providers);
        let provider_match = request
            .registry
            .match_provider(request.requested_provider, request.model, &selection)
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
        let declaration = self
            .declarations
            .get(&provider_match.provider_id)
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
            });
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
        let mut no_literal = config.clone();
        no_literal.api_key = None;
        let top = declaration.resolve(
            &no_literal,
            CredentialResolutionInput {
                runtime_override: invocation
                    .runtime_override
                    .clone()
                    .or_else(|| self.runtime_overrides.get(provider_id).cloned()),
                environment: declaration.environment.as_ref().and_then(|name| {
                    self.environment
                        .get(name)
                        .cloned()
                        .or_else(|| std::env::var(name).ok())
                        .map(|value| credential(declaration.family, value))
                }),
                ..CredentialResolutionInput::default()
            },
        );
        if let Ok(resolved) = top {
            return Ok(resolved);
        }
        if declaration.local_auth {
            match self.resolve_local(provider_id, declaration) {
                Ok(Some(resolved)) => return Ok(resolved),
                Ok(None) => {}
                Err(error) if error.status().fingerprint_stale => {}
                Err(error) => return Err(error),
            }
        }
        if let Some(command) = declaration.command.as_deref() {
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
                &no_literal,
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
            return result;
        }
        declaration.resolve(config, CredentialResolutionInput::default())
    }

    pub(crate) fn providers_for_selection(
        &self,
        providers: &shacs_providers::ProvidersConfig,
    ) -> shacs_providers::ProvidersConfig {
        let mut selection = providers.clone();
        if let Ok(auth) = LocalAuthStore::new(self.auth_path()).load() {
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
        selection
    }
}

fn credential(family: CredentialFamily, access: String) -> RawCredential {
    match family {
        CredentialFamily::ApiKey => RawCredential::api_key(access),
        CredentialFamily::OAuth => RawCredential::oauth(access, None, None),
    }
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
