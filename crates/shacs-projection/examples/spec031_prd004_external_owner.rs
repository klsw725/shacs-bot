use shacs_projection::{
    build_spec031_external_owner_projection, spec031_prd004_external_owner_artifacts,
};
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let output_dir = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(default_output_dir);
    let projection = build_spec031_external_owner_projection([], []);
    let artifacts = spec031_prd004_external_owner_artifacts(&projection, &output_dir)?;
    println!("projection_items={}", projection.items.len());
    println!("closure_blockers={}", projection.closure_blockers.len());
    for artifact in artifacts
        .read_audits
        .iter()
        .chain(&artifacts.closure_blockers)
    {
        println!("{} {}", artifact.status, artifact.file_name);
    }
    Ok(())
}

fn default_output_dir() -> PathBuf {
    PathBuf::from(".omo/evidence/spec031/prd004/external")
}
