mod load;
mod types;

pub use types::*;

use crate::controlled_child::ControlledChildAbort;
use shacs_projection::{ResourceActivation, ResourceCollisionStatus, ResourceLoadStatus};
use std::collections::BTreeMap;

pub fn inspect_resources(
    candidates: Vec<ResourceCandidate>,
    workspace_trust: WorkspaceResourceTrust,
    abort: &ControlledChildAbort,
) -> TrustedResourceInspection {
    let mut diagnostics = Vec::new();
    let mut invalid = Vec::new();
    let mut groups: BTreeMap<String, Vec<CanonicalCandidate>> = BTreeMap::new();
    for candidate in candidates {
        diagnostics.extend(candidate.diagnostics.iter().cloned());
        match CanonicalCandidate::new(candidate) {
            Ok(candidate) => groups
                .entry(candidate.candidate.resource_ref.clone())
                .or_default()
                .push(candidate),
            Err(error) => {
                let candidate = *error.candidate;
                diagnostics.push(ResourceDiagnostic {
                    resource_ref: candidate.resource_ref.clone(),
                    kind: ResourceDiagnosticKind::MalformedPath,
                    path: Some(candidate.path.to_string_lossy().into_owned()),
                    reason: error.reason,
                });
                invalid.push(ResourceFact::from_candidate(
                    candidate,
                    ResourceResolution {
                        collision: ResourceCollisionStatus::None,
                        activation: ResourceActivation::Inactive,
                        load_status: ResourceLoadStatus::ParseFailed,
                        receipt: None,
                    },
                ));
            }
        }
    }

    let mut resources = Vec::new();
    for (_, mut group) in groups {
        group.sort_by(|left, right| {
            precedence_rank(left.candidate.precedence)
                .cmp(&precedence_rank(right.candidate.precedence))
                .then_with(|| left.path_bytes.cmp(&right.path_bytes))
        });
        let collision = group.len() > 1;
        for (index, candidate) in group.into_iter().enumerate() {
            let winner = index == 0;
            let collision_status = match (collision, winner) {
                (false, _) => ResourceCollisionStatus::None,
                (true, true) => ResourceCollisionStatus::Winner,
                (true, false) => ResourceCollisionStatus::Loser,
            };
            if collision {
                diagnostics.push(ResourceDiagnostic {
                    resource_ref: candidate.candidate.resource_ref.clone(),
                    kind: if winner {
                        ResourceDiagnosticKind::CollisionWinner
                    } else {
                        ResourceDiagnosticKind::CollisionLoser
                    },
                    path: Some(candidate.canonical_path.clone()),
                    reason: "resource identity collision resolved by precedence and canonical path"
                        .to_owned(),
                });
            }
            if !winner {
                resources.push(ResourceFact::from_canonical(
                    candidate,
                    ResourceResolution {
                        collision: collision_status,
                        activation: ResourceActivation::Inactive,
                        load_status: ResourceLoadStatus::Rejected,
                        receipt: None,
                    },
                ));
                continue;
            }
            let activation = active_activation(&candidate.candidate, workspace_trust);
            if activation == ResourceActivation::Inactive {
                diagnostics.push(ResourceDiagnostic {
                    resource_ref: candidate.candidate.resource_ref.clone(),
                    kind: ResourceDiagnosticKind::WorkspaceTrustRequired,
                    path: Some(candidate.canonical_path.clone()),
                    reason: "trusted workspace assertion is required for executable resource"
                        .to_owned(),
                });
                resources.push(ResourceFact::from_canonical(
                    candidate,
                    ResourceResolution {
                        collision: collision_status,
                        activation,
                        load_status: ResourceLoadStatus::Rejected,
                        receipt: None,
                    },
                ));
                continue;
            }
            let outcome = load::run(&candidate.candidate.load_check, abort);
            if let Some(diagnostic) =
                outcome.diagnostic(&candidate.candidate.resource_ref, &candidate.canonical_path)
            {
                diagnostics.push(diagnostic);
            }
            resources.push(ResourceFact::from_canonical(
                candidate,
                ResourceResolution {
                    collision: collision_status,
                    activation,
                    load_status: outcome.status,
                    receipt: outcome.receipt,
                },
            ));
        }
    }
    resources.extend(invalid);
    for diagnostic in &diagnostics {
        for resource in resources
            .iter_mut()
            .filter(|resource| resource.projection.resource_ref == diagnostic.resource_ref)
        {
            let projected = diagnostic.projection();
            if !resource.projection.diagnostics.contains(&projected) {
                resource.projection.diagnostics.push(projected);
            }
        }
    }
    TrustedResourceInspection {
        resources,
        diagnostics,
    }
}

fn active_activation(
    candidate: &ResourceCandidate,
    workspace_trust: WorkspaceResourceTrust,
) -> ResourceActivation {
    match (candidate.activation, workspace_trust, candidate.kind) {
        (ResourceActivation::Explicit, _, _) => ResourceActivation::Explicit,
        (ResourceActivation::Inactive, _, _) => ResourceActivation::Inactive,
        (ResourceActivation::TrustedWorkspace, WorkspaceResourceTrust::Trusted, _) => {
            ResourceActivation::TrustedWorkspace
        }
        (
            ResourceActivation::TrustedWorkspace,
            WorkspaceResourceTrust::Untrusted,
            shacs_projection::ResourceKind::Skill
            | shacs_projection::ResourceKind::Extension
            | shacs_projection::ResourceKind::Package,
        ) => ResourceActivation::Inactive,
        (ResourceActivation::TrustedWorkspace, WorkspaceResourceTrust::Untrusted, _) => {
            ResourceActivation::TrustedWorkspace
        }
    }
}

const fn precedence_rank(precedence: shacs_projection::ResourcePrecedence) -> u8 {
    match precedence {
        shacs_projection::ResourcePrecedence::Explicit => 0,
        shacs_projection::ResourcePrecedence::ProjectConfigured => 1,
        shacs_projection::ResourcePrecedence::TrustedProjectAuto => 2,
        shacs_projection::ResourcePrecedence::UserConfigured => 3,
        shacs_projection::ResourcePrecedence::UserAuto => 4,
        shacs_projection::ResourcePrecedence::Package => 5,
        shacs_projection::ResourcePrecedence::Builtin => 6,
    }
}
