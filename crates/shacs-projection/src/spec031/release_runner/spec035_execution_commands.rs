use super::model::Spec031ReleaseArtifactError as Error;
use super::spec035_execution_io::Evidence;
use super::spec035_execution_model::CommandKind;
use std::collections::HashSet;

pub(super) fn validate_commands(evidence: &Evidence<'_>) -> Result<(), Error> {
    let mut ids = HashSet::new();
    let mut transcripts = HashSet::new();
    for command in &evidence.execution.commands {
        if command.id.is_empty()
            || !ids.insert(&command.id)
            || command.argv.is_empty()
            || !transcripts.insert(&command.stdout.path)
            || !transcripts.insert(&command.stderr.path)
        {
            return Err(Error::InvalidCommandEvidence);
        }
        evidence.identity(&command.run_id, &command.source_sha256)?;
        let stdout = evidence.bytes(&command.stdout)?;
        evidence.bytes(&command.stderr)?;
        if command.exit_code != 0 {
            return Err(Error::CommandFailed);
        }
        if matches!(
            command.kind,
            CommandKind::FocusedTest | CommandKind::WorkspaceTest
        ) {
            let counts = command
                .tests
                .as_ref()
                .ok_or(Error::InvalidCommandEvidence)?;
            let text = std::str::from_utf8(&stdout).map_err(|_| Error::InvalidCommandEvidence)?;
            let parsed = super::command::parse_cargo_test_counts_strict(text)?;
            if parsed.tests_failed != 0 || counts.tests_failed != 0 {
                return Err(Error::NonzeroTestsFailed);
            }
            if parsed.tests_run == 0 || counts.tests_run == 0 {
                return Err(Error::ZeroTestsRun);
            }
            if &parsed != counts {
                return Err(Error::InvalidCommandEvidence);
            }
        } else if command.tests.is_some() || command.package.is_some() || command.filter.is_some() {
            return Err(Error::InvalidCommandEvidence);
        }
        let argv: Vec<_> = command.argv.iter().map(String::as_str).collect();
        let valid = match command.kind {
            CommandKind::FocusedTest => {
                let package = command
                    .package
                    .as_deref()
                    .filter(|value| !value.is_empty())
                    .ok_or(Error::InvalidCommandEvidence)?;
                let filter = command
                    .filter
                    .as_deref()
                    .filter(|value| !value.is_empty())
                    .ok_or(Error::ZeroTestsRun)?;
                let expected = [
                    "cargo",
                    "test",
                    "--manifest-path",
                    "crates/Cargo.toml",
                    "--locked",
                    "-p",
                    package,
                    filter,
                ];
                argv == expected
                    || (argv.starts_with(&expected) && argv[expected.len()..] == ["--", "--exact"])
            }
            CommandKind::WorkspaceTest => {
                command.package.is_none()
                    && command.filter.is_none()
                    && argv
                        == [
                            "cargo",
                            "test",
                            "--manifest-path",
                            "crates/Cargo.toml",
                            "--locked",
                            "--workspace",
                        ]
            }
            CommandKind::Format => {
                argv == [
                    "cargo",
                    "fmt",
                    "--manifest-path",
                    "crates/Cargo.toml",
                    "--all",
                    "--",
                    "--check",
                ]
            }
            CommandKind::Lint => {
                argv == [
                    "cargo",
                    "clippy",
                    "--manifest-path",
                    "crates/Cargo.toml",
                    "--locked",
                    "--workspace",
                    "--all-targets",
                    "--",
                    "-D",
                    "warnings",
                ]
            }
            CommandKind::Build => ["shacs-cli", "shacs-tui"].iter().any(|package| {
                argv == [
                    "cargo",
                    "build",
                    "--manifest-path",
                    "crates/Cargo.toml",
                    "--locked",
                    "-p",
                    package,
                ]
            }),
            CommandKind::Surface | CommandKind::Review => argv[0] != "cargo" && !argv[0].is_empty(),
        };
        if !valid {
            return Err(Error::InvalidCommandEvidence);
        }
    }
    if ids.is_empty() {
        return Err(Error::ZeroTestsRun);
    }
    Ok(())
}

pub(super) fn pair(argv: &[String], flag: &str, value: &str) -> bool {
    argv.windows(2)
        .any(|pair| pair[0] == flag && pair[1] == value)
}
