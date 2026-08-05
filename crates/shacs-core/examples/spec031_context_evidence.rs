use shacs_core::runtime::{
    build_context_provider_handoff, discover_context_files, parse_context_references,
    project_spec031_context_evidence, resolve_context_reference, ContextBudgetInput,
    ContextFileDiscoveryOptions, ContextReferenceResolverConfig, Spec031ContextEvidenceInput,
    Spec031ContextOwnerRef,
};
use shacs_projection::Spec031Freshness;
use std::error::Error;

fn main() -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    std::fs::write(workspace.path().join("AGENTS.md"), "safe context")?;
    let outside = tempfile::NamedTempFile::new()?;

    let parsed = parse_context_references(&format!(
        "read @AGENTS.md @thing:value @{} @git:missing-rev",
        outside.path().display()
    ));
    let resolver = ContextReferenceResolverConfig::new(workspace.path());
    let artifacts = parsed
        .references
        .iter()
        .map(|reference| resolve_context_reference(reference, &resolver))
        .collect::<Vec<_>>();
    let discovery = discover_context_files(
        workspace.path(),
        ContextFileDiscoveryOptions {
            extra_context_files: vec!["absent.md".into()],
            ..ContextFileDiscoveryOptions::default()
        },
    );
    let handoff = build_context_provider_handoff(
        &artifacts,
        &discovery.entries,
        ContextBudgetInput {
            max_context_bytes: Some(0),
            ..ContextBudgetInput::default()
        },
    );
    let projection = project_spec031_context_evidence(Spec031ContextEvidenceInput {
        batch_ref: Spec031ContextOwnerRef::try_new("subject:context:qa").ok(),
        owner_freshness: Spec031Freshness::Current,
        inline_artifacts: &artifacts,
        context_files: &discovery.entries,
        provider_handoff: Some(&handoff),
    })?;

    println!("{}", serde_json::to_string_pretty(&projection)?);
    Ok(())
}
