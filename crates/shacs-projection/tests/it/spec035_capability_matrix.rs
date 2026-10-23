use shacs_projection::{
    Spec035TransportCapability, Spec035TransportCapabilitySupport, Spec035TransportClientHello,
    Spec035TransportClientHelloInput, Spec035TransportClientId, Spec035TransportGeneration,
    Spec035TransportOffer, Spec035TransportOfferInput, Spec035TransportSchemaVersion,
    Spec035TransportUnsupportedReason,
};
use std::error::Error;

const CAPABILITIES: [Spec035TransportCapability; 5] = [
    Spec035TransportCapability::TaskPause,
    Spec035TransportCapability::TaskResume,
    Spec035TransportCapability::TaskStop,
    Spec035TransportCapability::TaskRetry,
    Spec035TransportCapability::TaskRecover,
];

#[test]
fn capability_intersection_is_deterministic_for_the_cartesian_matrix() -> Result<(), Box<dyn Error>>
{
    for capability in CAPABILITIES {
        for client_supports in [false, true] {
            for server_supports in [false, true] {
                let client = client_hello(capability, client_supports)?;
                let offer = server_offer(capability, server_supports, 1)?;
                let first = offer.select(&client)?;
                let second = offer.select(&client)?;

                assert_eq!(first, second);
                assert_eq!(
                    first.capability_support(capability),
                    if client_supports && server_supports {
                        Spec035TransportCapabilitySupport::Supported
                    } else {
                        Spec035TransportCapabilitySupport::Unsupported {
                            reason: Spec035TransportUnsupportedReason::CapabilityUnavailable,
                        }
                    },
                    "capability={capability:?} client={client_supports} server={server_supports}"
                );
                let mutation = offer.negotiate_mutation(&client, capability);
                if client_supports && server_supports {
                    assert!(mutation.is_ok());
                } else {
                    assert_eq!(
                        mutation
                            .expect_err("missing intersection must be unsupported")
                            .unsupported_reason(),
                        Some(Spec035TransportUnsupportedReason::CapabilityUnavailable)
                    );
                }
            }
        }
    }
    Ok(())
}

#[test]
fn schema_failure_remains_distinct_from_unsupported_capability() -> Result<(), Box<dyn Error>> {
    let capability = Spec035TransportCapability::TaskStop;
    let client = client_hello(capability, true)?;
    let offer = server_offer(capability, false, 2)?;

    let error = offer
        .negotiate_mutation(&client, capability)
        .expect_err("schema mismatch must fail before capability selection");

    assert!(error.is_schema_mismatch());
    assert_eq!(error.unsupported_reason(), None);
    Ok(())
}

fn client_hello(
    capability: Spec035TransportCapability,
    supports: bool,
) -> Result<Spec035TransportClientHello, Box<dyn Error>> {
    Ok(Spec035TransportClientHello::try_new(
        Spec035TransportClientHelloInput {
            client_id: Spec035TransportClientId::try_new("client:matrix")?,
            schema_versions: vec![Spec035TransportSchemaVersion::try_new(1)?],
            mutation_capabilities: supports.then_some(capability).into_iter().collect(),
            resume: None,
        },
    )?)
}

fn server_offer(
    capability: Spec035TransportCapability,
    supports: bool,
    schema: u32,
) -> Result<Spec035TransportOffer, Box<dyn Error>> {
    Ok(Spec035TransportOffer::try_new(
        Spec035TransportOfferInput {
            schema_versions: vec![Spec035TransportSchemaVersion::try_new(schema)?],
            mutation_capabilities: supports.then_some(capability).into_iter().collect(),
            generation: Spec035TransportGeneration::try_new("generation:matrix")?,
        },
    )?)
}
