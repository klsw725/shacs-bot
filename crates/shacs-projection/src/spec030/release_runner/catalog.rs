use super::command_contract::CommandEvidenceMode;
use super::model::{
    Spec030CapturedFact, Spec030CoverageAssertion, Spec030CoverageRow, Spec030OwnerAudit,
    Spec030SurfaceArtifact,
};
use super::semantic::Spec030SurfaceAssertions;
use super::target_catalog::spec030_integration_targets;
use crate::{Spec031ReleaseCommandRecord, Spec031ReleaseCommandStatus};

pub(super) fn coverage(
    commands: &[Spec031ReleaseCommandRecord],
    surfaces: &Spec030SurfaceAssertions,
    evidence_mode: CommandEvidenceMode,
) -> Vec<Spec030CoverageRow> {
    [
        ("000", "trusted_runtime_profile", "surface-cli-json"),
        ("001", "pre_tool_hook", "spec030-core"),
        ("002", "process_controls", "spec030-core"),
        ("003", "credential_lifecycle", "surface-cli-json"),
        ("004", "optional_sandbox", "spec030-bwrap-active"),
        ("005", "resource_disclosure", "surface-cli-json"),
        ("006", "sequential_integration", "cargo-test-workspace"),
    ]
    .into_iter()
    .map(|(prd, owner_surface, surface_command)| {
        let mut assertions = target_assertions(prd, commands);
        assertions.push(surface_assertion(prd, surface_command, commands, surfaces));
        if prd == "006" {
            if let Some(command) = evidence_mode.owner_lifecycle_id() {
                assertions.push(Spec030CoverageAssertion {
                    id: "live:linux-owner-lifecycle".to_owned(),
                    evidence: format!("commands/{command}.stdout"),
                    passed: command_succeeded(commands, command),
                });
            }
        }
        let passed = assertions.iter().all(|assertion| assertion.passed);
        let command_ids = spec030_integration_targets()
            .iter()
            .filter(|target| target.prds.contains(&prd))
            .map(|target| target.command_id.to_owned())
            .chain([surface_command.to_owned()])
            .chain(
                (prd == "006")
                    .then(|| evidence_mode.owner_lifecycle_id())
                    .flatten()
                    .map(str::to_owned),
            )
            .collect::<Vec<_>>();
        Spec030CoverageRow {
            prd: prd.to_owned(),
            owner_surface: owner_surface.to_owned(),
            evidence: assertions
                .iter()
                .map(|assertion| assertion.evidence.clone())
                .collect(),
            command_ids,
            assertions,
            passed,
        }
    })
    .collect()
}

fn target_assertions(
    prd: &str,
    commands: &[Spec031ReleaseCommandRecord],
) -> Vec<Spec030CoverageAssertion> {
    spec030_integration_targets()
        .iter()
        .filter(|target| target.prds.contains(&prd))
        .map(|target| {
            let command = commands
                .iter()
                .find(|command| command.id == target.command_id);
            Spec030CoverageAssertion {
                id: format!("target:{}:nonzero", target.target),
                evidence: format!("commands/{}.stdout", target.command_id),
                passed: command.is_some_and(|command| {
                    command.status == Spec031ReleaseCommandStatus::Passed
                        && command
                            .tests
                            .as_ref()
                            .is_some_and(|tests| tests.tests_run > 0 && tests.tests_failed == 0)
                }),
            }
        })
        .collect()
}

fn surface_assertion(
    prd: &str,
    command: &str,
    commands: &[Spec031ReleaseCommandRecord],
    surfaces: &Spec030SurfaceAssertions,
) -> Spec030CoverageAssertion {
    let (id, evidence, passed) = match prd {
        "000" => (
            "live:projection-schema-status",
            "surface/cli.json",
            surfaces.schema_version == 1 && surfaces.feature_assertions.prd000_trusted_profile,
        ),
        "001" => (
            "live:hook-surface-captured",
            "surface/cli.json",
            surfaces.feature_assertions.prd001_active_hooks,
        ),
        "002" => (
            "live:process-controls-observed",
            "surface/cli.json",
            surfaces.feature_assertions.prd002_process_controls,
        ),
        "003" => (
            "live:credential-status-captured",
            "surface/cli.json",
            surfaces.feature_assertions.prd003_credential_lifecycle,
        ),
        "004" => (
            "live:bwrap-producer-transcript",
            "commands/spec030-bwrap-active.stdout",
            command_succeeded(commands, command)
                && surfaces.feature_assertions.prd004_active_sandbox,
        ),
        "005" => (
            "live:resource-disclosure-captured",
            "surface/cli.json",
            surfaces.feature_assertions.prd005_resource_disclosure,
        ),
        "006" => (
            "live:runner-surface-integrity",
            "surface/api.json",
            command_succeeded(commands, command)
                && surfaces.feature_assertions.prd006_surface_integrity,
        ),
        _ => ("invalid", "invalid", false),
    };
    Spec030CoverageAssertion {
        id: id.to_owned(),
        evidence: evidence.to_owned(),
        passed,
    }
}

pub(super) fn audits(
    commands: &[Spec031ReleaseCommandRecord],
    assertions: &Spec030SurfaceAssertions,
) -> Vec<Spec030OwnerAudit> {
    [
        (
            "031",
            "config/profile/auth locator and runtime layout",
            "crates/shacs-config/tests/spec030_auth_resolution.rs",
            "surface-cli-json",
        ),
        (
            "032",
            "trusted-code lifecycle disclosure",
            "crates/shacs-core/tests/spec030_resource_selection.rs",
            "surface-cli-json",
        ),
        (
            "035",
            "CLI/API/TUI projection parity",
            "crates/shacs-tui/tests/spec030_shared_surfaces.rs",
            "surface-api-schema",
        ),
    ]
    .into_iter()
    .map(|(owner, fact, source, command)| Spec030OwnerAudit {
        owner: owner.to_owned(),
        fact: fact.to_owned(),
        source_locator: source.to_owned(),
        command_ids: vec![command.to_owned()],
        passed: command_succeeded(commands, command)
            && match owner {
                "031" => assertions.schema_version == 1 && !assertions.credential_status.is_empty(),
                "032" => assertions.raw_content_possible && !assertions.trace_status.is_empty(),
                "035" => {
                    assertions.cli_api_json_parity
                        && assertions.cli_human_tui_runtime_parity
                        && assertions.tui_no_session
                        && assertions.tui_runtime_owner_facts
                }
                _ => false,
            },
    })
    .collect()
}

pub(super) fn facts(assertions: &Spec030SurfaceAssertions) -> Vec<Spec030CapturedFact> {
    [
        ("projection", assertions.runtime_status.clone()),
        (
            "process",
            if assertions.supported_process_adapter_count > 0 {
                "supported".to_owned()
            } else {
                "unsupported".to_owned()
            },
        ),
        ("credential", assertions.credential_status.clone()),
        ("sandbox", assertions.sandbox_status.clone()),
        (
            "resource",
            if assertions.resource_count > 0 {
                "loaded".to_owned()
            } else {
                "empty".to_owned()
            },
        ),
        ("disclosure", assertions.trace_status.clone()),
    ]
    .into_iter()
    .map(|(id, status)| Spec030CapturedFact {
        id: id.to_owned(),
        status,
        evidence: vec!["surface/cli.json".to_owned()],
    })
    .collect()
}

fn command_succeeded(commands: &[Spec031ReleaseCommandRecord], id: &str) -> bool {
    commands
        .iter()
        .any(|command| command.id == id && command.status == Spec031ReleaseCommandStatus::Passed)
}

pub(super) fn surfaces() -> Vec<Spec030SurfaceArtifact> {
    [
        ("cli_json", "surface-cli-json", "surface/cli.json"),
        ("cli_human", "surface-cli-human", "surface/cli.txt"),
        ("api", "surface-api-schema", "surface/api.json"),
        (
            "tui_no_session",
            "surface-tui-no-session",
            "surface/tui-no-session.txt",
        ),
        (
            "tui_runtime",
            "surface-tui-runtime",
            "surface/tui-runtime.txt",
        ),
    ]
    .into_iter()
    .map(|(surface, command, artifact)| Spec030SurfaceArtifact {
        surface: surface.to_owned(),
        command_id: command.to_owned(),
        artifact: artifact.to_owned(),
    })
    .collect()
}
