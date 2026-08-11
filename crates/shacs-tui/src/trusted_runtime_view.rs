use shacs_projection::{
    render_spec030_runtime, serialize_spec030_runtime, Spec030RuntimeProjection,
};

pub fn trusted_runtime_human(projection: &Spec030RuntimeProjection) -> String {
    render_spec030_runtime(projection)
}

pub fn trusted_runtime_lines(projection: &Spec030RuntimeProjection) -> Vec<String> {
    trusted_runtime_human(projection)
        .lines()
        .map(str::to_owned)
        .collect()
}

pub fn trusted_runtime_json(
    projection: &Spec030RuntimeProjection,
) -> Result<String, serde_json::Error> {
    serialize_spec030_runtime(projection)
}
