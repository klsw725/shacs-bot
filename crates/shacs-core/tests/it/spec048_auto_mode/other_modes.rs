use super::support::{
    call, now_unix_ms, run, run_answered, run_report, Exec, Options, Outcome, Rules, Setup,
    TestResult, ALL_MODES, SECRET_TOKEN,
};
use serde_json::json;
use shacs_config::{RememberedPermissionEffect, RememberedPermissionMatcher};
use shacs_core::runtime::{
    session_remembered_context_digest, ApprovalDecisionKind, PermissionMode,
    SessionRememberedPermissionRule,
};

fn asks(mode: PermissionMode, interactive: bool) -> bool {
    interactive && mode != PermissionMode::DontAsk
}

fn expect_ask_or_deny(
    label: &str,
    mode: PermissionMode,
    interactive: bool,
    outcome: &Outcome,
    failures: &mut Vec<String>,
) {
    let fine = if asks(mode, interactive) {
        outcome
            .question()
            .is_some_and(|question| question.once_only())
    } else {
        outcome.refusal().is_some()
    };
    if !fine {
        failures.push(format!(
            "{label} {mode:?} interactive {interactive}: {outcome:?}"
        ));
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
fn every_mode_asks_once_for_secret_reads_and_critical_removals() -> TestResult {
    let mut failures = Vec::new();
    for mode in ALL_MODES {
        for interactive in [true, false] {
            // Given
            let setup = Setup::workspace_is_data()?;
            let (registry, recorded) = setup.registry(Exec::GatedPermissive);
            let context = setup.context(&Options::mode(mode, interactive));
            let secret = call("read_file", json!({"path": "auth.json"}));
            let removal = call(
                "exec",
                json!({"command": format!("rm -r --force {}", setup.workspace.display())}),
            );

            // When
            let read = run(&registry, &context, secret.clone());
            let remove = run(&registry, &context, removal.clone());

            // Then
            expect_ask_or_deny("secret read", mode, interactive, &read, &mut failures);
            expect_ask_or_deny(
                "critical removal",
                mode,
                interactive,
                &remove,
                &mut failures,
            );
            if asks(mode, interactive) {
                let approved =
                    run_answered(&registry, &context, secret, ApprovalDecisionKind::Approved)?;
                if !matches!(&approved, Outcome::Ran(text) if text.contains(SECRET_TOKEN)) {
                    failures.push(format!("approved secret read {mode:?}: {approved:?}"));
                }
                let removed =
                    run_answered(&registry, &context, removal, ApprovalDecisionKind::Approved)?;
                if !matches!(&removed, Outcome::Ran(text) if text == "launched") {
                    failures.push(format!("approved removal {mode:?}: {removed:?}"));
                }
            }
            let launched = recorded.launched().len();
            if launched != usize::from(asks(mode, interactive)) {
                failures.push(format!("{mode:?} launched {launched} removals"));
            }
        }
    }
    finish(failures)
}

#[test]
fn every_mode_refuses_secret_writes_deny_rules_and_remembered_denials() -> TestResult {
    let mut failures = Vec::new();
    for mode in ALL_MODES {
        for interactive in [true, false] {
            // Given
            let setup = Setup::workspace_is_data()?;
            let (registry, _) = setup.registry(Exec::Gated);
            let options = Options::mode(mode, interactive);
            let context = setup.context(&options);
            let denied = setup.context(&options.clone().rules(Rules::deny(&["exec(echo *)"])));
            let echo = call("exec", json!({"command": "echo hi"}));
            let action = run_report(&registry, &context, echo.clone())
                .permissioned_actions
                .first()
                .cloned()
                .ok_or("no action")?;
            let mut remembered = context.clone();
            remembered.permission_session_remembered_rules =
                vec![SessionRememberedPermissionRule {
                    session_key: "cli:direct".to_owned(),
                    approval_context_digest: session_remembered_context_digest(&action),
                    effect: RememberedPermissionEffect::Deny,
                    matcher: RememberedPermissionMatcher::ExecPrefix {
                        tokens: vec!["echo".to_owned()],
                    },
                    created_unix_ms: now_unix_ms(),
                    legacy_imported: false,
                }];

            // When
            let outcomes = [
                (
                    "secret write",
                    run(
                        &registry,
                        &context,
                        call("write_file", json!({"path": "auth.json", "content": "{}"})),
                    ),
                ),
                (
                    "secret edit",
                    run(
                        &registry,
                        &context,
                        call(
                            "edit_file",
                            json!({"path": "auth.json", "old_text": "token", "new_text": "x"}),
                        ),
                    ),
                ),
                ("deny rule", run(&registry, &denied, echo.clone())),
                ("remembered denial", run(&registry, &remembered, echo)),
            ];

            // Then
            for (label, outcome) in outcomes {
                if outcome.refusal().is_none() {
                    failures.push(format!(
                        "{label} {mode:?} interactive {interactive}: {outcome:?}"
                    ));
                }
            }
            if !std::fs::read_to_string(setup.data.join("auth.json"))?.contains(SECRET_TOKEN) {
                failures.push(format!("{mode:?} changed auth.json"));
            }
        }
    }
    finish(failures)
}

#[test]
fn an_ask_rule_turns_an_allowed_call_into_a_question_in_every_mode() -> TestResult {
    let mut failures = Vec::new();
    for mode in ALL_MODES {
        for interactive in [true, false] {
            // Given
            let setup = Setup::new()?;
            let (registry, _) = setup.registry(Exec::Gated);
            let read = call("read_file", json!({"path": "notes.md"}));
            let plain = setup.context(&Options::mode(mode, interactive));
            let ruled = setup
                .context(&Options::mode(mode, interactive).rules(Rules::ask(&["read(notes.md)"])));

            // When
            let base = run(&registry, &plain, read.clone());
            let asked = run(&registry, &ruled, read);

            // Then
            match base {
                Outcome::Ran(_) => {
                    expect_ask_or_deny("ask rule", mode, interactive, &asked, &mut failures)
                }
                other => {
                    if other.ran() != asked.ran()
                        || other.question().is_some() != asked.question().is_some()
                    {
                        failures.push(format!("ask rule changed {mode:?}: {other:?} -> {asked:?}"));
                    }
                }
            }
        }
    }
    finish(failures)
}

#[test]
fn bypass_permissions_still_hits_deny_ask_secret_and_critical_rules() -> TestResult {
    // Given
    let setup = Setup::workspace_is_data()?;
    let (registry, recorded) = setup.registry(Exec::GatedPermissive);
    let bypass = |rules: Rules| {
        setup.context(&Options::mode(PermissionMode::BypassPermissions, true).rules(rules))
    };
    let echo = || call("exec", json!({"command": "echo hi"}));

    // When
    let allowed = run(&registry, &bypass(Rules::allow(&["exec(echo *)"])), echo());
    let unruled = run(&registry, &bypass(Rules::default()), echo());
    let asked = run(&registry, &bypass(Rules::ask(&["exec(echo *)"])), echo());
    let denied = run(&registry, &bypass(Rules::deny(&["exec(echo *)"])), echo());
    let secret = run(
        &registry,
        &bypass(Rules::allow(&["read"])),
        call("read_file", json!({"path": "auth.json"})),
    );
    let removal = run(
        &registry,
        &bypass(Rules::allow(&["exec"])),
        call(
            "exec",
            json!({"command": format!("rmdir {}", setup.home.display())}),
        ),
    );

    // Then
    assert!(allowed.ran() && unruled.ran(), "{allowed:?} {unruled:?}");
    assert!(asked
        .question()
        .is_some_and(|question| question.once_only()));
    assert!(denied.refusal().is_some());
    assert!(secret
        .question()
        .is_some_and(|question| question.once_only()));
    assert!(removal
        .question()
        .is_some_and(|question| question.once_only()));
    assert_eq!(recorded.launched(), ["echo hi", "echo hi"]);
    Ok(())
}
