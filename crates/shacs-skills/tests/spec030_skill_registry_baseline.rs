use shacs_skills::{
    discover_skill_registry, SkillRegistryOptions, SkillRegistryStatus, SkillSourceKind,
};
use std::error::Error;
use std::fs;
use std::path::Path;

#[test]
fn workspace_skill_registry_is_read_only_projection() -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    write_skill(
        workspace.path(),
        "spec030-weather",
        "Spec030 weather baseline",
    )?;

    let registry = discover_skill_registry(SkillRegistryOptions::new(workspace.path()))?;
    let entry = registry
        .find("spec030-weather")
        .ok_or("missing spec030-weather")?;

    assert_eq!(entry.status, SkillRegistryStatus::Active);
    assert_eq!(
        entry.descriptor.source_kind,
        SkillSourceKind::WorkspaceLocal
    );
    assert_eq!(
        entry.descriptor.description.as_deref(),
        Some("Spec030 weather baseline")
    );
    assert!(entry
        .raw
        .as_deref()
        .is_some_and(|raw| raw.contains("read-only")));
    assert!(!entry.descriptor.body_hash.is_empty());
    assert!(entry.descriptor.requirements.is_empty());
    assert!(entry.descriptor.install_metadata.is_none());
    Ok(())
}

#[test]
fn workspace_skill_shadows_virtual_builtin_without_granting_execution_metadata(
) -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    write_skill(workspace.path(), "weather", "Spec030 weather override")?;

    let registry = discover_skill_registry(SkillRegistryOptions::new(workspace.path()))?;
    let weather_entries = registry
        .entries
        .iter()
        .filter(|entry| entry.descriptor.name == "weather")
        .collect::<Vec<_>>();

    assert_eq!(weather_entries.len(), 2);
    assert!(weather_entries.iter().any(|entry| {
        entry.status == SkillRegistryStatus::Active
            && entry.descriptor.source_kind == SkillSourceKind::WorkspaceLocal
            && entry.descriptor.install_metadata.is_none()
    }));
    assert!(weather_entries.iter().any(|entry| {
        entry.status == SkillRegistryStatus::Shadowed
            && entry.descriptor.source_kind == SkillSourceKind::VirtualBuiltin
    }));
    Ok(())
}

fn write_skill(workspace: &Path, name: &str, description: &str) -> Result<(), Box<dyn Error>> {
    let skill_dir = workspace.join("skills").join(name);
    fs::create_dir_all(&skill_dir)?;
    fs::write(
        skill_dir.join("SKILL.md"),
        format!("---\nname: {name}\ndescription: {description}\n---\nread-only registry baseline"),
    )?;
    Ok(())
}
