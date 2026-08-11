use shacs_config::{
    CredentialFamily, CredentialSourceDeclaration, ProviderConfig, ProvidersConfig,
};
use shacs_core::controlled_child::ControlledChildAbort;
use shacs_core::runtime::trusted_runtime::{
    LocalSpec030ProjectionProvider, Spec030FactStore, WorkspaceTrustObservation,
};
use shacs_core::runtime::{
    CredentialResolvingProviderClient, ProviderClientResolutionRequest,
    ProviderCredentialInvocation, ProviderCredentialRuntime,
};
use shacs_projection::{
    CredentialStatus, ProcessAdapterKind, ProcessTerminalOutcome, Spec030ProjectionProvider,
};
use shacs_providers::{GenerationSettings, ProviderClient, ProviderRegistry, ProviderRequest};
use std::error::Error;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

#[test]
fn spec030_provider_command_nonzero_blocks_literal_transport() -> Result<(), Box<dyn Error>> {
    assert_command_blocks(
        "exit 7",
        Duration::from_secs(1),
        ProcessTerminalOutcome::Failed,
    )
}

#[test]
fn spec030_provider_command_empty_blocks_literal_transport() -> Result<(), Box<dyn Error>> {
    assert_command_blocks(
        "printf '   '",
        Duration::from_secs(1),
        ProcessTerminalOutcome::Succeeded,
    )
}

#[test]
fn spec030_provider_command_timeout_blocks_literal_transport() -> Result<(), Box<dyn Error>> {
    assert_command_blocks(
        "sleep 1; printf late",
        Duration::from_millis(20),
        ProcessTerminalOutcome::TimedOut,
    )
}

#[test]
fn spec030_provider_command_uses_shared_invocation_cancellation() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let facts = Spec030FactStore::new(WorkspaceTrustObservation::Trusted);
    let abort = ControlledChildAbort::new();
    let runtime = Arc::new(
        ProviderCredentialRuntime::new(root.path().join("auth.json"), root.path(), facts.clone())
            .with_declaration("openai", declaration("sleep 5; printf late")),
    );
    let providers = ProvidersConfig::from([(
        "openai".to_owned(),
        ProviderConfig {
            api_key: Some("literal-must-not-run".to_owned()),
            ..ProviderConfig::default()
        },
    )]);
    let client = CredentialResolvingProviderClient::new("openai", "gpt-4o", providers, runtime)
        .with_invocation(ProviderCredentialInvocation::new(None, abort.clone()));
    let worker = thread::spawn(move || {
        client.chat(ProviderRequest {
            messages: Vec::new(),
            tools: Vec::new(),
            model: "gpt-4o".to_owned(),
            settings: GenerationSettings::default(),
            tool_choice: None,
        })
    });

    thread::sleep(Duration::from_millis(30));
    abort.abort();
    let result = worker.join().map_err(|_| "command worker panicked")?;

    assert!(result.is_err());
    let projection = LocalSpec030ProjectionProvider::new(facts).projection();
    let command = projection
        .process_adapters()
        .iter()
        .find(|adapter| adapter.adapter == ProcessAdapterKind::CredentialCommand)
        .ok_or("credential command fact missing")?;
    assert_eq!(
        command.recent_outcomes[0].outcome,
        ProcessTerminalOutcome::Aborted
    );
    Ok(())
}

fn assert_command_blocks(
    command: &str,
    timeout: Duration,
    expected_outcome: ProcessTerminalOutcome,
) -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let facts = Spec030FactStore::new(WorkspaceTrustObservation::Trusted);
    let runtime =
        ProviderCredentialRuntime::new(root.path().join("auth.json"), root.path(), facts.clone())
            .with_declaration("openai", declaration(command))
            .with_command_timeout(timeout);
    let providers = ProvidersConfig::from([(
        "openai".to_owned(),
        ProviderConfig {
            api_key: Some("literal-must-not-run".to_owned()),
            ..ProviderConfig::default()
        },
    )]);

    let registry = ProviderRegistry::new();
    let result = runtime.resolve_provider_client(ProviderClientResolutionRequest {
        registry: &registry,
        requested_provider: "openai",
        model: "gpt-4o",
        providers: &providers,
    });

    assert!(result.is_err());
    let projection = LocalSpec030ProjectionProvider::new(facts).projection();
    assert_eq!(projection.credential().status, CredentialStatus::Missing);
    let command = projection
        .process_adapters()
        .iter()
        .find(|adapter| adapter.adapter == ProcessAdapterKind::CredentialCommand)
        .ok_or("credential command fact missing")?;
    assert!(!command.capabilities.abort);
    assert_eq!(command.recent_outcomes[0].outcome, expected_outcome);
    Ok(())
}

fn declaration(command: &str) -> CredentialSourceDeclaration {
    CredentialSourceDeclaration {
        family: CredentialFamily::ApiKey,
        environment: None,
        local_auth: false,
        command: Some(command.to_owned()),
    }
}
