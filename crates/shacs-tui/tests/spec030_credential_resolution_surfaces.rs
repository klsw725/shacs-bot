use serde_json::Value;
use shacs_api::{
    handle_api_request, ApiError, ApiHttpRequest, ChatCompletionAdapter, ChatCompletionInvocation,
    TRUSTED_RUNTIME_PATH,
};
use shacs_cli::spec030_cli::{render_trusted_runtime, Spec030CliFormat};
use shacs_config::{
    CredentialFamily, CredentialSourceDeclaration, ProviderConfig, ProvidersConfig,
};
use shacs_core::runtime::trusted_runtime::{
    LocalSpec030ProjectionProvider, Spec030FactStore, WorkspaceTrustObservation,
};
use shacs_core::runtime::{ProviderClientResolutionRequest, ProviderCredentialRuntime};
use shacs_projection::{Spec030ProjectionProvider, Spec030RuntimeProjection};
use shacs_providers::{LlmResponse, ProviderRegistry};
use shacs_tui::trusted_runtime_view::trusted_runtime_json;
use std::error::Error;

#[derive(Clone)]
struct SharedProvider(LocalSpec030ProjectionProvider);

impl Spec030ProjectionProvider for SharedProvider {
    fn projection(&self) -> Spec030RuntimeProjection {
        self.0.projection()
    }
}

impl ChatCompletionAdapter for SharedProvider {
    fn configured_model(&self) -> &str {
        "credential-surface"
    }

    fn complete_chat(
        &self,
        _invocation: ChatCompletionInvocation,
    ) -> Result<LlmResponse, ApiError> {
        Ok(LlmResponse::default())
    }

    fn trusted_runtime_projection(&self) -> Spec030RuntimeProjection {
        self.projection()
    }
}

#[test]
fn spec030_failed_production_resolution_matches_cli_api_and_tui_without_raw(
) -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let facts = Spec030FactStore::new(WorkspaceTrustObservation::Trusted);
    let provider = SharedProvider(LocalSpec030ProjectionProvider::new(facts.clone()));
    let runtime = ProviderCredentialRuntime::new(root.path().join("auth.json"), root.path(), facts)
        .with_declaration(
            "openai",
            CredentialSourceDeclaration {
                family: CredentialFamily::ApiKey,
                environment: None,
                local_auth: false,
                command: Some("printf RAW_COMMAND_CANARY; exit 9".to_owned()),
            },
        );
    let providers = ProvidersConfig::from([(
        "openai".to_owned(),
        ProviderConfig {
            api_key: Some("RAW_LITERAL_CANARY".to_owned()),
            ..ProviderConfig::default()
        },
    )]);

    let registry = ProviderRegistry::new();
    assert!(runtime
        .resolve_provider_client(ProviderClientResolutionRequest {
            registry: &registry,
            requested_provider: "openai",
            model: "gpt-4o",
            providers: &providers,
        })
        .is_err());
    let cli = render_trusted_runtime(&provider, Spec030CliFormat::Json)?;
    let api = handle_api_request(
        ApiHttpRequest::get(format!("{TRUSTED_RUNTIME_PATH}?schema_version=1")),
        &provider,
    );
    let tui = trusted_runtime_json(&provider.projection())?;
    let cli_value: Value = serde_json::from_str(&cli)?;
    let tui_value: Value = serde_json::from_str(&tui)?;

    assert_eq!(cli_value, api.body);
    assert_eq!(api.body, tui_value);
    assert_eq!(cli_value["credential"]["status"], "missing");
    for canary in ["RAW_COMMAND_CANARY", "RAW_LITERAL_CANARY"] {
        assert!(!cli.contains(canary));
        assert!(!tui.contains(canary));
        assert!(!api.body.to_string().contains(canary));
    }
    Ok(())
}
