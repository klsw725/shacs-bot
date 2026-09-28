use crate::{error_response, json_response, ApiError, ApiHttpResponse};
use shacs_projection::Spec035RevisedProjection;

pub fn spec035_revised_projection_response(
    projection: &Spec035RevisedProjection,
) -> ApiHttpResponse {
    json_response(200, serde_json::json!(projection))
}

pub fn spec035_revised_projection_json_response(input: &str) -> ApiHttpResponse {
    match Spec035RevisedProjection::parse_json(input) {
        Ok(projection) => spec035_revised_projection_response(&projection),
        Err(_) => error_response(ApiError::invalid_request(
            "invalid Spec035 revised projection",
        )),
    }
}
