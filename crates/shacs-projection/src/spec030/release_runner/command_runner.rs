use super::bwrap_runner::collect_bwrap;
use super::command_contract::{required_ids, CommandEvidenceMode, LIFECYCLE_COMMAND};
use super::fixture;
use super::model::*;
use super::records::push_blocker;
use super::semantic::fixture_surface_assertions;
use super::surface_runner::collect_surfaces;
use super::target_catalog::spec030_integration_targets;
use crate::release_evidence::EvidenceWriter;
use crate::{Spec031ReleaseCommandRecord, Spec031ReleaseCommandSpec, Spec031ReleaseGateKind};
use std::path::{Path, PathBuf};

pub(super) struct CollectedCommands {
    pub commands: Vec<Spec031ReleaseCommandRecord>,
    pub external_evidence: Vec<Spec030ExternalEvidence>,
    pub surface_assertions: super::semantic::Spec030SurfaceAssertions,
    pub surface_owner: Spec030SurfaceOwnerEvidence,
}

pub(super) struct Invocation {
    pub id: &'static str,
    pub package: Option<&'static str>,
    pub gate: Spec031ReleaseGateKind,
    pub cwd: PathBuf,
    pub argv: Vec<String>,
    pub filter: Option<&'static str>,
}

pub(super) fn collect(
    config: &Spec030ReleaseRunnerConfig,
    source_digest: &str,
    writer: &EvidenceWriter,
    blockers: &mut Vec<Spec030ReleaseBlocker>,
    evidence_mode: CommandEvidenceMode,
) -> Result<CollectedCommands, Spec030ReleaseArtifactError> {
    match config.mode {
        Spec030ReleaseRunnerMode::SuccessFixture => fixture_commands(config, writer, evidence_mode),
        Spec030ReleaseRunnerMode::CurrentWorktree => {
            current_commands(config, source_digest, writer, blockers)
        }
    }
}

fn fixture_commands(
    config: &Spec030ReleaseRunnerConfig,
    writer: &EvidenceWriter,
    evidence_mode: CommandEvidenceMode,
) -> Result<CollectedCommands, Spec030ReleaseArtifactError> {
    fixture::prepare(writer)?;
    let root = config.evidence_root.join("fixtures/success");
    let mut commands = spec030_integration_targets()
        .iter()
        .map(|target| {
            execute(
                config,
                writer,
                Invocation {
                    id: target.command_id,
                    package: Some(target.package),
                    gate: Spec031ReleaseGateKind::FocusedCargoTest,
                    cwd: root.clone(),
                    argv: strings(&["cargo", "test", "--test", target.target]),
                    filter: Some(target.target),
                },
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
    for id in required_ids(evidence_mode)
        .into_iter()
        .filter(|id| id.starts_with("cargo-"))
    {
        commands.push(execute(
            config,
            writer,
            Invocation {
                id,
                package: None,
                gate: Spec031ReleaseGateKind::FocusedCargoTest,
                cwd: root.clone(),
                argv: strings(&["cargo", "test", "--test", "spec030_fixture"]),
                filter: Some("spec030_fixture"),
            },
        )?);
    }
    for id in [
        "surface-tui-no-session",
        "surface-cli-json",
        "surface-cli-human",
        "surface-tui-runtime",
        "surface-api-schema",
        "spec030-bwrap-active",
    ] {
        commands.push(execute(
            config,
            writer,
            Invocation {
                id,
                package: None,
                gate: Spec031ReleaseGateKind::FocusedCargoTest,
                cwd: root.clone(),
                argv: strings(&["cargo", "test", "--test", "spec030_fixture"]),
                filter: Some("spec030_fixture"),
            },
        )?);
    }
    if evidence_mode == CommandEvidenceMode::LinuxCurrentWorktree {
        commands.push(execute_lifecycle(config, writer, &root)?);
    }
    let surface_assertions =
        super::semantic::parse_spec030_surface_assertions(&config.evidence_root.join("surface"))?;
    Ok(CollectedCommands {
        commands,
        external_evidence: Vec::new(),
        surface_assertions,
        surface_owner: super::surface_owner_evidence::fixture(writer)?,
    })
}

fn current_commands(
    config: &Spec030ReleaseRunnerConfig,
    source_digest: &str,
    writer: &EvidenceWriter,
    blockers: &mut Vec<Spec030ReleaseBlocker>,
) -> Result<CollectedCommands, Spec030ReleaseArtifactError> {
    let workspace_available = config.repo_root.join("crates/Cargo.toml").is_file();
    if !workspace_available {
        push_blocker(blockers, "missing_workspace", "crates/Cargo.toml is absent");
    }
    let (mut records, surface_assertions, surface_owner) = if workspace_available {
        let mut records = spec030_integration_targets()
            .iter()
            .map(|target| execute(config, writer, focused(config, target)))
            .collect::<Result<Vec<_>, _>>()?;
        for invocation in full_gates(config) {
            records.push(execute(config, writer, invocation)?);
        }
        let (surface_records, assertions, owner) = collect_surfaces(config, writer)?;
        records.extend(surface_records);
        (records, assertions, owner)
    } else {
        (
            Vec::new(),
            fixture_surface_assertions(),
            super::surface_owner_evidence::fixture(writer)?,
        )
    };
    let external_evidence = collect_bwrap(config, source_digest, writer, blockers, &mut records)?;
    Ok(CollectedCommands {
        commands: records,
        external_evidence,
        surface_assertions,
        surface_owner,
    })
}

fn full_gates(config: &Spec030ReleaseRunnerConfig) -> [Invocation; 3] {
    [
        cargo_gate(config, "cargo-fmt", &["fmt", "--all", "--", "--check"]),
        cargo_gate(
            config,
            "cargo-clippy-workspace",
            &[
                "clippy",
                "--workspace",
                "--all-targets",
                "--",
                "-D",
                "warnings",
            ],
        ),
        cargo_gate(config, "cargo-test-workspace", &["test", "--workspace"]),
    ]
}

fn cargo_gate(
    config: &Spec030ReleaseRunnerConfig,
    id: &'static str,
    arguments: &[&str],
) -> Invocation {
    let mut argv = vec!["cargo".to_owned(), arguments[0].to_owned()];
    argv.extend(strings(&["--manifest-path", "crates/Cargo.toml"]));
    argv.extend(arguments[1..].iter().map(|argument| (*argument).to_owned()));
    Invocation {
        id,
        package: None,
        gate: Spec031ReleaseGateKind::FullCargoGate,
        cwd: config.repo_root.clone(),
        argv,
        filter: None,
    }
}

fn focused(
    config: &Spec030ReleaseRunnerConfig,
    target: &'static super::target_catalog::Spec030IntegrationTarget,
) -> Invocation {
    let argv = strings(&[
        "cargo",
        "test",
        "--manifest-path",
        "crates/Cargo.toml",
        "-p",
        target.package,
        "--test",
        target.target,
    ]);
    Invocation {
        id: target.command_id,
        package: Some(target.package),
        gate: Spec031ReleaseGateKind::FocusedCargoTest,
        cwd: config.repo_root.clone(),
        argv,
        filter: Some(target.target),
    }
}

pub(super) fn execute(
    config: &Spec030ReleaseRunnerConfig,
    writer: &EvidenceWriter,
    invocation: Invocation,
) -> Result<Spec031ReleaseCommandRecord, Spec030ReleaseArtifactError> {
    crate::spec031::execute_spec031_release_command_with(
        writer,
        &Spec031ReleaseCommandSpec {
            id: invocation.id.to_owned(),
            gate: invocation.gate,
            package: invocation.package.map(str::to_owned),
            filter: invocation.filter.map(str::to_owned),
            argv: invocation.argv,
            cwd: invocation.cwd,
            timeout: config.command_timeout,
        },
    )
    .map_err(|_| Spec030ReleaseArtifactError::Io)
}

pub(super) fn execute_lifecycle(
    config: &Spec030ReleaseRunnerConfig,
    writer: &EvidenceWriter,
    cwd: &Path,
) -> Result<Spec031ReleaseCommandRecord, Spec030ReleaseArtifactError> {
    let canonical_cwd = cwd
        .canonicalize()
        .map_err(|_| Spec030ReleaseArtifactError::Io)?;
    let mut record = execute(
        config,
        writer,
        Invocation {
            id: LIFECYCLE_COMMAND.id,
            package: Some(LIFECYCLE_COMMAND.package),
            gate: LIFECYCLE_COMMAND.gate,
            cwd: canonical_cwd,
            argv: strings(LIFECYCLE_COMMAND.argv),
            filter: Some(LIFECYCLE_COMMAND.filter),
        },
    )?;
    let stdout = std::fs::read_to_string(config.evidence_root.join(&record.stdout_path))
        .map_err(|_| Spec030ReleaseArtifactError::Io)?;
    record.tests = crate::parse_cargo_test_counts(&stdout);
    Ok(record)
}

fn strings(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}
