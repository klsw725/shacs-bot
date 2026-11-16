//! Spec 053 PRD 020 acceptance: file tools with paths outside the workspace.

use crate::spec048_auto_mode::support::{
    call, now_unix_ms, run, run_answered, run_report, Exec, Options, Outcome, Setup, TestResult,
    Verdict, BLOCKED,
};
use serde_json::{json, Value};
use shacs_config::{RememberedPermissionEffect, RememberedPermissionMatcher};
use shacs_core::runtime::{
    safe_remembered_permission_matcher, session_remembered_context_digest, ApprovalDecisionKind,
    PermissionMode, SessionRememberedPermissionRule,
};
use shacs_core::tools::{
    EditFileTool, GlobTool, GrepTool, JsonMap, ListDirTool, NotebookEditTool, PathContext,
    ReadFileTool, Tool, ToolCallExecutionContext, WriteFileTool,
};
use std::fs;
use std::os::unix::fs::symlink;

const UNATTENDED_OUTSIDE: &str = "outside the workspace in an unattended run";
const OUTSIDE_WHY: &str = "this call reaches outside the workspace";

fn params(value: Value) -> JsonMap {
    value.as_object().cloned().unwrap_or_default()
}

/// The file tools as production registers them with `restrictToWorkspace`
/// off.
fn open_context(setup: &Setup) -> PathContext {
    let mut context = setup.path_context();
    context.outside_by_decision = true;
    context
}

#[test]
fn open_file_tools_name_outside_targets_instead_of_refusing() -> TestResult {
    // Given
    let setup = Setup::new()?;
    let outside = setup.home.join("x.txt");
    fs::write(&outside, "outside")?;
    symlink(&outside, setup.workspace.join("link.txt"))?;
    let read = ReadFileTool::new(open_context(&setup));
    let shown = outside.display().to_string();

    // When / Then: an absolute path, `..`, and a symlink out are targets.
    for path in [shown.as_str(), "../home/x.txt", "link.txt"] {
        let call = params(json!({"path": path}));
        assert_eq!(read.preflight_refusal(&call), None, "{path}");
        assert_eq!(
            read.outside_workspace_targets(&call),
            vec![shown.clone()],
            "{path}"
        );
    }
    let media = setup.media.join("photo.txt").display().to_string();
    for inside in ["notes.md", media.as_str()] {
        let call = params(json!({"path": inside}));
        assert!(read.outside_workspace_targets(&call).is_empty(), "{inside}");
    }
    let glob = GlobTool::new(open_context(&setup));
    assert!(glob
        .outside_workspace_targets(&params(json!({"pattern": "*.md"})))
        .is_empty());
    let written = setup.home.join("new.txt");
    let write = WriteFileTool::new(open_context(&setup));
    let new_file = params(json!({"path": written, "content": "x"}));
    assert_eq!(write.preflight_refusal(&new_file), None);
    assert_eq!(
        write.outside_workspace_targets(&new_file),
        vec![written.display().to_string()]
    );
    Ok(())
}

#[test]
fn closed_file_tools_refuse_outside_paths_as_before() -> TestResult {
    // Given: the default, as `restrictToWorkspace` and child runs use.
    let setup = Setup::new()?;
    let outside = setup.home.join("x.txt");
    fs::write(&outside, "outside")?;
    let read = ReadFileTool::new(setup.path_context());
    let call = params(json!({"path": outside}));

    // When
    let refusal = read.preflight_refusal(&call);

    // Then
    assert!(
        refusal
            .as_deref()
            .is_some_and(|text| text.contains("is outside allowed directory")),
        "{refusal:?}"
    );
    assert!(read.outside_workspace_targets(&call).is_empty());
    Ok(())
}

#[test]
fn the_remaining_file_tool_refusals_stay() -> TestResult {
    // Given
    let setup = Setup::new()?;
    symlink(&setup.home, setup.workspace.join("linked"))?;
    fs::create_dir_all(setup.workspace.join(".shacs-methods"))?;
    fs::write(setup.home.join("plain.txt"), "plain")?;
    symlink(setup.root.join("data"), setup.home.join("escape"))?;
    let write = WriteFileTool::new(open_context(&setup));
    let list = ListDirTool::new(open_context(&setup));

    // When
    let through_symlink =
        write.preflight_refusal(&params(json!({"path": "linked/a.txt", "content": "x"})));
    let into_store = write.preflight_refusal(&params(
        json!({"path": ".shacs-methods/m.md", "content": "x"}),
    ));
    let listing = list.execute_with_context(
        params(json!({"path": setup.home})),
        &ToolCallExecutionContext::default().with_approved_outside(vec![setup.home.clone()]),
    );

    // Then
    assert!(through_symlink.is_some(), "a write through a symlink");
    assert!(
        into_store
            .as_deref()
            .is_some_and(|text| text.contains("learned-method store")),
        "{into_store:?}"
    );
    let listing = listing.into_text();
    assert!(listing.contains("plain.txt"), "{listing}");
    assert!(
        !listing.contains("escape"),
        "symlinks are not listed: {listing}"
    );
    Ok(())
}

#[test]
fn auto_mode_sends_outside_reads_and_writes_to_the_classifier() -> TestResult {
    // Given
    let setup = Setup::new()?;
    let outside = setup.home.join("x.txt");
    fs::write(&outside, "outside text")?;
    let written = setup.home.join("new.txt");
    let (registry, _) = setup.registry(Exec::Open);
    let allowed = setup.context(&Options::auto(true).verdict(Verdict::Allow));
    let blocked = setup.context(&Options::auto(true).verdict(Verdict::Block));
    let read = call("read_file", json!({"path": outside}));
    let write = call("write_file", json!({"path": written, "content": "summary"}));

    // When
    let read_allowed = run(&registry, &allowed, read.clone());
    let read_blocked = run(&registry, &blocked, read);
    let write_blocked = run(&registry, &blocked, write.clone());
    let write_allowed = run(&registry, &allowed, write);

    // Then
    assert!(
        matches!(&read_allowed, Outcome::Ran(text) if text.contains("outside text")),
        "{read_allowed:?}"
    );
    for blocked in [&read_blocked, &write_blocked] {
        assert!(
            blocked
                .refusal()
                .is_some_and(|text| text.starts_with(BLOCKED)),
            "{blocked:?}"
        );
    }
    assert!(write_allowed.ran(), "{write_allowed:?}");
    assert_eq!(fs::read_to_string(&written)?, "summary");
    Ok(())
}

#[test]
fn other_modes_ask_for_outside_access_and_run_only_the_approved_call() -> TestResult {
    // Given
    let setup = Setup::new()?;
    let outside = setup.home.join("x.txt");
    fs::write(&outside, "outside text")?;
    let (registry, _) = setup.registry(Exec::Open);
    let read = call("read_file", json!({"path": outside}));
    let write = call(
        "write_file",
        json!({"path": setup.home.join("new.txt"), "content": "x"}),
    );
    let default = setup.context(&Options::mode(PermissionMode::Default, true));
    let edits = setup.context(&Options::mode(PermissionMode::AcceptEdits, true));
    let bypass = setup.context(&Options::mode(PermissionMode::BypassPermissions, true));

    // When
    let read_asked = run(&registry, &default, read.clone());
    let read_approved = run_answered(&registry, &default, read, ApprovalDecisionKind::Approved)?;
    let write_asked = run(&registry, &edits, write.clone());
    let write_bypassed = run(&registry, &bypass, write);

    // Then
    for asked in [&read_asked, &write_asked] {
        let question = asked
            .question()
            .ok_or_else(|| format!("not asked: {asked:?}"))?;
        assert!(question.text.contains(OUTSIDE_WHY), "{}", question.text);
    }
    assert!(
        matches!(&read_approved, Outcome::Ran(text) if text.contains("outside text")),
        "{read_approved:?}"
    );
    assert!(write_bypassed.ran(), "{write_bypassed:?}");
    assert_eq!(fs::read_to_string(setup.home.join("new.txt"))?, "x");
    Ok(())
}

#[test]
fn secret_files_outside_the_workspace_keep_the_secret_rules() -> TestResult {
    // Given: the data folder is outside the workspace.
    let setup = Setup::new()?;
    let env_file = setup.data.join(".env.json");
    fs::write(&env_file, r#"{"SHACS_X": "value"}"#)?;
    let (registry, _) = setup.registry(Exec::Open);
    let auto = setup.context(&Options::auto(true).verdict(Verdict::Allow));
    let default = setup.context(&Options::mode(PermissionMode::Default, true));

    // When
    let auto_read = run(
        &registry,
        &auto,
        call("read_file", json!({"path": env_file})),
    );
    let default_read = run(
        &registry,
        &default,
        call("read_file", json!({"path": env_file})),
    );
    let write = run(
        &registry,
        &auto,
        call("write_file", json!({"path": env_file, "content": "{}"})),
    );
    let listing = run(
        &registry,
        &auto,
        call("list_dir", json!({"path": setup.data})),
    );

    // Then
    for read in [&auto_read, &default_read] {
        let question = read
            .question()
            .ok_or_else(|| format!("not asked: {read:?}"))?;
        assert!(question.once_only(), "{}", question.text);
    }
    assert!(write.refusal().is_some(), "{write:?}");
    let Outcome::Ran(listing) = listing else {
        return Err(format!("not listed: {listing:?}").into());
    };
    assert!(listing.contains("config.json"), "{listing}");
    for hidden in [".env.json", "auth.json", "📁 auth"] {
        assert!(!listing.contains(hidden), "{hidden} in {listing}");
    }
    Ok(())
}

#[test]
fn unattended_runs_refuse_outside_file_access_except_in_bypass() -> TestResult {
    // Given
    let setup = Setup::new()?;
    let outside = setup.home.join("x.txt");
    fs::write(&outside, "outside text")?;
    let (registry, _) = setup.registry(Exec::Open);
    let read = call("read_file", json!({"path": outside}));
    let auto = setup.context(&Options::auto(false).verdict(Verdict::Allow));
    let bypass = setup.context(&Options::mode(PermissionMode::BypassPermissions, false));

    // When
    let auto_read = run(&registry, &auto, read.clone());
    let bypass_read = run(&registry, &bypass, read);

    // Then
    assert!(
        auto_read
            .refusal()
            .is_some_and(|text| text.contains(UNATTENDED_OUTSIDE)),
        "{auto_read:?}"
    );
    assert!(bypass_read.ran(), "{bypass_read:?}");
    Ok(())
}

#[test]
fn plan_asks_for_outside_reads_and_still_blocks_outside_writes() -> TestResult {
    // Given
    let setup = Setup::new()?;
    let (registry, _) = setup.registry(Exec::Open);
    let plan = setup.context(&Options::mode(PermissionMode::Plan, true));

    // When
    let read = run(
        &registry,
        &plan,
        call("read_file", json!({"path": setup.home.join("x.txt")})),
    );
    let write = run(
        &registry,
        &plan,
        call(
            "write_file",
            json!({"path": setup.home.join("new.txt"), "content": "x"}),
        ),
    );

    // Then
    let question = read
        .question()
        .ok_or_else(|| format!("not asked: {read:?}"))?;
    assert!(question.text.contains(OUTSIDE_WHY), "{}", question.text);
    assert!(write.refusal().is_some(), "{write:?}");
    assert!(!setup.home.join("new.txt").exists());
    Ok(())
}

#[test]
fn a_remembered_outside_read_is_not_asked_again_even_in_dont_ask() -> TestResult {
    // Given: the question's own remembered matcher, kept for the session.
    let setup = Setup::new()?;
    let (registry, _) = setup.registry(Exec::Open);
    let read = call("read_file", json!({"path": setup.home.join("x.txt")}));
    let default = setup.context(&Options::mode(PermissionMode::Default, true));
    let dont_ask = setup.context(&Options::mode(PermissionMode::DontAsk, true));
    // A remembered rule holds for the mode it was made in.
    let remembering = |context: &shacs_core::runtime::ToolExecutionContext| -> Result<_, Box<dyn std::error::Error>> {
        let report = run_report(&registry, context, read.clone());
        let action = report.permissioned_actions.first().ok_or("no action")?;
        let remembered = safe_remembered_permission_matcher(action, &setup.workspace)?;
        assert!(
            matches!(
                remembered.matcher,
                RememberedPermissionMatcher::ExactAction { .. }
            ),
            "outside targets are remembered as the exact action: {:?}",
            remembered.matcher
        );
        let mut remembering = context.clone();
        remembering.permission_session_remembered_rules = vec![SessionRememberedPermissionRule {
            session_key: "cli:direct".to_owned(),
            approval_context_digest: session_remembered_context_digest(action),
            effect: RememberedPermissionEffect::Allow,
            matcher: remembered.matcher,
            created_unix_ms: now_unix_ms(),
            legacy_imported: false,
        }];
        Ok(remembering)
    };
    let default_remembered = remembering(&default)?;
    let dont_ask_remembered = remembering(&dont_ask)?;

    // When
    let dont_ask_plain = run(&registry, &dont_ask, read.clone());
    let default_again = run(&registry, &default_remembered, read.clone());
    let dont_ask_again = run(&registry, &dont_ask_remembered, read);

    // Then
    assert!(dont_ask_plain.refusal().is_some(), "{dont_ask_plain:?}");
    for outcome in [&default_again, &dont_ask_again] {
        assert!(outcome.ran(), "{outcome:?}");
    }
    Ok(())
}

/// Runs `tool` as the runtime does after a decision that allowed `approved`.
fn execute(tool: &dyn Tool, call: Value, approved: &[std::path::PathBuf]) -> String {
    tool.execute_with_context(
        params(call),
        &ToolCallExecutionContext::default().with_approved_outside(approved.to_vec()),
    )
    .into_text()
}

#[test]
fn a_link_followed_by_dotdot_is_judged_where_the_tool_reads() -> TestResult {
    // Given: `link/../x.txt` reaches `home/x.txt` through the link, not
    // `ws/x.txt` as the letters suggest.
    let setup = Setup::new()?;
    fs::create_dir_all(setup.home.join("sub"))?;
    symlink(setup.home.join("sub"), setup.workspace.join("link"))?;
    fs::write(setup.workspace.join("x.txt"), "workspace file")?;
    let read = ReadFileTool::new(open_context(&setup));
    let call = json!({"path": "link/../x.txt"});

    // When
    let targets = read.outside_workspace_targets(&params(call.clone()));
    let unapproved = execute(&read, call.clone(), &[]);
    let approved = execute(&read, call, &[setup.home.join("x.txt")]);

    // Then
    assert_eq!(
        targets,
        vec![setup.home.join("x.txt").display().to_string()]
    );
    assert!(
        unapproved.contains("is outside allowed directory"),
        "{unapproved}"
    );
    assert!(approved.contains("home file"), "{approved}");
    Ok(())
}

#[test]
fn a_file_swapped_for_an_outside_link_after_the_decision_is_refused() -> TestResult {
    // Given: decided while `notes.md` was inside, so nothing was approved.
    let setup = Setup::new()?;
    let read = ReadFileTool::new(open_context(&setup));
    let call = json!({"path": "notes.md"});
    let approved: Vec<std::path::PathBuf> = read
        .outside_workspace_targets(&params(call.clone()))
        .into_iter()
        .map(std::path::PathBuf::from)
        .collect();
    assert!(approved.is_empty());

    // When: the file becomes a link out before the call runs.
    fs::remove_file(setup.workspace.join("notes.md"))?;
    symlink(setup.home.join("x.txt"), setup.workspace.join("notes.md"))?;
    let result = execute(&read, call, &approved);

    // Then
    assert!(result.contains("is outside allowed directory"), "{result}");
    assert!(!result.contains("home file"), "{result}");
    Ok(())
}

#[test]
fn every_file_tool_names_its_outside_target_and_runs_only_when_approved() -> TestResult {
    // Given
    let setup = Setup::new()?;
    let context = open_context(&setup);
    let home = setup.home.clone();
    let notebook = home.join("n.ipynb");
    fs::write(
        &notebook,
        r#"{"cells": [{"cell_type": "code", "source": ["1"], "metadata": {}, "outputs": [], "execution_count": null}], "metadata": {}, "nbformat": 4, "nbformat_minor": 5}"#,
    )?;
    let hashed = execute(
        &ReadFileTool::new(context.clone()),
        json!({"path": home.join("x.txt"), "hashlines": true}),
        &[home.join("x.txt")],
    );
    let line_tag = hashed
        .split('|')
        .next()
        .and_then(|tag| tag.split_whitespace().last())
        .ok_or("no line tag")?
        .to_owned();
    let edit_call = json!({
        "path": home.join("x.txt"), "op": "replace_line", "line_tag": line_tag, "text": "edited",
    });
    let cases: Vec<(Box<dyn Tool>, Value, std::path::PathBuf)> = vec![
        (
            Box::new(ReadFileTool::new(context.clone())),
            json!({"path": home.join("x.txt")}),
            home.join("x.txt"),
        ),
        (
            Box::new(WriteFileTool::new(context.clone())),
            json!({"path": home.join("w.txt"), "content": "written"}),
            home.join("w.txt"),
        ),
        (
            Box::new(EditFileTool::new(context.clone())),
            edit_call,
            home.join("x.txt"),
        ),
        (
            Box::new(ListDirTool::new(context.clone())),
            json!({"path": home}),
            home.clone(),
        ),
        (
            Box::new(GlobTool::new(context.clone())),
            json!({"pattern": "*.txt", "path": home}),
            home.clone(),
        ),
        (
            Box::new(GrepTool::new(context.clone())),
            json!({"pattern": "file", "path": home}),
            home.clone(),
        ),
        (
            Box::new(NotebookEditTool::new(context.clone())),
            json!({"path": notebook, "cell_index": 0, "new_source": "2"}),
            notebook.clone(),
        ),
    ];

    for (tool, call, target) in cases {
        // When
        let targets = tool.outside_workspace_targets(&params(call.clone()));
        let refused = execute(tool.as_ref(), call.clone(), &[]);
        let ran = execute(tool.as_ref(), call, std::slice::from_ref(&target));

        // Then
        assert_eq!(
            targets,
            vec![target.display().to_string()],
            "{}",
            tool.name()
        );
        assert!(
            refused.contains("outside allowed directory"),
            "{}: {refused}",
            tool.name()
        );
        assert!(
            !ran.contains("outside allowed directory") && !ran.starts_with("Error"),
            "{}: {ran}",
            tool.name()
        );
    }
    assert_eq!(fs::read_to_string(home.join("w.txt"))?, "written");
    assert_eq!(fs::read_to_string(home.join("x.txt"))?, "edited");
    assert!(fs::read_to_string(&notebook)?.contains('2'));
    Ok(())
}

#[test]
fn a_secret_file_reached_through_a_link_and_dotdot_is_still_asked_every_time() -> TestResult {
    // Given: `link` leads into the data folder's auth store, so
    // `link/../.env.json` is the data folder's `.env.json`.
    let setup = Setup::new()?;
    fs::write(setup.data.join(".env.json"), r#"{"SHACS_X": "value"}"#)?;
    symlink(setup.data.join("auth"), setup.workspace.join("link"))?;
    let (registry, _) = setup.registry(Exec::Open);
    let auto = setup.context(&Options::auto(true).verdict(Verdict::Allow));
    let bypass = setup.context(&Options::mode(PermissionMode::BypassPermissions, true));

    for path in ["link/../.env.json", "link/../auth.json"] {
        for context in [&auto, &bypass] {
            // When
            let read = run(&registry, context, call("read_file", json!({"path": path})));

            // Then
            let question = read
                .question()
                .ok_or_else(|| format!("{path} not asked: {read:?}"))?;
            assert!(question.once_only(), "{path}: {}", question.text);
        }
    }
    Ok(())
}
