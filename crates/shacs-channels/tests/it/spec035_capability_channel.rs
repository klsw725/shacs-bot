use shacs_channels::{
    negotiate_spec035_channel_hello, spec035_channel_worker_hello, ChannelManager,
};
use shacs_projection::{
    Spec035TransportCapability, Spec035TransportClientHello, Spec035TransportClientHelloInput,
    Spec035TransportClientId, Spec035TransportMutationRejection, Spec035TransportSchemaVersion,
};
use std::cell::Cell;
use std::error::Error;

#[test]
fn supported_channel_mutation_reaches_handler_once() -> Result<(), Box<dyn Error>> {
    let calls = Cell::new(0);
    let mut manager = ChannelManager::new();
    let result = manager.dispatch_spec035_mutation(
        &spec035_channel_worker_hello()?,
        Spec035TransportCapability::TaskStop,
        |_| calls.set(calls.get() + 1),
    );

    assert!(result.is_ok());
    assert_eq!(calls.get(), 1);
    Ok(())
}

#[test]
fn unsupported_channel_mutation_has_no_handler_adapter_or_owner_side_effects(
) -> Result<(), Box<dyn Error>> {
    let handler = Cell::new(0);
    let adapter = Cell::new(0);
    let owner = Cell::new(0);
    let mut manager = ChannelManager::new();
    let result = manager.dispatch_spec035_mutation(
        &hello(&[])?,
        Spec035TransportCapability::TaskStop,
        |_| {
            handler.set(handler.get() + 1);
            adapter.set(adapter.get() + 1);
            owner.set(owner.get() + 1);
        },
    );

    let error = result.expect_err("missing client capability must be unsupported");
    assert_eq!(error.status(), "unsupported");
    assert_eq!(error.reason(), "capability_unavailable");
    assert_eq!(
        error.rejection(),
        Spec035TransportMutationRejection::capability_unavailable()
    );
    assert_eq!((handler.get(), adapter.get(), owner.get()), (0, 0, 0));
    Ok(())
}

#[test]
fn channel_schema_mismatch_is_blocked_separately() -> Result<(), Box<dyn Error>> {
    let client = Spec035TransportClientHello::try_new(Spec035TransportClientHelloInput {
        client_id: Spec035TransportClientId::try_new("client:channel-schema")?,
        schema_versions: vec![Spec035TransportSchemaVersion::try_new(2)?],
        mutation_capabilities: vec![Spec035TransportCapability::TaskStop],
        resume: None,
    })?;

    let error = negotiate_spec035_channel_hello(&client).expect_err("schema mismatch must block");

    assert_eq!(error.status(), "blocked");
    assert_eq!(error.reason(), "schema_mismatch");
    assert_eq!(
        error.rejection(),
        Spec035TransportMutationRejection::schema_mismatch()
    );
    Ok(())
}

fn hello(
    capabilities: &[Spec035TransportCapability],
) -> Result<Spec035TransportClientHello, Box<dyn Error>> {
    Ok(Spec035TransportClientHello::try_new(
        Spec035TransportClientHelloInput {
            client_id: Spec035TransportClientId::try_new("client:channel")?,
            schema_versions: vec![Spec035TransportSchemaVersion::try_new(1)?],
            mutation_capabilities: capabilities.to_vec(),
            resume: None,
        },
    )?)
}
