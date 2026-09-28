use crate::{error_response, json_response, ApiError, ApiHttpResponse};
use serde_json::{json, Value};
use shacs_projection::{
    Spec035TransportCapability, Spec035TransportClientHello, Spec035TransportGeneration,
    Spec035TransportMutationRejection, Spec035TransportNegotiationError, Spec035TransportOffer,
    Spec035TransportOfferInput, Spec035TransportSchemaVersion, Spec035TransportServerHello,
};

pub const TRANSPORT_HELLO_PATH: &str = "/v1/transport/hello";

pub fn transport_hello_response(body: Option<&Value>) -> ApiHttpResponse {
    let client = match parse_client_hello(body) {
        Ok(client) => client,
        Err(response) => return response,
    };
    match api_offer() {
        Err(error) => error_response(error),
        Ok(offer) => match offer.select(&client) {
            Ok(server) => json_response(200, json!(server)),
            Err(_) => schema_mismatch_response(),
        },
    }
}

pub(crate) fn negotiate_api_mutation(
    hello: &Value,
    capability: Spec035TransportCapability,
) -> Result<Spec035TransportServerHello, ApiHttpResponse> {
    let client = parse_client_hello(Some(hello))?;
    match api_offer()
        .map_err(error_response)?
        .negotiate_mutation(&client, capability)
    {
        Ok(server) => Ok(server),
        Err(Spec035TransportNegotiationError::Unsupported(reason)) => Err(json_response(
            422,
            json!(match reason {
                shacs_projection::Spec035TransportUnsupportedReason::CapabilityUnavailable =>
                    Spec035TransportMutationRejection::capability_unavailable(),
            }),
        )),
        Err(Spec035TransportNegotiationError::SchemaMismatch(_)) => Err(schema_mismatch_response()),
    }
}

fn schema_mismatch_response() -> ApiHttpResponse {
    json_response(
        400,
        json!(Spec035TransportMutationRejection::schema_mismatch()),
    )
}

fn parse_client_hello(
    body: Option<&Value>,
) -> Result<Spec035TransportClientHello, ApiHttpResponse> {
    let Some(body) = body else {
        return Err(error_response(ApiError::invalid_request(
            "transport hello body is required",
        )));
    };
    Spec035TransportClientHello::parse_json(&body.to_string())
        .map_err(|error| error_response(ApiError::invalid_request(error.to_string())))
}

fn api_offer() -> Result<Spec035TransportOffer, ApiError> {
    let schema = Spec035TransportSchemaVersion::try_new(1)
        .map_err(|error| ApiError::internal(error.to_string()))?;
    let generation = Spec035TransportGeneration::try_new("generation:api-local")
        .map_err(|error| ApiError::internal(error.to_string()))?;
    Spec035TransportOffer::try_new(Spec035TransportOfferInput {
        schema_versions: vec![schema],
        mutation_capabilities: vec![
            Spec035TransportCapability::TaskPause,
            Spec035TransportCapability::TaskResume,
            Spec035TransportCapability::TaskStop,
            Spec035TransportCapability::TaskRecover,
        ],
        generation,
    })
    .map_err(|error| ApiError::internal(error.to_string()))
}
