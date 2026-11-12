//! Spec 050 PRD 020 acceptance 10 and 11: the login profile is a secret
//! file in every mode, and no call changes the browser's lock files or the
//! folder that holds them.

use crate::spec048_auto_mode::support::{
    call, now_unix_ms, run, run_report, Exec, Options, Outcome, Rules, Setup, TestResult, Verdict,
};
use serde_json::{json, Value};
use shacs_config::{RememberedPermissionEffect, RememberedPermissionMatcher};
use shacs_core::runtime::{
    session_remembered_context_digest, PermissionMode, SessionRememberedPermissionRule,
};
use std::fs;
use std::os::unix::fs::symlink;

const MODES: [PermissionMode; 4] = [
    PermissionMode::Default,
    PermissionMode::AcceptEdits,
    PermissionMode::Auto,
    PermissionMode::BypassPermissions,
];
const COOKIE: &str = "spec050-cookie-value";
const LOCKS: [&str; 3] = ["active.lock", "forget.lock", "profile.lock"];
const LOCK_CODE: &str = "browser_lock_write";
const LOCK_TEXT: &str = "브라우저 lock 파일은 바꿀 수 없습니다.";

/// A data folder that is also the workspace, with a login profile, the three
/// lock files, a hard link to a profile file and a symlink to `browser/`.
fn profile_setup() -> Result<Setup, Box<dyn std::error::Error>> {
    let setup = Setup::workspace_is_data()?;
    let browser = setup.data.join("browser");
    let profile = browser.join("profile").join("Default");
    fs::create_dir_all(&profile)?;
    fs::write(profile.join("Cookies"), COOKIE)?;
    for lock in LOCKS {
        fs::write(browser.join(lock), "")?;
    }
    fs::hard_link(profile.join("Cookies"), setup.workspace.join("copy.db"))?;
    symlink(&browser, setup.workspace.join("shortcut"))?;
    Ok(setup)
}

fn finish(failures: Vec<String>) -> TestResult {
    if failures.is_empty() {
        Ok(())
    } else {
        Err(failures.join("\n").into())
    }
}

fn ran_text(outcome: &Outcome) -> Result<String, String> {
    match outcome {
        Outcome::Ran(text) => Ok(text.clone()),
        other => Err(format!("did not run: {other:?}")),
    }
}

fn every_write_rule() -> Rules {
    Rules::allow(&[
        "edit",
        "exec(rm *)",
        "exec(mv *)",
        "exec(ln *)",
        "exec(touch *)",
        "exec(echo *)",
        "exec(python3 *)",
    ])
}

#[test]
fn login_profile_is_secret_in_every_mode() -> TestResult {
    let mut failures = Vec::new();
    for mode in MODES {
        // Given
        let setup = profile_setup()?;
        let data = setup.data.display().to_string();
        let (registry, recorded) = setup.registry(Exec::Gated);
        let context = setup.context(&Options::mode(mode, true));
        let allowed = setup.context(
            &Options::mode(mode, true)
                .rules(every_write_rule())
                .verdict(Verdict::Allow),
        );

        // When: scans
        let listed = ran_text(&run(
            &registry,
            &context,
            call("list_dir", json!({"path": ".", "recursive": true})),
        ))?;
        let globbed = ran_text(&run(
            &registry,
            &context,
            call("glob", json!({"pattern": "**/*"})),
        ))?;
        let grepped = ran_text(&run(
            &registry,
            &context,
            call(
                "grep",
                json!({"pattern": COOKIE, "path": ".", "output_mode": "content"}),
            ),
        ))?;

        // Then: the profile is left out, a hard link to it too; the lock
        // files are listed.
        if listed.contains("Cookies") || listed.contains("copy.db") {
            failures.push(format!("{mode:?} list shows the profile: {listed}"));
        }
        for lock in LOCKS {
            if !listed.contains(lock) {
                failures.push(format!("{mode:?} list hides {lock}: {listed}"));
            }
        }
        if globbed.contains("Cookies") || globbed.contains("copy.db") {
            failures.push(format!("{mode:?} glob shows the profile: {globbed}"));
        }
        if !grepped.starts_with("No matches found") {
            failures.push(format!("{mode:?} grep reads the profile: {grepped}"));
        }

        // When: reads
        let reads = [
            (
                "read_file",
                "read_file",
                json!({"path": "browser/profile/Default/Cookies"}),
            ),
            (
                "cat relative",
                "exec",
                json!({"command": "cat browser/profile/Default/Cookies"}),
            ),
            (
                "cat absolute",
                "exec",
                json!({"command": format!("cat {data}/browser/profile/Default/Cookies")}),
            ),
            ("hard link read", "read_file", json!({"path": "copy.db"})),
            ("hard link cat", "exec", json!({"command": "cat copy.db"})),
        ];
        for (label, tool, arguments) in reads {
            for (rules, context) in [("no rules", &context), ("allow rules", &allowed)] {
                let outcome = run(&registry, context, call(tool, arguments.clone()));
                let asked = outcome.question().is_some_and(|question| {
                    question.once_only() && question.text.contains("secret file")
                });
                if !asked {
                    failures.push(format!("{label} {mode:?} {rules}: {outcome:?}"));
                }
            }
        }

        // When: writes and removals
        let writes = [
            (
                "write_file",
                "write_file",
                json!({"path": "browser/profile/Default/Cookies", "content": "x"}),
            ),
            (
                "edit_file",
                "edit_file",
                json!({"path": "browser/profile/Default/Cookies", "op": "delete_line", "line_tag": "L1#00000000"}),
            ),
            (
                "rm",
                "exec",
                json!({"command": "rm browser/profile/Default/Cookies"}),
            ),
            (
                "rm folder",
                "exec",
                json!({"command": "rm -r browser/profile"}),
            ),
            (
                "mv",
                "exec",
                json!({"command": format!("mv {data}/browser/profile {data}/kept")}),
            ),
            (
                "hard link write",
                "write_file",
                json!({"path": "copy.db", "content": "x"}),
            ),
        ];
        for (label, tool, arguments) in writes {
            for (rules, context) in [("no rules", &context), ("allow rules", &allowed)] {
                let outcome = run(&registry, context, call(tool, arguments.clone()));
                let refused = outcome
                    .refusal()
                    .is_some_and(|text| text.contains("SecretFileWrite"));
                if !refused {
                    failures.push(format!("{label} {mode:?} {rules}: {outcome:?}"));
                }
            }
        }

        // Then: nothing ran, and the lock files are read without a question.
        if !recorded.launched().is_empty() {
            failures.push(format!("{mode:?} launched {:?}", recorded.launched()));
        }
        let cookies = setup.data.join("browser/profile/Default/Cookies");
        if fs::read_to_string(&cookies)? != COOKIE {
            failures.push(format!("{mode:?} changed the profile"));
        }
        for lock in LOCKS {
            let outcome = run(
                &registry,
                &context,
                call("read_file", json!({"path": format!("browser/{lock}")})),
            );
            if !outcome.ran() {
                failures.push(format!("{mode:?} read of {lock}: {outcome:?}"));
            }
        }
    }
    finish(failures)
}

fn lock_writes(data: &str) -> Vec<(&'static str, &'static str, Value)> {
    vec![
        (
            "write_file",
            "write_file",
            json!({"path": "browser/active.lock", "content": "x"}),
        ),
        (
            "edit_file",
            "edit_file",
            json!({"path": "browser/forget.lock", "op": "insert_after", "line_tag": "L1#00000000", "text": "x"}),
        ),
        (
            "write_file absolute",
            "write_file",
            json!({"path": format!("{data}/browser/profile.lock"), "content": "x"}),
        ),
        ("rm", "exec", json!({"command": "rm browser/profile.lock"})),
        (
            "mv away",
            "exec",
            json!({"command": "mv browser/active.lock moved.lock"}),
        ),
        (
            "mv over",
            "exec",
            json!({"command": "mv notes.md browser/forget.lock"}),
        ),
        (
            "ln -sf",
            "exec",
            json!({"command": "ln -sf notes.md browser/active.lock"}),
        ),
        (
            "touch",
            "exec",
            json!({"command": "touch browser/profile.lock"}),
        ),
        (
            "redirect",
            "exec",
            json!({"command": "echo x > browser/active.lock"}),
        ),
        (
            "glob rm",
            "exec",
            json!({"command": format!("rm {data}/browser/*.lock")}),
        ),
        (
            "python text absolute",
            "exec",
            json!({"command": format!("python3 -c \"open('{data}/browser/active.lock', 'w')\"")}),
        ),
        (
            "python text relative",
            "exec",
            json!({"command": "python3 -c \"open('browser/forget.lock', 'w')\""}),
        ),
        (
            "mv folder",
            "exec",
            json!({"command": format!("mv {data}/browser {data}/browser-old")}),
        ),
        (
            "rm folder",
            "exec",
            json!({"command": format!("rm -r {data}/browser")}),
        ),
        (
            "ln folder",
            "exec",
            json!({"command": format!("ln -sfn {data}/media {data}/browser")}),
        ),
        (
            "rm through symlink",
            "exec",
            json!({"command": "rm shortcut/active.lock"}),
        ),
        (
            "write through symlink",
            "write_file",
            json!({"path": "shortcut/forget.lock", "content": "x"}),
        ),
    ]
}

fn refused_as_lock_write(outcome: &Outcome) -> bool {
    outcome
        .refusal()
        .is_some_and(|text| text.contains(LOCK_CODE) && text.contains(LOCK_TEXT))
}

#[test]
fn browser_lock_paths_cannot_be_changed_in_any_mode() -> TestResult {
    let mut failures = Vec::new();
    for mode in MODES {
        for present in [true, false] {
            // Given: allow rules for every write, a classifier that allows
            // and a remembered allow for `rm`.
            let setup = profile_setup()?;
            if !present {
                for lock in LOCKS {
                    fs::remove_file(setup.data.join("browser").join(lock))?;
                }
            }
            let data = setup.data.display().to_string();
            let (registry, recorded) = setup.registry(Exec::Gated);
            let plain = setup.context(&Options::mode(mode, true));
            let allowed = setup.context(
                &Options::mode(mode, true)
                    .rules(every_write_rule())
                    .verdict(Verdict::Allow),
            );
            let removal = call("exec", json!({"command": "rm browser/profile.lock"}));
            let action = run_report(&registry, &plain, removal.clone())
                .permissioned_actions
                .first()
                .cloned()
                .ok_or("no action for the removal")?;
            let mut remembered = plain.clone();
            remembered.permission_session_remembered_rules =
                vec![SessionRememberedPermissionRule {
                    session_key: "cli:direct".to_owned(),
                    approval_context_digest: session_remembered_context_digest(&action),
                    effect: RememberedPermissionEffect::Allow,
                    matcher: RememberedPermissionMatcher::ExecPrefix {
                        tokens: vec!["rm".to_owned()],
                    },
                    created_unix_ms: now_unix_ms(),
                    legacy_imported: false,
                }];

            // When / Then: every write is refused for the lock paths.
            for (label, tool, arguments) in lock_writes(&data) {
                for (rules, context) in [("no rules", &plain), ("allow rules", &allowed)] {
                    let outcome = run(&registry, context, call(tool, arguments.clone()));
                    if !refused_as_lock_write(&outcome) {
                        failures.push(format!(
                            "{label} {mode:?} present={present} {rules}: {outcome:?}"
                        ));
                    }
                }
            }
            let outcome = run(&registry, &remembered, removal);
            if !refused_as_lock_write(&outcome) {
                failures.push(format!(
                    "remembered allow {mode:?} present={present}: {outcome:?}"
                ));
            }
            if !recorded.launched().is_empty() {
                failures.push(format!(
                    "{mode:?} present={present} launched {:?}",
                    recorded.launched()
                ));
            }
            for lock in LOCKS {
                if setup.data.join("browser").join(lock).exists() != present {
                    failures.push(format!("{mode:?} present={present} changed {lock}"));
                }
            }

            // Then: reading and listing are not refused for the lock paths,
            // and neither is a file of the same name outside `browser/`.
            let free = [
                ("cat", "exec", json!({"command": "cat browser/active.lock"})),
                ("ls", "exec", json!({"command": "ls browser"})),
                (
                    "same name elsewhere",
                    "write_file",
                    json!({"path": "active.lock", "content": "x"}),
                ),
                (
                    "same name elsewhere rm",
                    "exec",
                    json!({"command": "rm active.lock"}),
                ),
            ];
            for (label, tool, arguments) in free {
                let outcome = run(&registry, &allowed, call(tool, arguments));
                let lock_refusal = outcome
                    .refusal()
                    .is_some_and(|text| text.contains(LOCK_CODE) || text.contains(LOCK_TEXT));
                if lock_refusal {
                    failures.push(format!("{label} {mode:?} present={present}: {outcome:?}"));
                }
            }
        }
    }
    finish(failures)
}

/// The markers only the user changes are judged as before: refused in auto
/// mode with their own reason, asked about elsewhere.
#[test]
fn user_only_marker_judgement_is_unchanged() -> TestResult {
    // Given
    let setup = Setup::workspace_is_data()?;
    let runtime = setup.data.join("runtime");
    fs::create_dir_all(&runtime)?;
    fs::write(runtime.join("auto-mode-setup-hint-off"), "")?;
    let (registry, recorded) = setup.registry(Exec::Gated);
    let auto = setup.context(
        &Options::auto(true)
            .rules(every_write_rule())
            .verdict(Verdict::Allow),
    );
    let default = setup.context(&Options::mode(PermissionMode::Default, true));
    let write = json!({"path": "runtime/auto-mode-setup-hint-off", "content": "x"});

    // When
    let written = run(&registry, &auto, call("write_file", write.clone()));
    let globbed = run(
        &registry,
        &auto,
        call("exec", json!({"command": "rm runtime/*"})),
    );
    let in_default = run(&registry, &default, call("write_file", write));

    // Then
    for outcome in [&written, &globbed] {
        let refusal = outcome
            .refusal()
            .ok_or(format!("not refused: {outcome:?}"))?;
        assert!(
            refusal.contains("UserOnlyFileWrite") && !refusal.contains(LOCK_CODE),
            "the marker reason is kept: {refusal}"
        );
    }
    assert!(
        in_default.question().is_some(),
        "default mode still asks: {in_default:?}"
    );
    assert!(
        recorded.launched().is_empty(),
        "nothing ran: {:?}",
        recorded.launched()
    );
    Ok(())
}
