use crate::support::{projection_for, MediaAdapter};
use serde_json::Value;
use shacs_api::{handle_api_request, ApiHttpRequest, MEDIA_DIAGNOSTICS_PATH};
use std::error::Error;

#[test]
fn http_media_projection_preserves_every_canonical_state_and_field() -> Result<(), Box<dyn Error>> {
    for state in [
        "included",
        "unsupported",
        "extraction_failed",
        "analyzer_missing",
        "truncated",
        "unavailable",
    ] {
        let projection = projection_for(state)?;
        let expected = serde_json::to_value(&projection)?;
        let adapter = MediaAdapter {
            projection: Some(projection),
        };

        let response = handle_api_request(ApiHttpRequest::get(MEDIA_DIAGNOSTICS_PATH), &adapter);

        assert_eq!(response.status, 200, "{state}");
        assert_eq!(response.body, expected, "{state}");
    }
    Ok(())
}

#[test]
fn http_media_projection_does_not_invent_success_when_owner_facts_are_absent() {
    let adapter = MediaAdapter { projection: None };

    let response = handle_api_request(ApiHttpRequest::get(MEDIA_DIAGNOSTICS_PATH), &adapter);

    assert_eq!(response.status, 404);
    assert_eq!(
        response.body["error"]["type"],
        Value::String("not_found".to_owned())
    );
    assert!(!response.body.to_string().contains("included"));
}
