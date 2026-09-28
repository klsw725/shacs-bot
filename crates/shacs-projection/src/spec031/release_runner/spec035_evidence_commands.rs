use super::command::parse_cargo_test_counts_strict;
use super::model::Spec031ReleaseArtifactError;
use super::spec035_evidence_io::{evidence_error, read_json, read_text, require_inventory};
use super::spec035_evidence_model::CommandRegistry;
use std::collections::HashMap;
use std::path::Path;

pub(super) fn validate_commands(
    root: &Path,
    relative: &str,
    inventory: &HashMap<String, String>,
) -> Result<bool, Spec031ReleaseArtifactError> {
    require_inventory(inventory, relative)?;
    let registry: CommandRegistry = read_json(root, relative)?;
    if registry.test_summary.passed == 0 {
        return Err(Spec031ReleaseArtifactError::ZeroTestsRun);
    }
    let mut passed = 0u64;
    let mut failed = 0u64;
    for command in registry.commands {
        require_inventory(inventory, &command.transcript)?;
        let transcript = read_text(root, &command.transcript)?;
        if command.exit_code != 0 || command.failed.unwrap_or(0) != 0 {
            return Err(Spec031ReleaseArtifactError::CommandFailed);
        }
        match command.kind.as_deref() {
            None => {
                let claimed_passed = command.passed.ok_or_else(|| {
                    evidence_error(format!("{} missing passed count", command.id))
                })?;
                let claimed_failed = command.failed.ok_or_else(|| {
                    evidence_error(format!("{} missing failed count", command.id))
                })?;
                if claimed_passed == 0 {
                    return Err(Spec031ReleaseArtifactError::ZeroTestsRun);
                }
                let parsed = parse_cargo_test_counts_strict(&transcript).map_err(|_| {
                    evidence_error(format!(
                        "{} transcript has no Cargo test summary",
                        command.id
                    ))
                })?;
                if parsed.tests_run != claimed_passed + claimed_failed
                    || parsed.tests_failed != claimed_failed
                {
                    return Err(evidence_error(format!(
                        "{} transcript count mismatch",
                        command.id
                    )));
                }
                passed += claimed_passed;
                failed += claimed_failed;
            }
            Some("build" | "format" | "lint") => {
                if command.passed.is_some() || command.failed.is_some() {
                    return Err(evidence_error(format!(
                        "{} non-test command carries test counts",
                        command.id
                    )));
                }
            }
            Some(kind) => {
                return Err(evidence_error(format!(
                    "{} has unknown command kind {kind}",
                    command.id
                )));
            }
        }
    }
    if passed != registry.test_summary.passed || failed != registry.test_summary.failed {
        return Err(evidence_error("command registry testSummary mismatch"));
    }
    Ok(registry.result != "PASS"
        || registry.workspace_gate.status != "PASS"
        || registry.workspace_gate.exit_code != 0
        || registry.workspace_gate.failed != 0)
}
