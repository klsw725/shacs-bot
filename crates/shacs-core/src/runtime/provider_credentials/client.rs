use super::{
    ProviderClientResolutionRequest, ProviderCredentialClientConfig, ProviderCredentialInvocation,
    ProviderCredentialRuntime,
};
use shacs_providers::{
    LlmResponse, ProviderClient, ProviderError, ProviderEvent, ProviderInvocation,
    ProviderRegistry, ProviderRequest, ProvidersConfig,
};
use std::sync::Arc;

pub struct CredentialResolvingProviderClient {
    config: ProviderCredentialClientConfig,
    runtime: Arc<ProviderCredentialRuntime>,
    invocation: ProviderCredentialInvocation,
    transport_override: Option<Arc<dyn ProviderClient>>,
}

pub(crate) struct ProviderInvocationClient<'a> {
    client: &'a dyn ProviderClient,
    invocation: &'a ProviderInvocation,
}

impl<'a> ProviderInvocationClient<'a> {
    pub(crate) const fn new(
        client: &'a dyn ProviderClient,
        invocation: &'a ProviderInvocation,
    ) -> Self {
        Self { client, invocation }
    }
}

impl ProviderClient for ProviderInvocationClient<'_> {
    fn chat(&self, request: ProviderRequest) -> Result<LlmResponse, ProviderError> {
        self.client.chat_with_invocation(request, self.invocation)
    }

    fn chat_stream(
        &self,
        request: ProviderRequest,
        on_event: &mut dyn FnMut(ProviderEvent),
    ) -> Result<LlmResponse, ProviderError> {
        self.client
            .chat_stream_with_invocation(request, on_event, self.invocation)
    }
}

impl CredentialResolvingProviderClient {
    pub fn new(
        requested_provider: impl Into<String>,
        model: impl Into<String>,
        providers: ProvidersConfig,
        runtime: Arc<ProviderCredentialRuntime>,
    ) -> Self {
        Self::from_config(
            ProviderCredentialClientConfig {
                requested_provider: requested_provider.into(),
                model: model.into(),
                providers,
            },
            runtime,
        )
    }

    fn from_config(
        config: ProviderCredentialClientConfig,
        runtime: Arc<ProviderCredentialRuntime>,
    ) -> Self {
        Self {
            config,
            runtime,
            invocation: ProviderCredentialInvocation::default(),
            transport_override: None,
        }
    }

    pub fn with_invocation(mut self, invocation: ProviderCredentialInvocation) -> Self {
        self.invocation = invocation;
        self
    }

    pub fn with_transport_override(mut self, transport: Arc<dyn ProviderClient>) -> Self {
        self.transport_override = Some(transport);
        self
    }

    fn resolve(&self) -> Result<shacs_providers::ResolvedProviderClient, ProviderError> {
        self.resolve_with(&self.invocation)
    }

    fn resolve_for(
        &self,
        invocation: &ProviderInvocation,
    ) -> Result<shacs_providers::ResolvedProviderClient, ProviderError> {
        self.resolve_with(&ProviderCredentialInvocation::new(
            invocation.runtime_override().cloned(),
            crate::controlled_child::ControlledChildAbort::from_flag(
                invocation.cancellation_flag(),
            ),
        ))
    }

    fn resolve_with(
        &self,
        invocation: &ProviderCredentialInvocation,
    ) -> Result<shacs_providers::ResolvedProviderClient, ProviderError> {
        let registry = ProviderRegistry::new();
        self.runtime.resolve_provider_client_for_invocation(
            ProviderClientResolutionRequest {
                registry: &registry,
                requested_provider: &self.config.requested_provider,
                model: &self.config.model,
                providers: &self.config.providers,
            },
            invocation,
        )
    }

    fn chat_resolved(
        &self,
        request: ProviderRequest,
        invocation: Option<&ProviderInvocation>,
    ) -> Result<LlmResponse, ProviderError> {
        let resolved = match invocation {
            Some(invocation) => self.resolve_for(invocation)?,
            None => self.resolve()?,
        };
        match &self.transport_override {
            Some(transport) => match invocation {
                Some(invocation) => transport.chat_with_invocation(request, invocation),
                None => transport.chat(request),
            },
            None => match invocation {
                Some(invocation) => resolved.client.chat_with_invocation(request, invocation),
                None => resolved.client.chat(request),
            },
        }
    }

    fn chat_stream_resolved(
        &self,
        request: ProviderRequest,
        on_event: &mut dyn FnMut(ProviderEvent),
        invocation: Option<&ProviderInvocation>,
    ) -> Result<LlmResponse, ProviderError> {
        let resolved = match invocation {
            Some(invocation) => self.resolve_for(invocation)?,
            None => self.resolve()?,
        };
        match &self.transport_override {
            Some(transport) => match invocation {
                Some(invocation) => {
                    transport.chat_stream_with_invocation(request, on_event, invocation)
                }
                None => transport.chat_stream(request, on_event),
            },
            None => match invocation {
                Some(invocation) => resolved
                    .client
                    .chat_stream_with_invocation(request, on_event, invocation),
                None => resolved.client.chat_stream(request, on_event),
            },
        }
    }
}

impl ProviderClient for CredentialResolvingProviderClient {
    fn chat(&self, request: ProviderRequest) -> Result<LlmResponse, ProviderError> {
        self.chat_resolved(request, None)
    }

    fn chat_with_invocation(
        &self,
        request: ProviderRequest,
        invocation: &ProviderInvocation,
    ) -> Result<LlmResponse, ProviderError> {
        self.chat_resolved(request, Some(invocation))
    }

    fn chat_stream(
        &self,
        request: ProviderRequest,
        on_event: &mut dyn FnMut(ProviderEvent),
    ) -> Result<LlmResponse, ProviderError> {
        self.chat_stream_resolved(request, on_event, None)
    }

    fn chat_stream_with_invocation(
        &self,
        request: ProviderRequest,
        on_event: &mut dyn FnMut(ProviderEvent),
        invocation: &ProviderInvocation,
    ) -> Result<LlmResponse, ProviderError> {
        self.chat_stream_resolved(request, on_event, Some(invocation))
    }
}
