use super::support::{
    call, run, run_answered, Exec, Options, Outcome, Rules, Setup, TestResult, Verdict, ALL_MODES,
};
use serde_json::{json, Map, Value};
use shacs_core::runtime::{ApprovalDecisionKind, PermissionMode};
use shacs_core::tools::{ExecConfig, ExecTool, PathContext, ReadFileTool, Tool, ToolRegistry};
use std::fs;
use std::os::unix::fs::symlink;

const COULD_NOT_CHECK: &str = "Why: Auto mode could not check this action";

fn finish(failures: Vec<String>) -> TestResult {
    if failures.is_empty() {
        Ok(())
    } else {
        Err(failures.join("\n").into())
    }
}

#[test]
fn writes_to_each_protected_kind_go_to_the_classifier_even_with_an_allow_rule() -> TestResult {
    // Given
    let setup = Setup::workspace_is_data()?;
    let (registry, _) = setup.registry(Exec::Gated);
    let allow = Rules::allow(&["edit"]);
    let targets = [
        ".git/config",
        ".vscode/settings.json",
        ".bashrc",
        "nested/.npmrc",
        ".mcp.json",
        ".shacs-bot/plugins/tool/plugin.json",
        "plugins/tool/plugin.json",
        "config.json",
        "runtime/state.json",
        "sessions/cli_direct.jsonl",
        "AGENTS.md",
        "deep/CLAUDE.md",
        "skills/youtube/SKILL.md",
        "workflows/daily.md",
        "agents/helper.md",
        "permissions.json",
    ];
    let mut failures = Vec::new();
    for target in targets {
        let context = setup.context(&Options::auto(true).rules(allow.clone()));

        // When
        let outcome = run(
            &registry,
            &context,
            call("write_file", json!({"path": target, "content": "x"})),
        );

        // Then
        if !outcome
            .question()
            .is_some_and(|question| question.text.contains(COULD_NOT_CHECK))
        {
            failures.push(format!("{target}: {outcome:?}"));
        }
        if fs::read_to_string(setup.workspace.join(target)).is_ok_and(|text| text == "x") {
            failures.push(format!("{target} was written"));
        }
    }
    let plain = run(
        &registry,
        &setup.context(&Options::auto(true)),
        call("write_file", json!({"path": "draft.txt", "content": "x"})),
    );
    if !plain.ran() {
        failures.push(format!("plain workspace write: {plain:?}"));
    }
    finish(failures)
}

#[test]
fn a_redirect_whose_real_path_is_protected_asks_once() -> TestResult {
    // Given
    let setup = Setup::new()?;
    symlink(
        setup.workspace.join(".git"),
        setup.workspace.join("gitlink"),
    )?;
    let (registry, recorded) = setup.registry(Exec::Gated);
    let context = setup.context(
        &Options::auto(true)
            .rules(Rules::allow(&["exec(echo *)"]))
            .verdict(Verdict::Allow),
    );

    // When
    let outcome = run(
        &registry,
        &context,
        call("exec", json!({"command": "echo hi > gitlink/info/exclude"})),
    );

    // Then
    let question = outcome.question().ok_or("not asked")?;
    assert!(question.once_only());
    assert!(
        question
            .text
            .contains("follows a symlink to the protected path"),
        "{}",
        question.text
    );
    assert!(recorded.launched().is_empty());
    Ok(())
}

fn critical_commands(setup: &Setup) -> Vec<String> {
    let ws = setup.workspace.display().to_string();
    let data = setup.data.display().to_string();
    let home = setup.home.display().to_string();
    let parent = setup.root.display().to_string();
    vec![
        "rm -r --force /".to_owned(),
        "rmdir /usr".to_owned(),
        format!("rm -r --force {home}"),
        format!("rmdir {ws}"),
        format!("rm -r --force {parent}"),
        format!("rm -r --force {data}"),
        "rm -r --force /*".to_owned(),
        format!("rm -r --force {ws}/*"),
        "rm -r --force ~/*".to_owned(),
        "rm -r --force logs/*/*".to_owned(),
        "rm -r --force /Volumes/Backup".to_owned(),
        "rm -r --force /mnt/usb".to_owned(),
        "rm -r --force \"$DIR\"/*".to_owned(),
        "rm -r --force \"$TMPDIR/usr\"".to_owned(),
        "cd \"$SOMEWHERE\" && rm -r --force build".to_owned(),
        "bash -c 'rm -rf ~'".to_owned(),
        "if true; then rm -rf build; fi".to_owned(),
    ]
}

#[test]
fn critical_removals_ask_once_in_every_mode_and_skip_allow_rules_and_the_classifier() -> TestResult
{
    let mut failures = Vec::new();
    for mode in ALL_MODES {
        for interactive in [true, false] {
            // Given
            let setup = Setup::new()?;
            let (registry, recorded) = setup.registry(Exec::GatedPermissive);
            let context = setup.context(
                &Options::mode(mode, interactive)
                    .rules(Rules::allow(&["exec"]))
                    .verdict(Verdict::Allow),
            );
            for command in critical_commands(&setup) {
                // When
                let outcome = run(
                    &registry,
                    &context,
                    call("exec", json!({"command": command})),
                );

                // Then
                let asks = interactive && mode != PermissionMode::DontAsk;
                let fine = match &outcome {
                    Outcome::Asked(question) => {
                        asks && question.once_only() && question.text.contains("rm can remove")
                            || asks
                                && question.once_only()
                                && question.text.contains("critical folder")
                    }
                    Outcome::Refused(text) => !asks && text.contains("CriticalPathRemoval"),
                    Outcome::Ran(_) => false,
                };
                if !fine {
                    failures.push(format!("{command} {mode:?} {interactive}: {outcome:?}"));
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
fn an_approved_critical_removal_reaches_the_fake_process() -> TestResult {
    // Given
    let setup = Setup::new()?;
    let (registry, recorded) = setup.registry(Exec::GatedPermissive);
    let context = setup.context(&Options::auto(true));
    let command = format!("rm -r --force {}", setup.home.display());

    // When
    let outcome = run_answered(
        &registry,
        &context,
        call("exec", json!({"command": command})),
        ApprovalDecisionKind::Approved,
    )?;

    // Then
    assert!(
        matches!(&outcome, Outcome::Ran(text) if text == "launched"),
        "{outcome:?}"
    );
    assert_eq!(recorded.launched(), [command]);
    assert!(setup.home.exists());
    Ok(())
}

#[test]
fn bound_positional_parameters_and_named_files_are_not_critical() -> TestResult {
    // Given
    let setup = Setup::new()?;
    let (registry, _) = setup.registry(Exec::GatedPermissive);
    let context = setup.context(&Options::auto(true));

    for command in [
        "sh -c 'rm -rf \"$1\"/*' _ build",
        "rm -f \"$OUT/report.txt\"",
    ] {
        // When
        let outcome = run(
            &registry,
            &context,
            call("exec", json!({"command": command})),
        );

        // Then
        let question = outcome.question().ok_or("not asked")?;
        assert!(
            question.text.contains(COULD_NOT_CHECK),
            "{command}: {}",
            question.text
        );
    }
    Ok(())
}

#[test]
fn the_default_exec_guard_decides_before_the_critical_path_question() -> TestResult {
    let mut failures = Vec::new();
    for mode in ALL_MODES {
        // Given
        let setup = Setup::new()?;
        let (registry, recorded) = setup.registry(Exec::Gated);
        let context = setup.context(&Options::mode(mode, true));
        let ws = setup.workspace.display().to_string();

        // When
        let long_flags = run(
            &registry,
            &context,
            call(
                "exec",
                json!({"command": format!("rm --recursive --force {ws}")}),
            ),
        );
        let refused = [format!("rm -rf {ws}"), "rm -r ~".to_owned()].map(|command| {
            run(
                &registry,
                &context,
                call("exec", json!({"command": command})),
            )
        });

        // Then
        let critical = |outcome: &Outcome| {
            outcome
                .question()
                .is_some_and(|question| question.text.contains("critical folder"))
        };
        if mode != PermissionMode::DontAsk && !critical(&long_flags) {
            failures.push(format!("{mode:?} long flags: {long_flags:?}"));
        }
        for outcome in &refused {
            if critical(outcome) {
                failures.push(format!("{mode:?} guard-blocked removal asked: {outcome:?}"));
            }
            if mode == PermissionMode::Auto
                && !matches!(outcome, Outcome::Ran(text) if text.contains("Command blocked by safety guard"))
            {
                failures.push(format!("auto guard-blocked removal: {outcome:?}"));
            }
        }
        if !recorded.launched().is_empty() {
            failures.push(format!("{mode:?} launched {:?}", recorded.launched()));
        }
    }
    finish(failures)
}

#[test]
fn calls_the_tool_would_refuse_are_refused_with_the_tool_text() -> TestResult {
    // Given
    let setup = Setup::new()?;
    let (registry, recorded) = setup.registry(Exec::Gated);
    let outside = setup.home.join("x.txt");
    fs::write(&outside, "outside")?;
    let cases = [
        ("read_file", json!({"path": outside})),
        ("list_dir", json!({"path": setup.home})),
        ("grep", json!({"pattern": "x", "path": setup.home})),
        (
            "write_file",
            json!({"path": setup.home.join("y.txt"), "content": "x"}),
        ),
        ("exec", json!({"command": "rm -rf build"})),
        (
            "exec",
            json!({"command": format!("cat {}", outside.display())}),
        ),
        ("exec", json!({"command": "cat ../x.txt"})),
    ];
    let real_exec = ExecTool::new(ExecConfig::new(setup.path_context()));
    let mut failures = Vec::new();
    for interactive in [true, false] {
        let context = setup.context(&Options::auto(interactive).verdict(Verdict::Allow));
        for (tool, arguments) in &cases {
            let params = arguments.as_object().cloned().unwrap_or_else(Map::new);
            let expected = if *tool == "exec" {
                real_exec.execute(params)
            } else {
                registry.get(tool).ok_or("missing tool")?.execute(params)
            };

            // When
            let outcome = run(&registry, &context, call(tool, arguments.clone()));

            // Then
            if !matches!(&outcome, Outcome::Ran(text) if *text == expected.clone().into_text()) {
                failures.push(format!(
                    "{tool} {arguments}: {outcome:?} expected {expected:?}"
                ));
            }
        }
    }
    let native = setup.context(&Options::auto(true).native().verdict(Verdict::Allow));
    let gate = run(
        &registry,
        &native,
        call("exec", json!({"command": "echo hi"})),
    );
    if !matches!(&gate, Outcome::Ran(text) if text == "Error: Process launch blocked by permission gate (Denied)")
    {
        failures.push(format!("native exec: {gate:?}"));
    }
    if !recorded.launched().is_empty() {
        failures.push(format!("launched {:?}", recorded.launched()));
    }
    finish(failures)
}

#[test]
fn attachments_reads_without_a_range_and_sandboxed_exec_follow_the_table() -> TestResult {
    // Given
    let setup = Setup::new()?;
    let (registry, _) = setup.registry(Exec::GatedPermissive);
    let outside = setup.home.join("x.txt");
    fs::write(&outside, "outside text")?;
    let context = setup.context(&Options::auto(true));
    let mut unbounded = ToolRegistry::new();
    unbounded.register(ReadFileTool::new(PathContext {
        workspace: Some(setup.workspace.clone()),
        allowed_dir: None,
        media_dir: None,
        extra_allowed_dirs: Vec::new(),
    }));

    // When
    let attachment = run(
        &registry,
        &context,
        call("read_file", json!({"path": setup.media.join("photo.txt")})),
    );
    let unbounded_read = run(
        &unbounded,
        &context,
        call("read_file", json!({"path": outside})),
    );
    let sandboxed = run(
        &registry,
        &context,
        call(
            "exec",
            json!({"command": format!("cat {}", outside.display())}),
        ),
    );

    // Then
    assert!(
        matches!(&attachment, Outcome::Ran(text) if text.contains("attachment text")),
        "{attachment:?}"
    );
    assert!(
        matches!(&unbounded_read, Outcome::Ran(text) if text.contains("outside text")),
        "{unbounded_read:?}"
    );
    let question = sandboxed.question().ok_or("sandboxed exec not asked")?;
    assert!(question.text.contains(COULD_NOT_CHECK));
    Ok(())
}

#[test]
fn other_modes_ask_no_secret_or_critical_question_for_calls_the_tool_refuses() -> TestResult {
    // Given
    let setup = Setup::new()?;
    let (registry, _) = setup.registry(Exec::Gated);
    let context = setup.context(&Options::mode(PermissionMode::Default, true));
    let ws = setup.workspace.display().to_string();
    let calls: [(&str, Value); 2] = [
        ("read_file", json!({"path": setup.data.join("auth.json")})),
        ("exec", json!({"command": format!("rm -rf {ws}")})),
    ];

    for (tool, arguments) in calls {
        // When
        let outcome = run(&registry, &context, call(tool, arguments));

        // Then
        let text = outcome
            .question()
            .map(|question| question.text.clone())
            .unwrap_or_default();
        assert!(
            !text.contains("secret file") && !text.contains("critical folder"),
            "{text}"
        );
    }
    Ok(())
}
