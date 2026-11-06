use super::support::{
    call, run, run_answered, Exec, Options, Outcome, Rules, Setup, TestResult, Verdict, ALL_MODES,
    SECRET_TOKEN,
};
use serde_json::{json, Value};
use shacs_core::runtime::{ApprovalDecisionKind, PermissionMode};
use std::fs;
use std::os::unix::fs::symlink;

fn reads(setup: &Setup) -> Vec<(&'static str, &'static str, Value)> {
    let data = setup.data.display().to_string();
    vec![
        ("read_file", "read_file", json!({"path": "auth.json"})),
        (
            "grep path",
            "grep",
            json!({"pattern": "token", "path": "auth.json"}),
        ),
        ("cat", "exec", json!({"command": "cat auth.json"})),
        (
            "input redirect",
            "exec",
            json!({"command": "wc -c < auth.json"}),
        ),
        (
            "cd then cat",
            "exec",
            json!({"command": format!("cd {data} && cat auth.json")}),
        ),
        (
            "here-doc body",
            "exec",
            json!({"command": "python3 - <<'EOF'\nprint(open('auth.json').read())\nEOF"}),
        ),
        (
            "python3 -c code",
            "exec",
            json!({"command": "python3 -c \"print(open('auth.json').read())\""}),
        ),
        ("symlink read", "read_file", json!({"path": "link.json"})),
        ("hard link read", "read_file", json!({"path": "copy.json"})),
        ("hard link cat", "exec", json!({"command": "cat copy.json"})),
        (
            "auth folder read",
            "read_file",
            json!({"path": "auth/oauth.json"}),
        ),
    ]
}

fn writes() -> Vec<(&'static str, &'static str, Value)> {
    vec![
        (
            "write_file",
            "write_file",
            json!({"path": "auth.json", "content": "{}"}),
        ),
        (
            "edit_file",
            "edit_file",
            json!({"path": "auth.json", "old_text": "token", "new_text": "x"}),
        ),
        (
            "redirect",
            "exec",
            json!({"command": "echo {} > auth.json"}),
        ),
        ("tee", "exec", json!({"command": "echo {} | tee auth.json"})),
        ("rm", "exec", json!({"command": "rm auth.json"})),
        ("mv", "exec", json!({"command": "mv auth.json moved.json"})),
        (
            "hard link write",
            "write_file",
            json!({"path": "copy.json", "content": "{}"}),
        ),
    ]
}

fn links(setup: &Setup) -> TestResult {
    symlink(
        setup.data.join("auth.json"),
        setup.workspace.join("link.json"),
    )?;
    fs::hard_link(
        setup.data.join("auth.json"),
        setup.workspace.join("copy.json"),
    )?;
    Ok(())
}

#[test]
fn reads_of_the_auth_store_ask_once_in_every_mode() -> TestResult {
    let mut failures = Vec::new();
    for mode in ALL_MODES {
        for interactive in [true, false] {
            let setup = Setup::workspace_is_data()?;
            links(&setup)?;
            let (registry, recorded) = setup.registry(Exec::Gated);
            let context = setup.context(&Options::mode(mode, interactive));
            for (label, tool, arguments) in reads(&setup) {
                // When
                let outcome = run(&registry, &context, call(tool, arguments.clone()));

                // Then
                let asks = interactive && mode != PermissionMode::DontAsk;
                let fine = match &outcome {
                    Outcome::Asked(question) => {
                        asks && question.once_only() && question.text.contains("secret file")
                    }
                    Outcome::Refused(_) => !asks,
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
fn an_approved_secret_read_runs_that_call_only_and_a_denied_one_reads_nothing() -> TestResult {
    for mode in [PermissionMode::Auto, PermissionMode::BypassPermissions] {
        // Given
        let setup = Setup::workspace_is_data()?;
        let (registry, _) = setup.registry(Exec::Gated);
        let context = setup.context(&Options::mode(mode, true));
        let read = call("read_file", json!({"path": "auth.json"}));

        // When
        let approved = run_answered(
            &registry,
            &context,
            read.clone(),
            ApprovalDecisionKind::Approved,
        )?;
        let again = run(&registry, &context, read.clone());
        let denied = run_answered(
            &registry,
            &context,
            read.clone(),
            ApprovalDecisionKind::Denied,
        )?;
        let session = run_answered(
            &registry,
            &context,
            read,
            ApprovalDecisionKind::ApprovedForSession,
        )?;

        // Then
        assert!(
            matches!(&approved, Outcome::Ran(text) if text.contains(SECRET_TOKEN)),
            "{approved:?}"
        );
        assert!(
            again
                .question()
                .is_some_and(|question| question.once_only()),
            "{again:?}"
        );
        assert!(!format!("{denied:?}").contains(SECRET_TOKEN), "{denied:?}");
        assert!(!denied.ran(), "{denied:?}");
        assert!(!session.ran(), "{session:?}");
    }
    Ok(())
}

#[test]
fn writes_to_the_auth_store_are_refused_in_every_mode() -> TestResult {
    let mut failures = Vec::new();
    for mode in ALL_MODES {
        for interactive in [true, false] {
            // Given
            let setup = Setup::workspace_is_data()?;
            links(&setup)?;
            let (registry, recorded) = setup.registry(Exec::Gated);
            let context = setup.context(&Options::mode(mode, interactive));

            for (label, tool, arguments) in writes() {
                // When
                let outcome = run(&registry, &context, call(tool, arguments));

                // Then
                if outcome.refusal().is_none() {
                    failures.push(format!("{label} {mode:?} {interactive}: {outcome:?}"));
                }
            }
            if !recorded.launched().is_empty() {
                failures.push(format!("{mode:?} launched {:?}", recorded.launched()));
            }
            if !fs::read_to_string(setup.data.join("auth.json"))?.contains(SECRET_TOKEN) {
                failures.push(format!("{mode:?} changed auth.json"));
            }
        }
    }
    finish(failures)
}

#[test]
fn folder_scans_leave_the_auth_store_out_in_auto_and_bypass_modes() -> TestResult {
    for mode in [PermissionMode::Auto, PermissionMode::BypassPermissions] {
        // Given
        let setup = Setup::workspace_is_data()?;
        links(&setup)?;
        let (registry, _) = setup.registry(Exec::Gated);
        let context = setup.context(&Options::mode(mode, true));

        // When
        let listed = run(
            &registry,
            &context,
            call("list_dir", json!({"path": ".", "recursive": true})),
        );
        let globbed = run(
            &registry,
            &context,
            call("glob", json!({"pattern": "**/*.json"})),
        );
        let grepped = run(
            &registry,
            &context,
            call(
                "grep",
                json!({"pattern": SECRET_TOKEN, "path": ".", "output_mode": "content"}),
            ),
        );

        // Then
        let listed = ran_text(&listed)?;
        assert!(
            !listed.contains("auth.json") && !listed.contains("auth/"),
            "{listed}"
        );
        assert!(!listed.contains("copy.json"), "{listed}");
        assert!(listed.contains("notes.md"), "{listed}");
        let globbed = ran_text(&globbed)?;
        assert!(!globbed.contains("auth"), "{globbed}");
        assert!(globbed.contains("config.json"), "{globbed}");
        let grepped = ran_text(&grepped)?;
        assert!(grepped.starts_with("No matches found"), "{grepped}");
    }
    Ok(())
}

#[test]
fn read_deny_and_ask_rules_also_hide_paths_from_scans() -> TestResult {
    // Given
    let setup = Setup::new()?;
    fs::create_dir_all(setup.workspace.join("private"))?;
    fs::write(
        setup.workspace.join("private").join("diary.md"),
        "dear diary",
    )?;
    fs::write(setup.workspace.join(".env"), "KEY=dear diary")?;
    let (registry, _) = setup.registry(Exec::Gated);
    let mut rules = super::support::Rules::deny(&["read(private/**)"]);
    rules.ask = vec!["read(.env)".to_owned()];
    rules.allow = vec!["edit(notes.md)".to_owned()];
    let context = setup.context(&Options::auto(true).rules(rules));

    // When
    let listed = run(
        &registry,
        &context,
        call("list_dir", json!({"path": ".", "recursive": true})),
    );
    let grepped = run(
        &registry,
        &context,
        call(
            "grep",
            json!({"pattern": "dear diary", "path": ".", "output_mode": "content"}),
        ),
    );

    // Then
    let listed = ran_text(&listed)?;
    assert!(
        !listed.contains("diary.md") && !listed.contains(".env"),
        "{listed}"
    );
    assert!(listed.contains("notes.md"), "{listed}");
    let grepped = ran_text(&grepped)?;
    assert!(grepped.starts_with("No matches found"), "{grepped}");
    Ok(())
}

fn ran_text(outcome: &Outcome) -> Result<String, String> {
    match outcome {
        Outcome::Ran(text) => Ok(text.clone()),
        other => Err(format!("did not run: {other:?}")),
    }
}

fn finish(failures: Vec<String>) -> TestResult {
    if failures.is_empty() {
        Ok(())
    } else {
        Err(failures.join("\n").into())
    }
}

#[test]
fn absolute_secret_paths_that_redaction_hides_are_refused_before_any_rule() -> TestResult {
    for mode in ALL_MODES {
        // Given
        let setup = Setup::workspace_is_data()?;
        let (registry, _) = setup.registry(Exec::Gated);
        let context = setup.context(
            &Options::mode(mode, true).rules(super::support::Rules::allow(&["read", "edit"])),
        );
        let auth = setup.data.join("auth.json").display().to_string();

        // When
        let write = run(
            &registry,
            &context,
            call("write_file", json!({"path": auth, "content": "x"})),
        );
        let read = run(
            &registry,
            &context,
            call("read_file", json!({"path": auth})),
        );

        // Then
        assert!(write.refusal().is_some(), "{mode:?} {write:?}");
        assert!(!read.ran(), "{mode:?} {read:?}");
        assert!(!format!("{read:?}").contains(SECRET_TOKEN));
        assert!(fs::read_to_string(setup.data.join("auth.json"))?.contains(SECRET_TOKEN));
    }
    Ok(())
}

#[test]
fn links_made_after_the_store_was_saved_again_stay_secret_in_every_mode() -> TestResult {
    let mut failures = Vec::new();
    for mode in ALL_MODES {
        // Given
        let setup = Setup::workspace_is_data()?;
        let (registry, _) = setup.registry(Exec::Gated);
        let context = setup.context(&Options::mode(mode, true));
        let saved = setup.data.join(".auth.json.tmp-9");
        fs::write(&saved, format!("{{\"token\": \"{SECRET_TOKEN}-renewed\"}}"))?;
        fs::rename(&saved, setup.data.join("auth.json"))?;
        let deep = (0..6).fold(setup.data.join("auth"), |folder, level| {
            folder.join(format!("d{level}"))
        });
        fs::create_dir_all(&deep)?;
        fs::write(deep.join("refresh.json"), SECRET_TOKEN)?;
        fs::create_dir_all(setup.workspace.join("notes"))?;
        fs::hard_link(
            setup.data.join("auth.json"),
            setup.workspace.join("notes/renewed.json"),
        )?;
        fs::hard_link(
            deep.join("refresh.json"),
            setup.workspace.join("notes/deep.txt"),
        )?;

        // When
        let reads = ["notes/renewed.json", "notes/deep.txt"].map(|path| {
            run(
                &registry,
                &context,
                call("read_file", json!({"path": path})),
            )
        });
        let grepped = run(
            &registry,
            &context,
            call(
                "grep",
                json!({"pattern": SECRET_TOKEN, "path": "notes", "output_mode": "content"}),
            ),
        );
        let listed = run(
            &registry,
            &context,
            call("list_dir", json!({"path": "notes"})),
        );

        // Then
        let asks = mode != PermissionMode::DontAsk;
        for read in &reads {
            let fine = if asks {
                read.question().is_some_and(|question| question.once_only())
            } else {
                read.refusal().is_some()
            };
            if !fine {
                failures.push(format!("{mode:?} read: {read:?}"));
            }
        }
        let grep_hidden = match &grepped {
            Outcome::Ran(text) => text.starts_with("No matches found"),
            Outcome::Refused(_) => !asks,
            Outcome::Asked(_) => false,
        };
        if !grep_hidden {
            failures.push(format!("{mode:?} grep: {grepped:?}"));
        }
        let list_hidden = match &listed {
            Outcome::Ran(text) => !text.contains("renewed") && !text.contains("deep"),
            Outcome::Refused(_) => !asks,
            Outcome::Asked(_) => false,
        };
        if !list_hidden {
            failures.push(format!("{mode:?} list: {listed:?}"));
        }
    }
    finish(failures)
}

#[test]
fn auto_mode_refuses_writes_to_the_runtime_markers_that_rules_and_the_classifier_allow(
) -> TestResult {
    // Given: allow rules for every write and a classifier that allows.
    let setup = Setup::workspace_is_data()?;
    let runtime = setup.data.join("runtime");
    fs::create_dir_all(&runtime)?;
    fs::write(runtime.join("auto-mode-setup-hint-off"), "")?;
    fs::write(runtime.join("auto-mode-default-notice"), "")?;
    let (registry, recorded) = setup.registry(Exec::Gated);
    let rules = Rules::allow(&[
        "edit",
        "exec(touch *)",
        "exec(rm *)",
        "exec(mv *)",
        "exec(echo *)",
    ]);
    let auto = setup.context(&Options::auto(true).rules(rules).verdict(Verdict::Allow));
    let writes = [
        (
            "write_file",
            json!({"path": "runtime/auto-mode-setup-hint-off", "content": "x"}),
        ),
        (
            "edit_file",
            json!({"path": "runtime/auto-mode-default-notice", "old_text": "", "new_text": "x"}),
        ),
        (
            "exec",
            json!({"command": "touch runtime/auto-mode-setup-hint-off"}),
        ),
        (
            "exec",
            json!({"command": "rm runtime/auto-mode-default-notice"}),
        ),
        (
            "exec",
            json!({"command": "echo x > runtime/auto-mode-setup-hint-off"}),
        ),
        ("exec", json!({"command": "mv runtime runtime-old"})),
        ("exec", json!({"command": "rm runtime/auto-mode-*"})),
        ("exec", json!({"command": "rm runtime/*"})),
    ];
    let default = setup.context(&Options::mode(PermissionMode::Default, true));

    // When
    let outcomes = writes
        .iter()
        .map(|(tool, arguments)| run(&registry, &auto, call(tool, arguments.clone())))
        .collect::<Vec<_>>();
    let in_default = run(&registry, &default, call(writes[0].0, writes[0].1.clone()));

    // Then
    for (outcome, (tool, arguments)) in outcomes.iter().zip(&writes) {
        assert!(
            outcome.refusal().is_some(),
            "{tool} {arguments}: {outcome:?}"
        );
    }
    assert!(recorded.launched().is_empty());
    assert!(runtime.join("auto-mode-setup-hint-off").exists());
    assert!(runtime.join("auto-mode-default-notice").exists());
    assert!(in_default.question().is_some(), "{in_default:?}");
    Ok(())
}
