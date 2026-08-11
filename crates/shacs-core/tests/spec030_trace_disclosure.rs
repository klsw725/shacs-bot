use shacs_core::runtime::trusted_runtime::LocalSpec030ProjectionProvider;
use shacs_projection::{DataSurface, Spec030ProjectionProvider, TraceStatus};
use std::{error::Error, fs};

#[test]
fn disabled_trace_keeps_extension_data_disclosure() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let workspace = root.path().join("workspace");
    let config = root.path().join("config.json");
    fs::create_dir_all(&workspace)?;
    fs::write(
        &config,
        serde_json::json!({"trustedRuntime":{"trace":{"enabled":false}}}).to_string(),
    )?;

    let projection =
        LocalSpec030ProjectionProvider::load(Some(config), Some(workspace)).projection();

    assert_eq!(projection.disclosure().trace.status, TraceStatus::Disabled);
    assert!(projection
        .disclosure()
        .surfaces
        .contains(&DataSurface::ExtensionData));
    Ok(())
}
