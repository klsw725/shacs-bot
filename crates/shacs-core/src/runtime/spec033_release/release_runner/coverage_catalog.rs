use super::*;

mod blocker_catalog;
pub(super) use blocker_catalog::required as required_blockers;
pub(super) use blocker_catalog::{blocker_coverage, validate_blocker_coverage};

struct CoverageSpec {
    requirement: &'static str,
    code_path: &'static str,
    check: Spec033ReleaseCheck,
}

const COVERAGE: [CoverageSpec; 37] = [
    spec("033-MH001", "crates/shacs-core/src/runtime/goal_surface.rs", Spec033ReleaseCheck::GoalAccounting),
    spec("033-MH002", "crates/shacs-core/src/runtime/agent_loop.rs", Spec033ReleaseCheck::SnapshotReplay),
    spec("033-MH003", "crates/shacs-core/src/runtime/automation_lifecycle.rs", Spec033ReleaseCheck::AutomationDispatch),
    spec("033-MH004", "crates/shacs-core/src/runtime/automation_adapter.rs", Spec033ReleaseCheck::AutomationDispatch),
    spec("033-MH005", "crates/shacs-core/src/runtime/self_improvement_live/mod.rs", Spec033ReleaseCheck::SelfImprovement),
    spec("033-MH006", "crates/shacs-core/src/runtime/spec033_projection.rs", Spec033ReleaseCheck::SnapshotReplay),
    spec("033-MH007", "crates/shacs-core/src/runtime/snapshot_replay.rs", Spec033ReleaseCheck::SnapshotReplay),
    spec("033-MH008", "crates/shacs-core/src/runtime/spec033_projection.rs", Spec033ReleaseCheck::SnapshotReplay),
    spec("033-MH009", "crates/shacs-core/src/runtime/spec033_release/release_runner/coverage_catalog.rs", Spec033ReleaseCheck::ReviewArtifacts),
    spec("033-MH010", "docs/specs/033-evaluation-automation-live-integration/evidence/index.json", Spec033ReleaseCheck::ReviewArtifacts),
    spec("033-MH011", "crates/shacs-core/src/runtime/spec033_release/release_runner/coverage_catalog/blocker_catalog.rs", Spec033ReleaseCheck::SelfImprovement),
    spec("033-AC001", "crates/shacs-core/src/runtime/goal_surface.rs", Spec033ReleaseCheck::GoalAccounting),
    spec("033-AC002", "crates/shacs-core/src/runtime/agent_loop.rs", Spec033ReleaseCheck::SnapshotReplay),
    spec("033-AC003", "crates/shacs-core/src/runtime/automation_production.rs", Spec033ReleaseCheck::AutomationDispatch),
    spec("033-AC004", "crates/shacs-core/src/runtime/automation_adapter.rs", Spec033ReleaseCheck::AutomationDispatch),
    spec("033-AC005", "crates/shacs-core/src/runtime/self_improvement_live/mod.rs", Spec033ReleaseCheck::SelfImprovement),
    spec("033-AC006", "crates/shacs-core/src/runtime/automation_gates.rs", Spec033ReleaseCheck::SelfImprovement),
    spec("033-AC007", "crates/shacs-core/src/runtime/snapshot_replay.rs", Spec033ReleaseCheck::SnapshotReplay),
    spec("033-AC008", "crates/shacs-core/src/runtime/spec033_projection.rs", Spec033ReleaseCheck::SnapshotReplay),
    spec("033-AC009", "crates/shacs-core/src/runtime/spec033_projection.rs", Spec033ReleaseCheck::GoalAccounting),
    spec("033-AC010", "crates/shacs-core/src/runtime/spec033_release/release_runner/coverage_catalog.rs", Spec033ReleaseCheck::ReviewArtifacts),
    spec("033-AC011", "docs/specs/033-evaluation-automation-live-integration/evidence/index.json", Spec033ReleaseCheck::ReviewArtifacts),
    spec("018-PRD000", "crates/shacs-eval/src/evaluator.rs", Spec033ReleaseCheck::SnapshotReplay),
    spec("018-PRD001", "crates/shacs-core/src/runtime/goal_accounting.rs", Spec033ReleaseCheck::GoalAccounting),
    spec("018-PRD002", "crates/shacs-core/src/runtime/automation_gates.rs", Spec033ReleaseCheck::SelfImprovement),
    spec("018-PRD003", "crates/shacs-core/src/runtime/automation_lifecycle.rs", Spec033ReleaseCheck::AutomationDispatch),
    spec("018-PRD004", "crates/shacs-eval/src/evaluator.rs", Spec033ReleaseCheck::SnapshotReplay),
    spec("018-PRD005", "crates/shacs-core/src/runtime/self_improvement_live/mod.rs", Spec033ReleaseCheck::SelfImprovement),
    spec("018-PRD006", "crates/shacs-core/src/runtime/snapshot_replay.rs", Spec033ReleaseCheck::SnapshotReplay),
    spec("018-PRD007", "crates/shacs-core/src/runtime/spec033_projection.rs", Spec033ReleaseCheck::ReviewArtifacts),
    spec("018-PRD008", "crates/shacs-core/src/runtime/goal_evaluator.rs", Spec033ReleaseCheck::SnapshotReplay),
    spec("018-PRD009", "crates/shacs-core/src/runtime/automation_production.rs", Spec033ReleaseCheck::AutomationDispatch),
    spec("018-PRD010", "crates/shacs-eval/src/evaluator.rs", Spec033ReleaseCheck::SnapshotReplay),
    spec("018-PRD011", "crates/shacs-core/src/runtime/self_improvement_live/mod.rs", Spec033ReleaseCheck::SelfImprovement),
    spec("018-PRD012", "crates/shacs-core/src/runtime/snapshot_replay.rs", Spec033ReleaseCheck::SnapshotReplay),
    spec("018-PRD013", "crates/shacs-core/src/runtime/spec033_projection.rs", Spec033ReleaseCheck::GoalAccounting),
    spec("018-PRD014", "crates/shacs-projection/src/diagnostics_release.rs", Spec033ReleaseCheck::ReviewArtifacts),
];

const fn spec(
    requirement: &'static str,
    code_path: &'static str,
    check: Spec033ReleaseCheck,
) -> CoverageSpec {
    CoverageSpec {
        requirement,
        code_path,
        check,
    }
}

pub(super) fn coverage(
    root: &Path,
    commands: &[Spec033ReleaseCommandEvidence],
) -> Result<Vec<Spec033CoverageRow>, Spec033ReleaseArtifactError> {
    COVERAGE
        .iter()
        .map(|entry| row(root, commands, entry))
        .collect()
}

fn row(
    root: &Path,
    commands: &[Spec033ReleaseCommandEvidence],
    entry: &CoverageSpec,
) -> Result<Spec033CoverageRow, Spec033ReleaseArtifactError> {
    let command = commands
        .iter()
        .find(|command| command.kind == entry.check)
        .ok_or(Spec033ReleaseArtifactError::MissingGuarantee)?;
    Ok(Spec033CoverageRow {
        requirement: entry.requirement.to_owned(),
        code_path: entry.code_path.to_owned(),
        test_command: command.command.argv.join(" "),
        artifact: command.redacted_stdout.clone(),
        artifact_digest: super::release_artifacts::digest_file(
            &root.join(&command.redacted_stdout),
        )?,
        evidence_source: command.redacted_stdout.clone(),
        status: "passed".to_owned(),
        non_guarantee: format!(
            "{} is bounded to the mapped source and recorded Cargo gate",
            entry.requirement
        ),
    })
}

pub(super) fn validate_coverage(
    rows: &[Spec033CoverageRow],
) -> Result<(), Spec033ReleaseArtifactError> {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    if rows.len() != COVERAGE.len() {
        return Err(Spec033ReleaseArtifactError::MissingGuarantee);
    }
    for entry in &COVERAGE {
        let matching = rows
            .iter()
            .filter(|row| row.requirement == entry.requirement)
            .collect::<Vec<_>>();
        let [row] = matching.as_slice() else {
            return Err(Spec033ReleaseArtifactError::MissingGuarantee);
        };
        let expected = std::iter::once("cargo".to_owned())
            .chain(entry.check.cargo_args())
            .collect::<Vec<_>>()
            .join(" ");
        if row.code_path != entry.code_path
            || !repo.join(&row.code_path).is_file()
            || row.test_command != expected
            || row.evidence_source != row.artifact
            || row.status != "passed"
            || !valid_digest(&row.artifact_digest)
            || row.artifact.is_empty()
            || row.non_guarantee.is_empty()
        {
            return Err(Spec033ReleaseArtifactError::MissingGuarantee);
        }
    }
    Ok(())
}

fn valid_digest(value: &str) -> bool {
    value.len() == 71
        && value.starts_with("sha256:")
        && value[7..].bytes().all(|byte| byte.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::COVERAGE;
    use std::collections::BTreeSet;

    #[test]
    fn catalog_contains_each_current_required_row_once() {
        // Given
        let expected = (1..=11)
            .map(|item| format!("033-MH{item:03}"))
            .chain((1..=11).map(|item| format!("033-AC{item:03}")))
            .chain((0..=14).map(|item| format!("018-PRD{item:03}")))
            .collect::<BTreeSet<_>>();

        // When
        let actual = COVERAGE
            .iter()
            .map(|entry| entry.requirement.to_owned())
            .collect::<BTreeSet<_>>();

        // Then
        assert_eq!(actual, expected);
        assert_eq!(COVERAGE.len(), expected.len());
    }
}
