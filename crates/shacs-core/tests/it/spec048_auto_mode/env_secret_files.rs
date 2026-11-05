//! Spec 048 PRD 010 acceptance 6: `.env.json`, its temporary file and the
//! config migrations' backups and secrets marker are secret files in every
//! mode, including file tool paths that 022 redaction erases.

use super::support::{
    call, run, run_answered, Exec, Options, Outcome, Setup, TestResult, ALL_MODES,
};
use serde_json::{json, Value};
use shacs_core::runtime::{ApprovalDecisionKind, PermissionMode};
use shacs_core::tools::{JsonMap, SchemaFragment, Tool, ToolParameters, ToolRegistry, ToolResult};
use std::fs;
use std::os::unix::fs::symlink;

const ENV_VALUE: &str = "spec048-env-value-8842";
const SECRET_FILES: [&str; 6] = [
    "..env.json.tmp-1",
    "config.json.migration-backup",
    "config.json.secrets-migration-backup",
    "config.json.secrets-migration-env-backup",
    "config.json.secrets-migration-in-progress",
    ".config.json.secrets-migration-backup.tmp-2",
];

/// `.env.json` and the other secret files in the data folder, with a
/// symlink and a hard link to `.env.json` beside them.
fn with_env_files(setup: &Setup) -> TestResult {
    fs::write(
        setup.data.join(".env.json"),
        format!("{{\"SHACS_CHANNELS_DISCORD_TOKEN\": \"{ENV_VALUE}\"}}"),
    )?;
    for name in SECRET_FILES {
        fs::write(
            setup.data.join(name),
            format!("{{\"token\": \"{ENV_VALUE}\"}}"),
        )?;
    }
    fs::create_dir_all(setup.workspace.join("links"))?;
    symlink(
        setup.data.join(".env.json"),
        setup.workspace.join("links/env-link.json"),
    )?;
    fs::hard_link(
        setup.data.join(".env.json"),
        setup.workspace.join("links/env-copy.json"),
    )?;
    Ok(())
}

/// A `notebook_read` tool, which its name alone makes a file read.
struct NotebookRead;

impl Tool for NotebookRead {
    fn name(&self) -> &str {
        "notebook_read"
    }

    fn description(&self) -> &str {
        "Reads a notebook (recorded)."
    }

    fn parameters(&self) -> Value {
        ToolParameters::new().to_json_schema()
    }

    fn execute(&self, _params: JsonMap) -> ToolResult {
        "read".into()
    }
}

fn registry(setup: &Setup) -> (ToolRegistry, super::support::Recorded) {
    let (mut registry, recorded) = setup.registry(Exec::Gated);
    registry.register(NotebookRead);
    (registry, recorded)
}

fn reads(setup: &Setup) -> Vec<(&'static str, &'static str, Value, &'static str)> {
    let absolute = setup.data.join(".env.json").display().to_string();
    let mut reads = vec![
        (
            "read_file",
            "read_file",
            json!({"path": ".env.json"}),
            ".env.json",
        ),
        (
            "read_file ./",
            "read_file",
            json!({"path": "./.env.json"}),
            ".env.json",
        ),
        (
            "read_file absolute",
            "read_file",
            json!({"path": absolute}),
            ".env.json",
        ),
        (
            "notebook_read",
            "notebook_read",
            json!({"path": ".env.json"}),
            ".env.json",
        ),
        (
            "symlink",
            "read_file",
            json!({"path": "links/env-link.json"}),
            "env-link.json",
        ),
        (
            "hard link",
            "read_file",
            json!({"path": "links/env-copy.json"}),
            "env-copy.json",
        ),
        (
            "cat",
            "exec",
            json!({"command": "cat .env.json"}),
            ".env.json",
        ),
        (
            "cat backup",
            "exec",
            json!({"command": "cat config.json.migration-backup"}),
            "config.json.migration-backup",
        ),
        (
            "here-doc",
            "exec",
            json!({"command": "python3 - <<'EOF'\nprint(open('.env.json').read())\nEOF"}),
            ".env.json",
        ),
        (
            "python3 -c",
            "exec",
            json!({"command": "python3 -c \"print(open('.env.json').read())\""}),
            ".env.json",
        ),
    ];
    for name in SECRET_FILES {
        reads.push((
            "read_file secret file",
            "read_file",
            json!({"path": name}),
            name,
        ));
    }
    reads
}

fn finish(failures: Vec<String>) -> TestResult {
    if failures.is_empty() {
        Ok(())
    } else {
        Err(failures.join("\n").into())
    }
}

#[test]
fn reads_of_env_and_migration_files_ask_once_in_every_mode() -> TestResult {
    let mut failures = Vec::new();
    for mode in ALL_MODES {
        for interactive in [true, false] {
            // Given
            let setup = Setup::workspace_is_data()?;
            with_env_files(&setup)?;
            let (registry, recorded) = registry(&setup);
            let context = setup.context(&Options::mode(mode, interactive));
            for (label, tool, arguments, name) in reads(&setup) {
                // When
                let outcome = run(&registry, &context, call(tool, arguments.clone()));

                // Then
                let asks = interactive && mode != PermissionMode::DontAsk;
                let fine = match &outcome {
                    Outcome::Asked(question) => {
                        asks && question.once_only()
                            && question.text.contains("secret file")
                            && question.text.contains(name)
                            && !question.text.contains(ENV_VALUE)
                    }
                    Outcome::Refused(reason) => !asks && !reason.contains(ENV_VALUE),
                    Outcome::Ran(_) => false,
                };
                if !fine {
                    failures.push(format!("{label} {mode:?} {interactive}: {outcome:?}"));
                }
            }
            if !recorded.launched().is_empty() {
                failures.push(format!("{mode:?} launched {:?}", recorded.launched()));
            }
        }
    }
    finish(failures)
}

#[test]
fn a_restored_path_decides_but_the_question_still_shows_it_redacted() -> TestResult {
    // Given
    let setup = Setup::workspace_is_data()?;
    with_env_files(&setup)?;
    let (registry, _) = registry(&setup);
    let context = setup.context(&Options::auto(true));

    // When
    let outcome = run(
        &registry,
        &context,
        call("read_file", json!({"path": ".env.json"})),
    );

    // Then
    let question = outcome.question().ok_or("not asked")?;
    assert!(
        question
            .text
            .contains("Arguments: `{\"path\":\"[REDACTED]\"}`"),
        "{}",
        question.text
    );
    assert!(question.text.contains("Why: `.env.json` is a secret file"));
    Ok(())
}

#[test]
fn an_approved_env_read_runs_that_call_only() -> TestResult {
    for mode in [PermissionMode::Auto, PermissionMode::BypassPermissions] {
        // Given
        let setup = Setup::workspace_is_data()?;
        with_env_files(&setup)?;
        let (registry, _) = registry(&setup);
        let context = setup.context(&Options::mode(mode, true));
        let read = call("read_file", json!({"path": ".env.json"}));

        // When
        let approved = run_answered(
            &registry,
            &context,
            read.clone(),
            ApprovalDecisionKind::Approved,
        )?;
        let again = run(&registry, &context, read.clone());
        let denied = run_answered(&registry, &context, read, ApprovalDecisionKind::Denied)?;

        // Then
        assert!(
            matches!(&approved, Outcome::Ran(text) if text.contains(ENV_VALUE)),
            "{mode:?} {approved:?}"
        );
        assert!(again
            .question()
            .is_some_and(|question| question.once_only()));
        assert!(!denied.ran(), "{denied:?}");
        assert!(!format!("{denied:?}").contains(ENV_VALUE));
    }
    Ok(())
}

#[test]
fn writes_to_env_and_migration_files_are_refused_in_every_mode() -> TestResult {
    let mut failures = Vec::new();
    let writes = [
        ("write_file", json!({"path": ".env.json", "content": "{}"})),
        (
            "edit_file",
            json!({"path": ".env.json", "old_text": "SHACS", "new_text": "X"}),
        ),
        (
            "write_file",
            json!({"path": "config.json.secrets-migration-backup", "content": "{}"}),
        ),
        (
            "write_file",
            json!({"path": "..env.json.tmp-1", "content": "{}"}),
        ),
        ("exec", json!({"command": "echo {} > .env.json"})),
        (
            "exec",
            json!({"command": "echo {} > config.json.secrets-migration-in-progress"}),
        ),
        ("exec", json!({"command": "rm .env.json"})),
    ];
    for mode in ALL_MODES {
        for interactive in [true, false] {
            // Given
            let setup = Setup::workspace_is_data()?;
            with_env_files(&setup)?;
            let (registry, recorded) = registry(&setup);
            let context = setup.context(&Options::mode(mode, interactive));
            for (tool, arguments) in &writes {
                // When
                let outcome = run(&registry, &context, call(tool, arguments.clone()));

                // Then
                if outcome.refusal().is_none() {
                    failures.push(format!(
                        "{tool} {arguments} {mode:?} {interactive}: {outcome:?}"
                    ));
                }
            }
            if !recorded.launched().is_empty() {
                failures.push(format!("{mode:?} launched {:?}", recorded.launched()));
            }
            if !fs::read_to_string(setup.data.join(".env.json"))?.contains(ENV_VALUE) {
                failures.push(format!("{mode:?} changed .env.json"));
            }
        }
    }
    finish(failures)
}

/// The text a scan shows when it runs, answering a question once if the mode
/// asks first.
fn scanned(
    registry: &ToolRegistry,
    context: &shacs_core::runtime::ToolExecutionContext,
    tool: &str,
    arguments: Value,
) -> Result<Option<String>, Box<dyn std::error::Error>> {
    let outcome = match run(registry, context, call(tool, arguments.clone())) {
        Outcome::Asked(_) => run_answered(
            registry,
            context,
            call(tool, arguments),
            ApprovalDecisionKind::Approved,
        )?,
        other => other,
    };
    Ok(match outcome {
        Outcome::Ran(text) => Some(text),
        Outcome::Asked(_) | Outcome::Refused(_) => None,
    })
}

#[test]
fn folder_scans_leave_env_and_migration_files_out_in_every_mode() -> TestResult {
    let mut failures = Vec::new();
    for mode in ALL_MODES {
        // Given
        let setup = Setup::workspace_is_data()?;
        with_env_files(&setup)?;
        let (registry, _) = registry(&setup);
        let context = setup.context(&Options::mode(mode, true));

        // When
        let shown = [
            scanned(
                &registry,
                &context,
                "list_dir",
                json!({"path": ".", "recursive": true}),
            )?,
            scanned(&registry, &context, "glob", json!({"pattern": "**/*"}))?,
            scanned(
                &registry,
                &context,
                "grep",
                json!({"pattern": "8842", "path": ".", "output_mode": "content"}),
            )?,
        ];

        // Then
        for text in shown.iter().flatten() {
            let leaked = text.contains(ENV_VALUE)
                || text.contains(".env.json")
                || text.contains("migration-backup")
                || text.contains("secrets-migration")
                || text.contains("env-copy.json");
            if leaked {
                failures.push(format!("{mode:?}: {text}"));
            }
        }
        if shown[0]
            .as_deref()
            .is_some_and(|text| !text.contains("notes.md"))
        {
            failures.push(format!("{mode:?} listed nothing: {:?}", shown[0]));
        }
        if shown[2]
            .as_deref()
            .is_some_and(|text| !text.starts_with("No matches found"))
        {
            failures.push(format!("{mode:?} grep found: {:?}", shown[2]));
        }
    }
    finish(failures)
}

#[test]
fn reads_the_file_tools_cannot_reach_or_whole_redacted_commands_are_refused_unasked() -> TestResult
{
    let mut failures = Vec::new();
    for mode in ALL_MODES {
        // Given
        let outside = Setup::new()?;
        with_env_files(&outside)?;
        let inside = Setup::workspace_is_data()?;
        with_env_files(&inside)?;
        fs::create_dir_all(inside.workspace.join(".ssh"))?;
        fs::write(inside.workspace.join(".ssh/config"), "Host *")?;
        let cases = [
            (
                &outside,
                "read_file",
                json!({"path": outside.data.join(".env.json").display().to_string()}),
            ),
            (&inside, "exec", json!({"command": "/bin/cat .env.json"})),
            (&inside, "read_file", json!({"path": ".ssh/config"})),
        ];
        for (setup, tool, arguments) in cases {
            let (registry, recorded) = registry(setup);
            let context = setup.context(&Options::mode(mode, true));

            // When
            let outcome = run(&registry, &context, call(tool, arguments.clone()));

            // Then
            let leaked = format!("{outcome:?}").contains(ENV_VALUE);
            if outcome.question().is_some() || leaked || !recorded.launched().is_empty() {
                failures.push(format!("{tool} {arguments} {mode:?}: {outcome:?}"));
            }
            if matches!(&outcome, Outcome::Ran(text) if !text.starts_with("Error")) {
                failures.push(format!("{tool} {arguments} {mode:?} ran: {outcome:?}"));
            }
        }
    }
    finish(failures)
}
