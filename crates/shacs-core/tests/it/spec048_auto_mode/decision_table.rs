use super::support::{call, run, Exec, Options, Outcome, Rules, Setup, TestResult, Verdict};
use serde_json::{json, Value};
use std::os::unix::fs::symlink;

#[derive(Debug, Clone, Copy)]
enum Cell {
    Run,
    Ask,
    Deny,
    Classifier,
    /// The tool's own refusal, starting with this text.
    Refuse(&'static str),
    Unused,
}

struct Row {
    name: &'static str,
    workspace_is_data: bool,
    exec: Exec,
    contained: bool,
    call: fn(&Setup) -> (&'static str, Value),
    /// Rules for the narrow allow, broad allow, ask and deny columns;
    /// `{media}` and `{home}` stand for those folders.
    rules: [&'static str; 4],
    /// Cells for no rule, narrow allow, broad allow, ask and deny.
    cells: [Cell; 5],
}

fn rows() -> Vec<Row> {
    use Cell::{Ask, Classifier, Deny, Refuse, Run, Unused};
    vec![
        Row {
            name: "workspace read",
            workspace_is_data: false,
            exec: Exec::Gated,
            contained: true,
            call: |_| ("read_file", json!({"path": "notes.md"})),
            rules: ["read(notes.md)", "", "read(notes.md)", "read(notes.md)"],
            cells: [Run, Run, Unused, Ask, Deny],
        },
        Row {
            name: "media read",
            workspace_is_data: false,
            exec: Exec::Gated,
            contained: true,
            call: |setup| ("read_file", json!({"path": setup.media.join("photo.txt")})),
            rules: [
                "read(/{media}/photo.txt)",
                "",
                "read(/{media}/**)",
                "read(/{media}/**)",
            ],
            cells: [Run, Run, Unused, Ask, Deny],
        },
        Row {
            name: "read outside the file tool range",
            workspace_is_data: false,
            exec: Exec::Gated,
            contained: true,
            call: |setup| ("read_file", json!({"path": setup.home.join("x.txt")})),
            rules: [
                "read(/{home}/x.txt)",
                "read",
                "read(/{home}/**)",
                "read(/{home}/**)",
            ],
            cells: [
                Refuse("Error reading file:"),
                Refuse("Error reading file:"),
                Refuse("Error reading file:"),
                Refuse("Error reading file:"),
                Deny,
            ],
        },
        Row {
            name: "exec the safety guard refuses",
            workspace_is_data: false,
            exec: Exec::Gated,
            contained: true,
            call: |_| ("exec", json!({"command": "rm -rf build"})),
            rules: ["exec(rm -rf build)", "exec", "exec(rm *)", "exec(rm *)"],
            cells: [
                Refuse("Error executing command: Error: Command blocked by safety guard"),
                Refuse("Error executing command: Error: Command blocked by safety guard"),
                Refuse("Error executing command: Error: Command blocked by safety guard"),
                Refuse("Error executing command: Error: Command blocked by safety guard"),
                Deny,
            ],
        },
        Row {
            name: "exec naming a path outside the workspace without a sandbox",
            workspace_is_data: false,
            exec: Exec::Gated,
            contained: true,
            call: |setup| {
                (
                    "exec",
                    json!({"command": format!("cat {}", setup.home.display())}),
                )
            },
            rules: ["exec(cat *)", "exec", "exec(cat *)", "exec(cat *)"],
            cells: [
                Refuse(
                    "Error executing command: Error: Command blocked by safety guard (path outside",
                ),
                Refuse(
                    "Error executing command: Error: Command blocked by safety guard (path outside",
                ),
                Refuse(
                    "Error executing command: Error: Command blocked by safety guard (path outside",
                ),
                Refuse(
                    "Error executing command: Error: Command blocked by safety guard (path outside",
                ),
                Deny,
            ],
        },
        Row {
            name: "exec outside a proven container",
            workspace_is_data: false,
            exec: Exec::Gated,
            contained: false,
            call: |_| ("exec", json!({"command": "echo hi"})),
            rules: ["exec(echo *)", "exec", "exec(echo *)", "exec(echo *)"],
            cells: [
                Refuse("Error: Process launch blocked by permission gate (Denied)"),
                Refuse("Error: Process launch blocked by permission gate (Denied)"),
                Refuse("Error: Process launch blocked by permission gate (Denied)"),
                Refuse("Error: Process launch blocked by permission gate (Denied)"),
                Deny,
            ],
        },
        Row {
            name: "workspace write",
            workspace_is_data: false,
            exec: Exec::Gated,
            contained: true,
            call: |_| ("write_file", json!({"path": "new.txt", "content": "x"})),
            rules: ["edit(new.txt)", "", "edit(new.txt)", "edit(new.txt)"],
            cells: [Run, Run, Unused, Ask, Deny],
        },
        Row {
            name: "protected path write",
            workspace_is_data: false,
            exec: Exec::Gated,
            contained: true,
            call: |_| ("write_file", json!({"path": ".git/config", "content": "x"})),
            rules: [
                "edit(.git/config)",
                "",
                "edit(.git/config)",
                "edit(.git/config)",
            ],
            cells: [Classifier, Classifier, Unused, Ask, Deny],
        },
        Row {
            name: "exec write whose real path is protected",
            workspace_is_data: false,
            exec: Exec::Gated,
            contained: true,
            call: |_| ("exec", json!({"command": "echo hi > gitlink/config"})),
            rules: ["exec(echo *)", "exec", "exec(echo *)", "exec(echo *)"],
            cells: [Ask, Ask, Ask, Ask, Deny],
        },
        Row {
            name: "exec write whose real path leaves the workspace",
            workspace_is_data: false,
            exec: Exec::Gated,
            contained: true,
            call: |_| ("exec", json!({"command": "echo hi > outlink/file.txt"})),
            rules: ["exec(echo *)", "exec", "exec(echo *)", "exec(echo *)"],
            cells: [Ask, Ask, Ask, Ask, Deny],
        },
        Row {
            name: "exec redirecting outside the workspace",
            workspace_is_data: false,
            exec: Exec::GatedPermissive,
            contained: true,
            call: |setup| {
                (
                    "exec",
                    json!({"command": format!("echo hi > {}", setup.root.join("out.txt").display())}),
                )
            },
            rules: ["exec(echo *)", "exec", "exec(echo *)", "exec(echo *)"],
            cells: [Classifier, Classifier, Classifier, Ask, Deny],
        },
        Row {
            name: "secret file read",
            workspace_is_data: true,
            exec: Exec::Gated,
            contained: true,
            call: |_| ("read_file", json!({"path": "auth.json"})),
            rules: ["read(auth.json)", "", "read(auth.json)", "read(auth.json)"],
            cells: [Ask, Ask, Unused, Ask, Deny],
        },
        Row {
            name: "secret file write",
            workspace_is_data: true,
            exec: Exec::Gated,
            contained: true,
            call: |_| ("write_file", json!({"path": "auth.json", "content": "{}"})),
            rules: ["edit(auth.json)", "", "edit(auth.json)", "edit(auth.json)"],
            cells: [Deny, Deny, Unused, Deny, Deny],
        },
        Row {
            name: "critical path removal",
            workspace_is_data: false,
            exec: Exec::GatedPermissive,
            contained: true,
            call: |setup| {
                (
                    "exec",
                    json!({"command": format!("rm -r --force {}", setup.workspace.display())}),
                )
            },
            rules: ["exec(rm -r --force *)", "exec", "exec(rm *)", "exec(rm *)"],
            cells: [Ask, Ask, Ask, Ask, Deny],
        },
        Row {
            name: "exec inside a container",
            workspace_is_data: false,
            exec: Exec::Gated,
            contained: true,
            call: |_| ("exec", json!({"command": "echo hi"})),
            rules: ["exec(echo *)", "exec", "exec(echo *)", "exec(echo *)"],
            cells: [Classifier, Run, Classifier, Ask, Deny],
        },
        Row {
            name: "exec 022 could not summarize",
            workspace_is_data: false,
            exec: Exec::Gated,
            contained: true,
            call: |_| ("exec", json!({"command": "python3 -m http.server --help"})),
            rules: [
                "exec(python3 -m http.server *)",
                "exec(python3 *)",
                "exec(python3 *)",
                "exec(python3 *)",
            ],
            cells: [Classifier, Run, Classifier, Ask, Deny],
        },
        Row {
            name: "web fetch",
            workspace_is_data: false,
            exec: Exec::Gated,
            contained: true,
            call: |_| ("web_fetch", json!({"url": "https://example.com/page"})),
            rules: [
                "web_fetch(domain:example.com)",
                "",
                "web_fetch",
                "web_fetch",
            ],
            cells: [Classifier, Run, Unused, Ask, Deny],
        },
        Row {
            name: "web search",
            workspace_is_data: false,
            exec: Exec::Gated,
            contained: true,
            call: |_| ("web_search", json!({"query": "weather"})),
            rules: ["web_search", "", "web_search", "web_search"],
            cells: [Classifier, Run, Unused, Ask, Deny],
        },
        Row {
            name: "MCP tool",
            workspace_is_data: false,
            exec: Exec::Gated,
            contained: true,
            call: |_| ("mcp_github_get_issue", json!({"number": 1})),
            rules: ["mcp_github", "", "mcp_github_*", "mcp_*"],
            cells: [Classifier, Run, Unused, Ask, Deny],
        },
        Row {
            name: "sub task",
            workspace_is_data: false,
            exec: Exec::Gated,
            contained: true,
            call: |_| ("spawn", json!({"task": "look around"})),
            rules: ["spawn", "", "spawn", "spawn"],
            cells: [Classifier, Classifier, Unused, Ask, Deny],
        },
        Row {
            name: "message",
            workspace_is_data: false,
            exec: Exec::Gated,
            contained: true,
            call: |_| ("message", json!({"content": "done"})),
            rules: ["message", "", "message", "message"],
            cells: [Classifier, Run, Unused, Ask, Deny],
        },
        Row {
            name: "cron",
            workspace_is_data: false,
            exec: Exec::Gated,
            contained: true,
            call: |_| ("cron", json!({"action": "list"})),
            rules: ["cron", "", "cron", "cron"],
            cells: [Classifier, Run, Unused, Ask, Deny],
        },
        Row {
            name: "image generation",
            workspace_is_data: false,
            exec: Exec::Gated,
            contained: true,
            call: |_| ("image_generate", json!({"prompt": "a cat"})),
            rules: ["image_generate", "", "image_generate", "image_generate"],
            cells: [Classifier, Run, Unused, Ask, Deny],
        },
        Row {
            name: "config change",
            workspace_is_data: false,
            exec: Exec::Gated,
            contained: true,
            call: |_| ("my", json!({"action": "show"})),
            rules: ["my", "", "my", "my"],
            cells: [Classifier, Run, Unused, Ask, Deny],
        },
    ]
}

fn column_rules(row: &Row, column: usize, setup: &Setup) -> Rules {
    let rule = |index: usize| {
        row.rules[index]
            .replace("{media}", &setup.media.display().to_string())
            .replace("{home}", &setup.home.display().to_string())
    };
    match column {
        0 => Rules::default(),
        1 | 2 => Rules::allow(&[&rule(column - 1)]),
        3 => Rules::ask(&[&rule(2)]),
        _ => Rules::deny(&[&rule(3)]),
    }
}

fn setup_for(row: &Row) -> Result<Setup, Box<dyn std::error::Error>> {
    let setup = if row.workspace_is_data {
        Setup::workspace_is_data()?
    } else {
        Setup::new()?
    };
    symlink(
        setup.workspace.join(".git"),
        setup.workspace.join("gitlink"),
    )?;
    let outside = setup.root.join("outside");
    std::fs::create_dir_all(&outside)?;
    symlink(&outside, setup.workspace.join("outlink"))?;
    Ok(setup)
}

/// What a cell means for one run: whether the call ran, was asked, or was
/// refused, and with which words.
fn check(
    row: &Row,
    column: usize,
    interactive: bool,
    verdict: Option<Verdict>,
    outcome: &Outcome,
) -> Result<(), String> {
    let cell = row.cells[column];
    let label = format!(
        "{} / column {column} / interactive {interactive} / verdict {verdict:?}: {outcome:?}",
        row.name
    );
    let asked_or_refused = |why: &str, reason: &str| match outcome {
        Outcome::Asked(question) if interactive && question.text.contains(why) => Ok(()),
        Outcome::Refused(text) if !interactive && text.contains(reason) => Ok(()),
        _ => Err(label.clone()),
    };
    match (cell, verdict) {
        (Cell::Run, _) | (Cell::Classifier, Some(Verdict::Allow)) => {
            outcome.ran().then_some(()).ok_or(label.clone())
        }
        (Cell::Ask, _) => asked_or_refused("Permission approval required", "Permission denied"),
        (Cell::Deny, _) => outcome.refusal().map(|_| ()).ok_or(label.clone()),
        (Cell::Classifier, None) => asked_or_refused(
            "Why: Auto mode could not check this action",
            "EvaluatorUnavailable",
        ),
        (Cell::Classifier, Some(Verdict::Block)) => asked_or_refused(
            "Why: Auto mode blocked this: [Data Exfiltration] sends files to an unnamed host",
            "EvaluatorUncertain",
        ),
        (Cell::Refuse(prefix), _) => match outcome {
            Outcome::Ran(text) if text.starts_with(prefix) => Ok(()),
            _ => Err(label),
        },
        (Cell::Unused, _) => Ok(()),
    }
}

#[test]
fn auto_mode_decides_each_cell_of_the_table() -> TestResult {
    let mut failures = Vec::new();
    for row in rows() {
        for column in 0..5 {
            if matches!(row.cells[column], Cell::Unused) {
                continue;
            }
            let verdicts: &[Option<Verdict>] = match row.cells[column] {
                Cell::Classifier => &[None, Some(Verdict::Allow), Some(Verdict::Block)],
                _ => &[None],
            };
            for interactive in [true, false] {
                for verdict in verdicts {
                    // Given
                    let setup = setup_for(&row)?;
                    let (registry, recorded) = setup.registry(row.exec);
                    let mut options =
                        Options::auto(interactive).rules(column_rules(&row, column, &setup));
                    options.contained = row.contained;
                    options.verdict = *verdict;
                    let context = setup.context(&options);
                    let (tool, arguments) = (row.call)(&setup);

                    // When
                    let outcome = run(&registry, &context, call(tool, arguments));

                    // Then
                    if let Err(failure) = check(&row, column, interactive, *verdict, &outcome) {
                        failures.push(failure);
                    }
                    if !outcome.ran() && !recorded.launched().is_empty() {
                        failures.push(format!("{} launched without running", row.name));
                    }
                }
            }
        }
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(failures.join("\n").into())
    }
}

#[test]
fn workspace_writes_go_to_the_classifier_without_allow_workspace_edits() -> TestResult {
    // Given
    let setup = Setup::new()?;
    let (registry, _) = setup.registry(Exec::Gated);
    let mut options = Options::auto(true);
    options.allow_workspace_edits = false;
    let context = setup.context(&options);

    // When
    let outcome = run(
        &registry,
        &context,
        call("write_file", json!({"path": "new.txt", "content": "x"})),
    );

    // Then
    let question = outcome.question().ok_or("not asked")?;
    assert!(question
        .text
        .contains("Why: Auto mode could not check this action"));
    assert!(!setup.workspace.join("new.txt").exists());
    Ok(())
}
