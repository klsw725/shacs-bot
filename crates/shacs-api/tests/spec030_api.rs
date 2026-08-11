use serde_json::Value;
use shacs_api::{
    handle_api_request, ApiError, ApiHttpRequest, ChatCompletionAdapter, ChatCompletionInvocation,
    TRUSTED_RUNTIME_PATH,
};
use shacs_projection::{Spec030RuntimeProjection, Spec030UnavailableReason};
use shacs_providers::LlmResponse;
use std::error::Error;

struct FixtureAdapter;

impl ChatCompletionAdapter for FixtureAdapter {
    fn configured_model(&self) -> &str {
        "spec030-fixture"
    }

    fn complete_chat(
        &self,
        _invocation: ChatCompletionInvocation,
    ) -> Result<LlmResponse, ApiError> {
        Ok(LlmResponse::default())
    }

    fn trusted_runtime_projection(&self) -> Spec030RuntimeProjection {
        Spec030RuntimeProjection::unavailable(Spec030UnavailableReason::OwnerUnavailable)
    }
}

#[test]
fn trusted_runtime_route_returns_the_adapter_projection_when_schema_is_supported(
) -> Result<(), Box<dyn Error>> {
    let response = handle_api_request(
        ApiHttpRequest::get(format!("{TRUSTED_RUNTIME_PATH}?schema_version=1")),
        &FixtureAdapter,
    );

    assert_eq!(response.status, 200);
    assert_eq!(response.body["schemaVersion"], 1);
    assert_eq!(response.body["unavailableReason"], "ownerUnavailable");
    let serialized = serde_json::to_string(&response.body)?;
    for forbidden in ["apiKey", "accessToken", "refreshToken", "credentialValue"] {
        assert!(!serialized.contains(forbidden), "{forbidden}: {serialized}");
    }
    Ok(())
}

#[test]
fn trusted_runtime_route_rejects_an_unsupported_schema_selector() {
    let response = handle_api_request(
        ApiHttpRequest::get(format!("{TRUSTED_RUNTIME_PATH}?schema_version=2")),
        &FixtureAdapter,
    );

    assert_eq!(response.status, 400);
    assert_eq!(
        response.body["error"]["type"],
        Value::from("invalid_request_error")
    );
}
