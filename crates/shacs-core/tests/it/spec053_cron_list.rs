//! Spec 053 PRD 030 acceptance: cron jobs' approved permission lists.

use crate::spec048_auto_mode::support::{
    call, run, run_answered, Exec, Options, Outcome, Recorded, Rules, Setup, TestResult, Verdict,
};
use serde_json::{json, Value};
use shacs_core::runtime::{
    ApprovalDecisionKind, AutomationRunSource, PermissionMode, RuntimeToolExecutor,
    ToolExecutionContext,
};
use shacs_core::tools::{CronTool, JsonMap, Tool, ToolRegistry, ToolResult};
use shacs_cron::{
    current_grant, grants_path, permissions_digest, record_grant, revoke_grants, valid_grant,
    CronAccess, CronGrant, CronPermission, CronService, PersistentCronService,
};
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

const UNATTENDED_OUTSIDE: &str = "outside the workspace in an unattended run";

struct Cron {
    setup: Setup,
    registry: ToolRegistry,
    recorded: Recorded,
    service: Arc<PersistentCronService>,
    grants: PathBuf,
}

/// The production file tools, gated `exec` and `cron`, with the job store
/// where the gateway keeps it.
fn cron() -> Result<Cron, Box<dyn std::error::Error>> {
    let setup = Setup::new()?;
    let (mut registry, recorded) = setup.registry(Exec::Open);
    let store = setup.data.join("runtime").join("cron").join("jobs.json");
    let service = Arc::new(PersistentCronService::new(&store)?);
    service.start()?;
    let tool = CronTool::new(service.clone());
    tool.set_context("cli", "direct", None, Some("cli:direct".to_owned()));
    registry.register(tool);
    let grants = grants_path(&store);
    Ok(Cron {
        setup,
        registry,
        recorded,
        service,
        grants,
    })
}

fn path(path: &std::path::Path, access: CronAccess) -> CronPermission {
    CronPermission::Path {
        path: path.display().to_string(),
        access,
    }
}

fn command(pattern: &str) -> CronPermission {
    CronPermission::Command {
        command: pattern.to_owned(),
    }
}

fn add(permissions: Value) -> shacs_core::runtime::RuntimeToolCall {
    call(
        "cron",
        json!({
            "action": "add", "name": "transcripts", "message": "summarize the new videos",
            "every_seconds": 3600, "permissions": permissions,
        }),
    )
}

impl Cron {
    /// An approval record as an approved `cron add` leaves it.
    fn approve(&self, job_id: &str, items: Vec<CronPermission>) -> TestResult {
        let mut items = items;
        items.sort();
        record_grant(
            &self.grants,
            CronGrant {
                job_id: job_id.to_owned(),
                digest: permissions_digest(&items),
                items,
                approved_at_ms: 1,
                revoked_at_ms: None,
            },
        )?;
        Ok(())
    }

    /// The tool context of a run of `job_id` queued with `items` approved.
    fn run_of(
        &self,
        options: &Options,
        job_id: &str,
        items: &[CronPermission],
    ) -> ToolExecutionContext {
        let mut items = items.to_vec();
        items.sort();
        let mut context = self.setup.context(options);
        context.automation_source = Some(AutomationRunSource::Cron {
            job_id: Some(job_id.to_owned()),
            grant_digest: Some(permissions_digest(&items)),
            work_id: format!("work-{job_id}"),
        });
        context.cron_grant_store = Some(self.grants.clone());
        context
    }
}

fn unattended(outcome: &Outcome) -> bool {
    outcome
        .refusal()
        .is_some_and(|text| text.contains(UNATTENDED_OUTSIDE))
}

#[test]
fn every_interactive_mode_asks_once_for_the_whole_list() -> TestResult {
    // Given
    let cron = cron()?;
    let home = cron.setup.home.display().to_string();
    let list = json!([
        {"path": home, "access": "read"},
        {"command": "python3 /opt/scripts/fetch.py *"},
    ]);
    let modes = [
        Options::auto(true).verdict(Verdict::Allow),
        Options::mode(PermissionMode::BypassPermissions, true),
        Options::mode(PermissionMode::DontAsk, true),
        Options::mode(PermissionMode::Default, true).rules(Rules::allow(&["cron"])),
    ];

    for options in modes {
        // When
        let outcome = run(
            &cron.registry,
            &cron.setup.context(&options),
            add(list.clone()),
        );

        // Then
        let question = outcome
            .question()
            .ok_or_else(|| format!("{:?} not asked: {outcome:?}", options.mode))?;
        assert!(question.once_only(), "{}", question.text);
        for shown in [
            "transcripts",
            "every 3600s",
            "summarize the new videos",
            &format!("read {home}"),
            "run `python3 /opt/scripts/fetch.py *`",
        ] {
            assert!(
                question.text.contains(shown),
                "{shown} in {}",
                question.text
            );
        }
    }
    assert!(cron.service.list_jobs().is_empty());
    Ok(())
}

#[test]
fn an_unattended_run_cannot_register_a_list() -> TestResult {
    // Given
    let cron = cron()?;
    let context = cron
        .setup
        .context(&Options::mode(PermissionMode::BypassPermissions, false));

    // When
    let outcome = run(
        &cron.registry,
        &context,
        add(json!([{"path": "/tmp", "access": "read"}])),
    );

    // Then
    assert!(outcome.refusal().is_some(), "{outcome:?}");
    assert!(cron.service.list_jobs().is_empty());
    Ok(())
}

#[test]
fn approving_keeps_the_job_and_its_record_and_denying_keeps_nothing() -> TestResult {
    // Given
    let cron = cron()?;
    let context = cron
        .setup
        .context(&Options::mode(PermissionMode::Default, true));
    let list = json!([{"path": "/tmp", "access": "write"}]);

    // When
    let denied = run_answered(
        &cron.registry,
        &context,
        add(list.clone()),
        ApprovalDecisionKind::Denied,
    )?;
    let jobs_after_denial = cron.service.list_jobs().len();
    let approved = run_answered(
        &cron.registry,
        &context,
        add(list),
        ApprovalDecisionKind::Approved,
    )?;
    let listed = run_answered(
        &cron.registry,
        &context,
        call("cron", json!({"action": "list"})),
        ApprovalDecisionKind::Approved,
    )?;

    // Then
    assert!(!denied.ran(), "{denied:?}");
    assert_eq!(jobs_after_denial, 0);
    assert!(
        matches!(&approved, Outcome::Ran(text) if text.starts_with("Created job")),
        "{approved:?}"
    );
    let job = cron.service.list_jobs().pop().ok_or("no job")?;
    let items = vec![path(std::path::Path::new("/tmp"), CronAccess::Write)];
    assert!(valid_grant(&cron.grants, &job.id, &permissions_digest(&items)).is_some());
    assert!(
        matches!(&listed, Outcome::Ran(text) if text.contains("Permissions: write /tmp")),
        "{listed:?}"
    );
    Ok(())
}

#[test]
fn lists_naming_secrets_protected_writes_or_broad_commands_are_refused() -> TestResult {
    // Given
    let cron = cron()?;
    let context = cron
        .setup
        .context(&Options::mode(PermissionMode::Default, true));
    let env_file = cron.setup.data.join(".env.json");
    fs::write(&env_file, "{}")?;
    let config = cron.setup.data.join("config.json");
    let refused = [
        json!([{"path": env_file, "access": "read"}]),
        json!([{"path": config, "access": "write"}]),
        json!([{"command": "python3 *"}]),
        json!([{"path": "relative/folder", "access": "read"}]),
    ];

    for list in refused {
        // When
        let outcome = run(&cron.registry, &context, add(list.clone()));

        // Then
        assert!(outcome.question().is_none(), "{list}: {outcome:?}");
        assert!(
            !outcome.ran()
                || matches!(&outcome, Outcome::Ran(text) if !text.starts_with("Created job")),
            "{list}: {outcome:?}"
        );
    }
    assert!(cron.service.list_jobs().is_empty());
    Ok(())
}

#[test]
fn a_cron_run_uses_its_path_entries_by_kind_and_nothing_else() -> TestResult {
    // Given: `notes` may be read, `out` written.
    let cron = cron()?;
    let notes = cron.setup.root.join("notes");
    let out = cron.setup.root.join("out");
    let elsewhere = cron.setup.root.join("elsewhere");
    for folder in [&notes, &out, &elsewhere] {
        fs::create_dir_all(folder)?;
    }
    fs::write(notes.join("a.txt"), "note")?;
    fs::write(elsewhere.join("b.txt"), "other")?;
    symlink_into(&elsewhere.join("b.txt"), &notes.join("link.txt"))?;
    let items = vec![
        path(&notes, CronAccess::Read),
        path(&out, CronAccess::Write),
    ];
    cron.approve("job-1", items.clone())?;
    let auto = cron.run_of(
        &Options::auto(false).verdict(Verdict::Allow),
        "job-1",
        &items,
    );
    let bypass = cron.run_of(
        &Options::mode(PermissionMode::BypassPermissions, false),
        "job-1",
        &items,
    );
    let read = |file: PathBuf| call("read_file", json!({"path": file}));
    let write = |file: PathBuf| call("write_file", json!({"path": file, "content": "x"}));

    // When / Then
    assert!(run(&cron.registry, &auto, read(notes.join("a.txt"))).ran());
    assert!(run(&cron.registry, &auto, write(out.join("w.txt"))).ran());
    assert_eq!(fs::read_to_string(out.join("w.txt"))?, "x");
    for context in [&auto, &bypass] {
        for refused in [
            write(notes.join("w.txt")),
            read(elsewhere.join("b.txt")),
            read(notes.join("link.txt")),
            call(
                "exec",
                json!({"command": format!("mkdir {}", notes.join("d").display())}),
            ),
            call(
                "exec",
                json!({"command": format!("cat {}/../elsewhere/b.txt", notes.display())}),
            ),
        ] {
            let outcome = run(&cron.registry, context, refused.clone());
            assert!(unattended(&outcome), "{refused:?}: {outcome:?}");
        }
    }
    let cat = format!("cat {}", notes.join("a.txt").display());
    assert!(run(&cron.registry, &auto, call("exec", json!({"command": cat}))).ran());
    assert_eq!(cron.recorded.launched(), vec![cat]);
    Ok(())
}

#[cfg(unix)]
fn symlink_into(target: &std::path::Path, link: &std::path::Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(target, link)
}

#[test]
fn another_job_a_heartbeat_or_a_tampered_list_gets_nothing() -> TestResult {
    // Given
    let cron = cron()?;
    let notes = cron.setup.root.join("notes");
    fs::create_dir_all(&notes)?;
    fs::write(notes.join("a.txt"), "note")?;
    let items = vec![path(&notes, CronAccess::Read)];
    cron.approve("job-1", items.clone())?;
    let options = Options::mode(PermissionMode::BypassPermissions, false);
    let read = call("read_file", json!({"path": notes.join("a.txt")}));
    let other_job = cron.run_of(&options, "job-2", &items);
    let mut heartbeat = cron.setup.context(&options);
    heartbeat.automation_source = Some(AutomationRunSource::Heartbeat {
        work_id: "work-h".to_owned(),
    });
    heartbeat.cron_grant_store = Some(cron.grants.clone());
    // The stored job's list grew to the root folder; the record did not.
    let tampered = cron.run_of(
        &options,
        "job-1",
        &[path(&cron.setup.root, CronAccess::Write)],
    );

    // When / Then
    for context in [&other_job, &tampered] {
        let outcome = run(&cron.registry, context, read.clone());
        assert!(unattended(&outcome), "{outcome:?}");
    }
    assert!(
        run(&cron.registry, &heartbeat, read).ran(),
        "a heartbeat in bypass is not a cron run"
    );
    Ok(())
}

#[test]
fn a_removed_job_loses_its_list_from_the_next_call() -> TestResult {
    // Given: the run holds a valid list, then another service instance
    // removes the job between two calls.
    let cron = cron()?;
    let notes = cron.setup.root.join("notes");
    fs::create_dir_all(&notes)?;
    fs::write(notes.join("a.txt"), "note")?;
    let job = cron.service.add_job(shacs_cron::AddCronJob {
        permissions: Vec::new(),
        name: "j".to_owned(),
        schedule: shacs_cron::CronSchedule::every(60_000),
        message: "m".to_owned(),
        deliver: false,
        channel: None,
        to: None,
        delete_after_run: false,
        channel_meta: Value::Null,
        session_key: None,
    });
    let items = vec![path(&notes, CronAccess::Read)];
    cron.approve(&job.id, items.clone())?;
    let context = cron.run_of(
        &Options::mode(PermissionMode::BypassPermissions, false),
        &job.id,
        &items,
    );
    let read = call("read_file", json!({"path": notes.join("a.txt")}));

    // When
    let before = run(&cron.registry, &context, read.clone());
    let other = PersistentCronService::new(cron.service.store_path())?;
    assert_eq!(
        other.remove_job(&job.id),
        shacs_cron::RemoveJobResult::Removed
    );
    let after = run(&cron.registry, &context, read);

    // Then
    assert!(before.ran(), "{before:?}");
    assert!(unattended(&after), "{after:?}");
    Ok(())
}

#[test]
fn a_command_entry_runs_its_script_unasked_in_a_default_mode_cron_run() -> TestResult {
    // Given
    let cron = cron()?;
    let out = cron.setup.root.join("out");
    fs::create_dir_all(&out)?;
    // The skill's script lives in the workspace, as on the server.
    let script = format!(
        "python3 {}",
        cron.setup.workspace.join("scripts/fetch.py").display()
    );
    let items = vec![
        command(&format!("{script} *")),
        path(&out, CronAccess::Write),
    ];
    cron.approve("job-1", items.clone())?;
    let context = cron.run_of(
        &Options::mode(PermissionMode::Default, false),
        "job-1",
        &items,
    );
    let fetch = |command: String| call("exec", json!({"command": command}));

    // When
    let matching = run(&cron.registry, &context, fetch(format!("{script} video")));
    let redirected = run(
        &cron.registry,
        &context,
        fetch(format!("{script} video > {}", out.join("t.txt").display())),
    );
    let refused = [
        fetch(format!(
            "python3 {}",
            cron.setup.workspace.join("scripts/other.py").display()
        )),
        fetch(format!("{script} video && curl https://example.com")),
        fetch(format!("{script} $(cat secret)")),
        fetch(format!(
            "{script} video > {}",
            cron.setup.root.join("elsewhere.txt").display()
        )),
        fetch("rm -rf build".to_owned()),
    ]
    .map(|call| run(&cron.registry, &context, call));
    let interactive = run(
        &cron.registry,
        &cron
            .setup
            .context(&Options::mode(PermissionMode::Default, true)),
        fetch(format!("{script} video")),
    );

    // Then
    assert!(matching.ran(), "{matching:?}");
    assert!(redirected.ran(), "{redirected:?}");
    for outcome in &refused {
        assert!(
            outcome.refusal().is_some()
                || matches!(outcome, Outcome::Ran(text) if text.contains("blocked")),
            "{outcome:?}"
        );
    }
    assert!(
        interactive.question().is_some(),
        "a turn of the same session does not use the job's entries: {interactive:?}"
    );
    assert_eq!(cron.recorded.launched().len(), 2);
    Ok(())
}

#[test]
fn a_narrow_allow_rule_runs_an_unsummarized_script_in_default_mode() -> TestResult {
    // Given
    let cron = cron()?;
    let script = "python3 /opt/scripts/fetch.py video";
    let rules = |rule: &str| Rules::allow(&[rule]);
    let contexts = [
        (
            "narrow",
            cron.setup.context(
                &Options::mode(PermissionMode::Default, true)
                    .rules(rules("exec(python3 /opt/scripts/fetch.py *)")),
            ),
        ),
        (
            "broad",
            cron.setup.context(
                &Options::mode(PermissionMode::Default, true).rules(rules("exec(python3 *)")),
            ),
        ),
        (
            "uncontained",
            cron.setup.context(
                &Options::mode(PermissionMode::Default, true)
                    .rules(rules("exec(python3 /opt/scripts/fetch.py *)"))
                    .native(),
            ),
        ),
    ];

    for (name, context) in contexts {
        // When
        let outcome = run(
            &cron.registry,
            &context,
            call("exec", json!({"command": script})),
        );

        // Then
        match name {
            "narrow" => assert!(outcome.ran(), "{name}: {outcome:?}"),
            _ => assert!(!outcome.ran(), "{name}: {outcome:?}"),
        }
    }
    assert_eq!(cron.recorded.launched(), vec![script.to_owned()]);
    Ok(())
}

#[test]
fn the_approval_records_are_a_secret_file() -> TestResult {
    // Given
    let cron = cron()?;
    cron.approve(
        "job-1",
        vec![path(std::path::Path::new("/tmp"), CronAccess::Read)],
    )?;
    let bypass = cron
        .setup
        .context(&Options::mode(PermissionMode::BypassPermissions, true));

    // When
    let read = run(
        &cron.registry,
        &bypass,
        call("read_file", json!({"path": cron.grants})),
    );
    let write = run(
        &cron.registry,
        &bypass,
        call("write_file", json!({"path": cron.grants, "content": "{}"})),
    );

    // Then
    let question = read
        .question()
        .ok_or_else(|| format!("not asked: {read:?}"))?;
    assert!(question.once_only());
    assert!(write.refusal().is_some(), "{write:?}");
    Ok(())
}

#[test]
fn in_auto_mode_a_write_entry_stands_in_for_an_edit_rule_on_redirect_targets() -> TestResult {
    // Given: no classifier verdict, so only an allow rule (row 12) can run
    // the command; its redirect target must be covered.
    let cron = cron()?;
    let out = cron.setup.root.join("out");
    fs::create_dir_all(&out)?;
    let script = format!(
        "python3 {}",
        cron.setup.workspace.join("scripts/fetch.py").display()
    );
    let items = vec![
        command(&format!("{script} *")),
        path(&out, CronAccess::Write),
    ];
    cron.approve("job-1", items.clone())?;
    let context = cron.run_of(&Options::auto(false), "job-1", &items);
    let redirected = format!("{script} video > {}", out.join("t.txt").display());

    // When
    let outcome = run(
        &cron.registry,
        &context,
        call("exec", json!({"command": redirected})),
    );

    // Then
    assert!(outcome.ran(), "{outcome:?}");
    assert_eq!(cron.recorded.launched(), vec![redirected]);
    Ok(())
}

#[test]
fn bypass_registers_the_job_once_the_user_approves() -> TestResult {
    // Given
    let cron = cron()?;
    let context = cron
        .setup
        .context(&Options::mode(PermissionMode::BypassPermissions, true));

    // When
    let approved = run_answered(
        &cron.registry,
        &context,
        add(json!([{"path": "/tmp", "access": "read"}])),
        ApprovalDecisionKind::Approved,
    )?;

    // Then
    assert!(
        matches!(&approved, Outcome::Ran(text) if text.starts_with("Created job")),
        "{approved:?}"
    );
    assert_eq!(cron.service.list_jobs().len(), 1);
    Ok(())
}

#[test]
fn an_outside_working_folder_needs_a_write_entry() -> TestResult {
    // Given: `notes` is only readable.
    let cron = cron()?;
    let notes = cron.setup.root.join("notes");
    fs::create_dir_all(&notes)?;
    let items = vec![path(&notes, CronAccess::Read)];
    cron.approve("job-1", items.clone())?;
    let context = cron.run_of(
        &Options::mode(PermissionMode::BypassPermissions, false),
        "job-1",
        &items,
    );

    // When
    let outcome = run(
        &cron.registry,
        &context,
        call(
            "exec",
            json!({"command": format!("ls {}; touch new.txt", notes.display()), "working_dir": notes}),
        ),
    );

    // Then
    assert!(unattended(&outcome), "{outcome:?}");
    assert!(cron.recorded.launched().is_empty());
    Ok(())
}

#[test]
fn a_working_folder_reached_through_a_link_and_dotdot_still_needs_a_write_entry() -> TestResult {
    // Given: `link` leads to `outside/child`, so `link/..` is `outside`,
    // which is only readable.
    let cron = cron()?;
    let outside = cron.setup.root.join("outside");
    fs::create_dir_all(outside.join("child"))?;
    symlink_into(&outside.join("child"), &cron.setup.workspace.join("link"))?;
    let items = vec![path(&outside, CronAccess::Read)];
    cron.approve("job-1", items.clone())?;
    let context = cron.run_of(
        &Options::mode(PermissionMode::BypassPermissions, false),
        "job-1",
        &items,
    );

    // When
    let outcome = run(
        &cron.registry,
        &context,
        call(
            "exec",
            json!({"command": format!("ls {}; touch new.txt", outside.display()), "working_dir": "link/.."}),
        ),
    );

    // Then
    assert!(unattended(&outcome), "{outcome:?}");
    assert!(cron.recorded.launched().is_empty());
    Ok(())
}

#[test]
fn a_path_asked_for_outside_the_list_is_refused_even_if_it_leads_into_it() -> TestResult {
    // Given: `elsewhere/link` leads to an approved file.
    let cron = cron()?;
    let notes = cron.setup.root.join("notes");
    let elsewhere = cron.setup.root.join("elsewhere");
    fs::create_dir_all(&notes)?;
    fs::create_dir_all(&elsewhere)?;
    fs::write(notes.join("a.txt"), "note")?;
    symlink_into(&notes.join("a.txt"), &elsewhere.join("link.txt"))?;
    let items = vec![path(&notes, CronAccess::Read)];
    cron.approve("job-1", items.clone())?;
    let context = cron.run_of(
        &Options::mode(PermissionMode::BypassPermissions, false),
        "job-1",
        &items,
    );

    // When
    let outcome = run(
        &cron.registry,
        &context,
        call("read_file", json!({"path": elsewhere.join("link.txt")})),
    );

    // Then
    assert!(unattended(&outcome), "{outcome:?}");
    Ok(())
}

#[test]
fn a_hard_link_to_the_approval_records_is_a_secret_file_too() -> TestResult {
    // Given
    let cron = cron()?;
    cron.approve(
        "job-1",
        vec![path(std::path::Path::new("/tmp"), CronAccess::Read)],
    )?;
    let alias = cron.setup.workspace.join("alias.json");
    fs::hard_link(&cron.grants, &alias)?;
    let bypass = cron
        .setup
        .context(&Options::mode(PermissionMode::BypassPermissions, true));

    // When
    let read = run(
        &cron.registry,
        &bypass,
        call("read_file", json!({"path": "alias.json"})),
    );
    let write = run(
        &cron.registry,
        &bypass,
        call("write_file", json!({"path": "alias.json", "content": "{}"})),
    );

    // Then
    assert!(
        read.question().is_some_and(|question| question.once_only()),
        "{read:?}"
    );
    assert!(write.refusal().is_some(), "{write:?}");
    assert!(fs::read_to_string(&cron.grants)?.contains("job-1"));
    Ok(())
}

#[test]
fn a_longer_list_written_into_the_job_store_is_not_used() -> TestResult {
    // Given: an approved job, then its stored list grows by hand.
    let cron = cron()?;
    let notes = cron.setup.root.join("notes");
    let root_file = cron.setup.root.join("root.txt");
    fs::create_dir_all(&notes)?;
    let context = cron
        .setup
        .context(&Options::mode(PermissionMode::Default, true));
    run_answered(
        &cron.registry,
        &context,
        add(json!([{"path": notes, "access": "write"}])),
        ApprovalDecisionKind::Approved,
    )?;
    let job = cron.service.list_jobs().pop().ok_or("no job")?;
    let store = cron.service.store_path().to_path_buf();
    let stored = fs::read_to_string(&store)?.replace(
        &format!("\"path\": \"{}\"", notes.display()),
        &format!("\"path\": \"{}\"", cron.setup.root.display()),
    );
    fs::write(&store, stored)?;
    let grant = current_grant(&cron.grants, &job.id).ok_or("no record")?;
    let run_context = cron.run_of(
        &Options::mode(PermissionMode::BypassPermissions, false),
        &job.id,
        &grant.items,
    );

    // When
    let widened = run(
        &cron.registry,
        &run_context,
        call("write_file", json!({"path": root_file, "content": "x"})),
    );
    let approved = run(
        &cron.registry,
        &run_context,
        call(
            "write_file",
            json!({"path": notes.join("n.txt"), "content": "x"}),
        ),
    );

    // Then
    assert!(unattended(&widened), "{widened:?}");
    assert!(approved.ran(), "{approved:?}");
    Ok(())
}

/// A read-only call that revokes job-1's list when it runs, as a removal by
/// the user in the middle of a dispatch.
struct RemoveWhileRunning(PathBuf);

impl Tool for RemoveWhileRunning {
    fn name(&self) -> &str {
        "remove_meanwhile"
    }
    fn description(&self) -> &str {
        "Revoke the list while the dispatch runs"
    }
    fn parameters(&self) -> Value {
        json!({"type": "object", "properties": {}})
    }
    fn read_only(&self) -> bool {
        true
    }
    fn execute(&self, _: JsonMap) -> ToolResult {
        match revoke_grants(&self.0, "job-1", 2) {
            Ok(()) => "revoked".into(),
            Err(error) => error.to_string().into(),
        }
    }
}

#[test]
fn a_call_decided_before_a_removal_does_not_start_after_it() -> TestResult {
    // Given: in one dispatch, a read-only batch runs (and the list is
    // revoked meanwhile) before an already decided write.
    let mut cron = cron()?;
    let out = cron.setup.root.join("out");
    fs::create_dir_all(&out)?;
    let items = vec![path(&out, CronAccess::Write)];
    cron.approve("job-1", items.clone())?;
    cron.registry
        .register(RemoveWhileRunning(cron.grants.clone()));
    let context = cron.run_of(
        &Options::mode(PermissionMode::BypassPermissions, false),
        "job-1",
        &items,
    );

    // When
    let report = RuntimeToolExecutor::new(&cron.registry).execute_tool_calls_concurrent(
        vec![
            call("remove_meanwhile", json!({})),
            call(
                "write_file",
                json!({"path": out.join("w.txt"), "content": "x"}),
            ),
        ],
        &context,
    );

    // Then
    let write = report.messages.last().ok_or("no write result")?;
    assert!(write.content.contains(UNATTENDED_OUTSIDE), "{report:?}");
    assert!(!out.join("w.txt").exists());
    Ok(())
}
