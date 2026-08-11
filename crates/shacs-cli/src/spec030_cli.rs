use shacs_api::{TrustedRuntimeObservation, TrustedRuntimeProjectionSource};
use shacs_projection::{
    render_spec030_runtime, serialize_spec030_runtime, Spec030ProjectionProvider,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Spec030CliFormat {
    #[default]
    Human,
    Json,
}

pub fn render_trusted_runtime(
    provider: &impl Spec030ProjectionProvider,
    format: Spec030CliFormat,
) -> Result<String, serde_json::Error> {
    let projection = provider.projection();
    match format {
        Spec030CliFormat::Human => Ok(render_spec030_runtime(&projection)),
        Spec030CliFormat::Json => serialize_spec030_runtime(&projection),
    }
}

pub fn render_trusted_runtime_observation(
    observation: &TrustedRuntimeObservation,
    format: Spec030CliFormat,
) -> Result<String, serde_json::Error> {
    match format {
        Spec030CliFormat::Human => {
            let source = match observation.source {
                TrustedRuntimeProjectionSource::ActiveRuntime => {
                    "Projection source: active loopback runtime"
                }
                TrustedRuntimeProjectionSource::UnavailablePreview => {
                    "Projection source: unavailable preview (active runtime unreachable)"
                }
            };
            Ok(format!(
                "{source}\n{}",
                render_spec030_runtime(&observation.projection)
            ))
        }
        Spec030CliFormat::Json => serialize_spec030_runtime(&observation.projection),
    }
}
