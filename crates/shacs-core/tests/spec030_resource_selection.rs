use shacs_core::controlled_child::ControlledChildAbort;
use shacs_core::runtime::trusted_resources::{
    inspect_resources, ResourceCandidate, ResourceDiagnosticKind, ResourceEvidence,
    ResourceLoadCheck, WorkspaceResourceTrust,
};
use shacs_projection::{
    ResourceActivation, ResourceCollisionStatus, ResourceKind, ResourceLoadStatus,
    ResourcePrecedence, ResourceSource, TrustedCodeDisclosure,
};
use std::error::Error;

use sha2::{Digest, Sha256};

fn candidate(
    root: &std::path::Path,
    name: &str,
    precedence: ResourcePrecedence,
) -> Result<ResourceCandidate, Box<dyn Error>> {
    let path = root.join(name);
    std::fs::write(&path, name)?;
    Ok(ResourceCandidate {
        resource_ref: "skill:shared".to_owned(),
        kind: ResourceKind::Skill,
        source: source(precedence),
        precedence,
        path,
        activation: if precedence == ResourcePrecedence::Explicit {
            ResourceActivation::Explicit
        } else {
            ResourceActivation::TrustedWorkspace
        },
        trusted_code_disclosure: TrustedCodeDisclosure::Shown,
        load_check: ResourceLoadCheck::Content,
        diagnostics: Vec::new(),
    })
}

const fn source(precedence: ResourcePrecedence) -> ResourceSource {
    match precedence {
        ResourcePrecedence::Explicit => ResourceSource::Explicit,
        ResourcePrecedence::ProjectConfigured | ResourcePrecedence::TrustedProjectAuto => {
            ResourceSource::Project
        }
        ResourcePrecedence::UserConfigured | ResourcePrecedence::UserAuto => ResourceSource::User,
        ResourcePrecedence::Package => ResourceSource::Package,
        ResourcePrecedence::Builtin => ResourceSource::Builtin,
    }
}

#[test]
fn all_source_families_follow_the_seven_level_precedence() -> Result<(), Box<dyn Error>> {
    // Given
    let root = tempfile::tempdir()?;
    let levels = [
        ResourcePrecedence::Builtin,
        ResourcePrecedence::Package,
        ResourcePrecedence::UserAuto,
        ResourcePrecedence::UserConfigured,
        ResourcePrecedence::TrustedProjectAuto,
        ResourcePrecedence::ProjectConfigured,
        ResourcePrecedence::Explicit,
    ];
    let candidates = levels
        .into_iter()
        .enumerate()
        .map(|(index, level)| candidate(root.path(), &format!("{index}.md"), level))
        .collect::<Result<Vec<_>, _>>()?;

    // When
    let inspection = inspect_resources(
        candidates,
        WorkspaceResourceTrust::Trusted,
        &ControlledChildAbort::new(),
    );

    // Then
    let winner = inspection
        .resources
        .iter()
        .find(|resource| resource.projection.collision == ResourceCollisionStatus::Winner)
        .ok_or("winner missing")?;
    assert_eq!(winner.projection.precedence, ResourcePrecedence::Explicit);
    assert_eq!(winner.projection.load_status, ResourceLoadStatus::Loaded);
    assert_eq!(inspection.resources.len(), 7);
    assert_eq!(
        inspection
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.kind == ResourceDiagnosticKind::CollisionLoser)
            .count(),
        6
    );
    Ok(())
}

#[test]
fn equal_precedence_uses_canonical_path_byte_order() -> Result<(), Box<dyn Error>> {
    // Given
    let root = tempfile::tempdir()?;
    let candidates = vec![
        candidate(root.path(), "z.md", ResourcePrecedence::Package)?,
        candidate(root.path(), "a.md", ResourcePrecedence::Package)?,
    ];

    // When
    let inspection = inspect_resources(
        candidates,
        WorkspaceResourceTrust::Trusted,
        &ControlledChildAbort::new(),
    );

    // Then
    let winner = inspection
        .resources
        .iter()
        .find(|resource| resource.projection.collision == ResourceCollisionStatus::Winner)
        .ok_or("winner missing")?;
    assert!(winner.projection.canonical_path.ends_with("a.md"));
    assert!(inspection.diagnostics.iter().any(|diagnostic| {
        diagnostic.kind == ResourceDiagnosticKind::CollisionWinner
            && diagnostic.path.as_deref() == Some(winner.projection.canonical_path.as_str())
    }));
    let resources = inspection
        .resources
        .iter()
        .map(|fact| &fact.projection)
        .collect::<Vec<_>>();
    let expected_a = format!("{:x}", Sha256::digest(b"a.md"));
    let expected_z = format!("{:x}", Sha256::digest(b"z.md"));
    assert!(resources.iter().any(|resource| {
        resource.collision == ResourceCollisionStatus::Winner
            && resource.content_sha256.as_deref() == Some(expected_a.as_str())
    }));
    assert!(resources.iter().any(|resource| {
        resource.collision == ResourceCollisionStatus::Loser
            && resource.content_sha256.as_deref() == Some(expected_z.as_str())
    }));
    Ok(())
}

#[test]
fn malformed_path_is_diagnostic_and_never_loaded() {
    // Given
    let candidate = ResourceCandidate {
        resource_ref: "extension:missing".to_owned(),
        kind: ResourceKind::Extension,
        source: ResourceSource::Project,
        precedence: ResourcePrecedence::ProjectConfigured,
        path: std::path::PathBuf::from("/definitely/missing/shacs-resource"),
        activation: ResourceActivation::Explicit,
        trusted_code_disclosure: TrustedCodeDisclosure::Shown,
        load_check: ResourceLoadCheck::Content,
        diagnostics: Vec::new(),
    };

    // When
    let inspection = inspect_resources(
        vec![candidate],
        WorkspaceResourceTrust::Trusted,
        &ControlledChildAbort::new(),
    );

    // Then
    assert_eq!(
        inspection.resources[0].projection.load_status,
        ResourceLoadStatus::ParseFailed
    );
    assert!(inspection.diagnostics.iter().any(|diagnostic| {
        diagnostic.kind == ResourceDiagnosticKind::MalformedPath && !diagnostic.reason.is_empty()
    }));
}

#[test]
fn trust_removal_invalidates_auto_executable_and_inspect_is_not_proof() -> Result<(), Box<dyn Error>>
{
    // Given
    let root = tempfile::tempdir()?;
    let candidate = candidate(
        root.path(),
        "auto.md",
        ResourcePrecedence::TrustedProjectAuto,
    )?;

    // When
    let inspection = inspect_resources(
        vec![candidate],
        WorkspaceResourceTrust::Untrusted,
        &ControlledChildAbort::new(),
    );

    // Then
    let fact = &inspection.resources[0];
    assert_eq!(fact.projection.activation, ResourceActivation::Inactive);
    assert_eq!(fact.projection.load_status, ResourceLoadStatus::Rejected);
    assert_eq!(fact.authorization, ResourceEvidence::NotProvided);
    assert_eq!(fact.sandbox, ResourceEvidence::NotProvided);
    Ok(())
}
