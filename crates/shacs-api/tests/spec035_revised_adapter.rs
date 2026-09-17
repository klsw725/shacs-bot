use shacs_api::{
    handle_api_request, spec035_revised_projection_json_response, ApiError, ApiHttpRequest,
    ChatCompletionAdapter, ChatCompletionInvocation, DIAGNOSTICS_PATH,
};
use shacs_projection::{
    project_spec035_revised_owner_facts, Spec030RuntimeProjection, Spec030UnavailableReason,
    Spec035OwnerSurface, Spec035RevisedOwnerFacts,
};
use shacs_providers::LlmResponse;
use std::error::Error;

const SENSITIVE_VALUES: [(&str, &str); 6] = [
    ("secret", "sk-spec035-adapter-secret"),
    (
        "credential_url",
        "https://user:password@example.invalid/path",
    ),
    ("absolute_path", "/tmp/spec035-private"),
    ("process_handle", "spec035-process-handle-4242"),
    ("stdout", "spec035-raw-stdout"),
    ("stderr", "spec035-raw-stderr"),
];

struct DiagnosticsAdapter;

impl ChatCompletionAdapter for DiagnosticsAdapter {
    fn configured_model(&self) -> &str {
        "spec035-diagnostics"
    }

    fn complete_chat(
        &self,
        _invocation: ChatCompletionInvocation,
    ) -> Result<LlmResponse, ApiError> {
        Ok(LlmResponse::default())
    }
}

#[test]
fn diagnostics_route_contains_revised_projection_from_actual_owner_source() {
    // Given: an API adapter whose Spec030 owner facts are unavailable.
    let adapter = DiagnosticsAdapter;

    // When: the existing diagnostics route is requested.
    let response = handle_api_request(ApiHttpRequest::get(DIAGNOSTICS_PATH), &adapter);

    // Then: the existing response includes the shared conservative projection.
    assert_eq!(response.status, 200);
    assert_eq!(
        response.body["runtime"]["spec035_revised"]["schema_version"],
        1
    );
    assert_eq!(
        response.body["runtime"]["spec035_revised"]["delivery"]["final_delivery"]["state"],
        "unknown"
    );
}

#[test]
fn api_adapter_preserves_canonical_projection_and_rejects_sensitive_input(
) -> Result<(), Box<dyn Error>> {
    // Given: owner-backed output and independent forged sensitive fields.
    let trusted_runtime =
        Spec030RuntimeProjection::unavailable(Spec030UnavailableReason::OwnerFactsMissing);
    let canonical = serde_json::to_string(&project_spec035_revised_owner_facts(
        Spec035RevisedOwnerFacts::new(Spec035OwnerSurface::Api, &trusted_runtime),
    )?)?;

    // When: the owner output crosses the API response adapter boundary.
    let response = spec035_revised_projection_json_response(&canonical);

    // Then: canonical fields are unchanged and every sensitive field is rejected independently.
    assert_eq!(response.status, 200);
    assert_eq!(
        response.body,
        serde_json::from_str::<serde_json::Value>(&canonical)?
    );
    for (field, sentinel) in SENSITIVE_VALUES {
        let mut sensitive: serde_json::Value = serde_json::from_str(&canonical)?;
        sensitive[field] = sentinel.into();
        let rejected =
            spec035_revised_projection_json_response(&serde_json::to_string(&sensitive)?);
        assert_eq!(rejected.status, 400);
        assert!(!serde_json::to_string(&rejected.body)?.contains(sentinel));
    }
    Ok(())
}
