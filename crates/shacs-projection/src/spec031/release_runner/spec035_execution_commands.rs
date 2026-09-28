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
        let stderr = evidence.bytes(&command.stderr)?;
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
            let parsed = match &command.test_accounting {
                Some(file) if command.kind == CommandKind::WorkspaceTest => {
                    let accounting: super::spec035_test_counts::Accounting = evidence.json(file)?;
                    match accounting.schema {
                        super::spec035_test_counts::AccountingSchema::V1 => {}
                    }
                    evidence.identity(&accounting.run_id, &accounting.source_sha256)?;
                    if accounting.stdout != command.stdout || accounting.stderr != command.stderr {
                        return Err(Error::InvalidCommandEvidence);
                    }
                    let stderr =
                        std::str::from_utf8(&stderr).map_err(|_| Error::InvalidCommandEvidence)?;
                    super::spec035_test_counts::workspace_counts(text, stderr, &accounting.targets)?
                        .counts()
                }
                Some(_) => return Err(Error::InvalidCommandEvidence),
                None if command.kind == CommandKind::WorkspaceTest => {
                    let stderr =
                        std::str::from_utf8(&stderr).map_err(|_| Error::InvalidCommandEvidence)?;
                    super::spec035_test_counts::flat_workspace_counts(text, stderr)?
                }
                None => super::command::parse_cargo_test_counts_strict(text)?,
            };
            if parsed.tests_failed != 0 || counts.tests_failed != 0 {
                return Err(Error::NonzeroTestsFailed);
            }
            if parsed.tests_run == 0 || counts.tests_run == 0 {
                return Err(Error::ZeroTestsRun);
            }
            if &parsed != counts {
                return Err(Error::InvalidCommandEvidence);
            }
        } else if command.tests.is_some()
            || command.package.is_some()
            || command.filter.is_some()
            || command.test_accounting.is_some()
        {
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
                command.package.is_none() && command.filter.is_none() && workspace_argv(&argv)
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

pub(super) fn workspace_argv(argv: &[&str]) -> bool {
    let expected = [
        "cargo",
        "test",
        "--manifest-path",
        "crates/Cargo.toml",
        "--locked",
        "--workspace",
    ];
    argv == expected
        || (argv.starts_with(&expected) && argv[expected.len()..] == ["--no-fail-fast"])
}
