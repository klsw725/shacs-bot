use shacs_projection::{
    Spec035RevisedParseError, Spec035RevisedProjection, Spec035TransportMutationRejection,
};

pub fn render_spec035_revised_projection(
    projection: &Spec035RevisedProjection,
) -> Result<String, serde_json::Error> {
    serde_json::to_string(projection)
}

pub fn render_spec035_transport_rejection(rejection: &Spec035TransportMutationRejection) -> String {
    rejection.to_string()
}

pub fn render_spec035_revised_json(input: &str) -> Result<String, Spec035RevisedParseError> {
    let projection = Spec035RevisedProjection::parse_json(input)?;
    serde_json::to_string(&projection).map_err(|_| Spec035RevisedParseError::InvalidSchema)
}
