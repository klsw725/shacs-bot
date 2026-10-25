use shacs_projection::{Spec035TaskCount, Spec035TaskOwnerKind, Spec035TasksProjection};
use std::collections::BTreeSet;
use std::error::Error;
use std::fs;
use std::io::ErrorKind;
use std::os::unix::fs::{symlink, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

pub type TestResult<T = ()> = Result<T, Box<dyn Error>>;
pub type Tree = Vec<(String, Option<Vec<u8>>)>;

/// Locations the pre-043 binary wrote; Spec 043 keeps them byte for byte.
pub const PRESERVED: [&str; 3] = ["apps", "authoring", "snapshots/app-lifecycle"];

const HELLO: &str = r#"{"client_id":"client:spec043","schema_versions":[1],"mutation_capabilities":["task_stop","task_recover"]}"#;

const APPS_FORMS: [&[&str]; 19] = [
    &["apps"],
    &["app"],
    &["apps", "--help"],
    &["apps", "-h"],
    &["apps", "init", "demo.new"],
    &["apps", "propose", "draft-demo.draft", "--intent", "retired"],
    &["apps", "apply", "draft-demo.draft", "--intent", "retired"],
    &["apps", "install", "demo.note.shacsapp"],
    &["apps", "list"],
    &["apps", "inspect", "demo.clock"],
    &["apps", "enable", "demo.clock"],
    &["apps", "disable", "demo.clock"],
    &["apps", "start", "demo.clock"],
    &["apps", "stop", "demo.clock"],
    &["apps", "restart", "demo.clock"],
    &["apps", "recover", "demo.recover"],
    &["apps", "uninstall", "demo.clock"],
    &["apps", "frobnicate", "--weird", "value"],
    &["app", "list", "--workspace", "/must-not-be-read"],
];

/// Copies the leftover app data that the pre-043 binary created into `data_dir`.
pub fn copy_into(data_dir: &Path) -> TestResult {
    copy_dir(
        &Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../shacs-core/tests/fixtures/spec043_leftover_apps"),
        data_dir,
    )
}

fn copy_dir(source: &Path, target: &Path) -> TestResult {
    fs::create_dir_all(target)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let path = target.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir(&entry.path(), &path)?;
        } else {
            fs::copy(entry.path(), path)?;
        }
    }
    Ok(())
}

pub fn tree(root: &Path, scopes: &[&str]) -> TestResult<Tree> {
    let mut entries = Vec::new();
    for scope in scopes {
        collect(root, &root.join(scope), &mut entries)?;
    }
    entries.sort();
    Ok(entries)
}

fn collect(root: &Path, path: &Path, entries: &mut Tree) -> TestResult {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.into()),
    };
    let relative = path.strip_prefix(root)?.to_string_lossy().into_owned();
    if metadata.file_type().is_symlink() {
        entries.push((
            format!("{relative} -> {}", fs::read_link(path)?.display()),
            None,
        ));
    } else if metadata.is_dir() {
        entries.push((relative, None));
        for entry in fs::read_dir(path)? {
            collect(root, &entry?.path(), entries)?;
        }
    } else {
        entries.push((relative, Some(fs::read(path)?)));
    }
    Ok(())
}

struct Roots {
    root: tempfile::TempDir,
    data_dir: PathBuf,
    workspace: PathBuf,
}

fn roots(leftover: bool) -> TestResult<Roots> {
    let root = tempfile::tempdir()?;
    let data_dir = root.path().join("data");
    let workspace = root.path().join("workspace");
    for directory in [&data_dir, &workspace, &root.path().join("home")] {
        fs::create_dir_all(directory)?;
    }
    fs::write(data_dir.join("config.json"), b"{}")?;
    if leftover {
        copy_into(&data_dir)?;
    }
    Ok(Roots {
        root,
        data_dir,
        workspace,
    })
}

fn tasks(roots: &Roots, extra: &[&str]) -> TestResult<Output> {
    Ok(Command::new(env!("CARGO_BIN_EXE_shacs-bot"))
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", roots.root.path().join("home"))
        .arg("--config")
        .arg(roots.data_dir.join("config.json"))
        .args(["tasks", "--json", "--session", "cli:direct", "--workspace"])
        .arg(&roots.workspace)
        .arg("--data-dir")
        .arg(&roots.data_dir)
        .args(extra)
        .output()?)
}

fn command(roots: &Roots) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_shacs-bot"));
    command
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", roots.root.path().join("home"));
    command
}

fn apps_notice(roots: &Roots, args: &[&str]) -> TestResult<String> {
    let output = command(roots)
        .arg("--config")
        .arg(roots.data_dir.join("config.json"))
        .args(args)
        .output()?;
    assert_eq!(output.status.code(), Some(1), "{args:?}");
    assert!(output.stdout.is_empty(), "{args:?}");
    Ok(String::from_utf8(output.stderr)?)
}

fn app_line<'a>(notice: &'a str, id: &str) -> TestResult<&'a str> {
    Ok(notice
        .lines()
        .find(|line| line.starts_with(&format!("- {id}: ")))
        .ok_or(format!("no line for {id}"))?)
}

#[test]
fn every_apps_form_prints_one_read_only_notice_and_fails() -> TestResult {
    for leftover in [true, false] {
        // Given
        let roots = roots(leftover)?;
        let before = tree(&roots.data_dir, &[""])?;
        let mut notices = BTreeSet::new();

        // When
        for form in APPS_FORMS {
            notices.insert(apps_notice(&roots, form)?);
        }
        let trailing_config = command(&roots)
            .args(["apps", "list", "--config"])
            .arg(roots.data_dir.join("config.json"))
            .output()?;
        notices.insert(String::from_utf8(trailing_config.stderr)?);

        // Then
        assert_eq!(trailing_config.status.code(), Some(1));
        assert_eq!(notices.len(), 1, "{notices:#?}");
        let notice = notices.pop_first().ok_or("notice")?;
        assert!(notice.starts_with("shacs-bot apps: the app capability has been retired."));
        assert!(notice.contains(&format!("Data dir: {}", roots.data_dir.display())));
        assert_eq!(notice.contains("demo.clock"), leftover);
        assert_eq!(
            notice.contains("No leftover app data was found in this data dir."),
            !leftover
        );
        assert_eq!(tree(&roots.data_dir, &[""])?, before);
    }
    Ok(())
}

#[test]
fn notice_lists_leftover_locations_and_never_claims_a_live_app_stopped() -> TestResult {
    // Given
    let roots = roots(true)?;

    // When
    let notice = apps_notice(&roots, &["apps", "list"])?;

    // Then
    for location in [
        "- apps/registry.json: present, 1 file",
        "- apps/<id>.shacsapp/: 3 bundles, 7 files",
        "- apps/app-supervisor/: 4 journals, 22 files",
        "- authoring/apps/: 4 drafts (demo.clock, demo.draft, demo.note, demo.recover), 16 files",
        "- authoring/flow/: present, 12 files",
        "- snapshots/app-lifecycle/: present, 3 files",
        "- runtime/app-ledger/: absent",
    ] {
        assert!(notice.contains(location), "{location}\n{notice}");
    }
    for id in ["demo.clock", "demo.recover"] {
        let line = app_line(&notice, id)?;
        assert!(line.contains("may still be running"), "{line}");
        assert!(!line.contains("stopped"), "{line}");
    }
    assert!(app_line(&notice, "demo.clock")?.contains("last journal state=running"));
    assert!(app_line(&notice, "demo.recover")?.contains("last journal state=recovery_needed"));
    let note = app_line(&notice, "demo.note")?;
    assert!(note.contains("registry=enabled, last journal state=stopped, bundle=present"));
    assert!(note.contains("does not guarantee that no app process is running"));
    let unregistered = app_line(&notice, "nope.app")?;
    assert!(unregistered.contains("registry=not registered, last journal state=failed"));
    assert!(!notice.contains("fixture-src"), "{notice}");
    Ok(())
}

#[test]
fn malformed_leftover_files_are_named_without_their_contents() -> TestResult {
    // Given
    let roots = roots(true)?;
    fs::write(
        roots.data_dir.join("apps/registry.json"),
        "{\"entries\": SECRET-REGISTRY-RAW",
    )?;
    fs::write(
        roots
            .data_dir
            .join("apps/app-supervisor/demo.note/state.json"),
        "SECRET-STATE-RAW",
    )?;
    let before = tree(&roots.data_dir, &[""])?;

    // When
    let notice = apps_notice(&roots, &["apps"])?;

    // Then
    assert!(notice.contains("- apps/registry.json: present, unreadable"));
    let note = app_line(&notice, "demo.note")?;
    assert!(
        note.contains("registry=unknown (registry unreadable)"),
        "{note}"
    );
    assert!(note.contains("last journal state=unreadable"), "{note}");
    assert!(note.contains("may still be running"), "{note}");
    assert!(!notice.contains("SECRET-"), "{notice}");
    assert_eq!(tree(&roots.data_dir, &[""])?, before);
    Ok(())
}

#[test]
fn symlinks_out_of_the_data_dir_are_never_followed() -> TestResult {
    // Given
    let roots = roots(true)?;
    let outside = roots.root.path().join("outside");
    fs::create_dir_all(outside.join("flow"))?;
    fs::create_dir_all(outside.join("receipts"))?;
    fs::write(outside.join("flow/OUTSIDE-FLOW.json"), "{}")?;
    fs::write(outside.join("receipts/OUTSIDE-RECEIPT.json"), "{}")?;
    fs::write(outside.join("OUTSIDE-LOCK"), "lock")?;
    fs::write(outside.join("state.json"), r#"{"state":"stopped"}"#)?;
    let journal = roots.data_dir.join("apps/app-supervisor/demo.clock");
    fs::remove_dir_all(roots.data_dir.join("authoring/flow"))?;
    symlink(outside.join("flow"), roots.data_dir.join("authoring/flow"))?;
    fs::remove_dir_all(journal.join("receipts"))?;
    symlink(outside.join("receipts"), journal.join("receipts"))?;
    fs::remove_file(journal.join("lifecycle.lock"))?;
    symlink(outside.join("OUTSIDE-LOCK"), journal.join("lifecycle.lock"))?;
    let recover_state = roots
        .data_dir
        .join("apps/app-supervisor/demo.recover/state.json");
    fs::remove_file(&recover_state)?;
    symlink(outside.join("state.json"), &recover_state)?;
    let before = tree(&roots.data_dir, &[""])?;
    let outside_before = tree(&outside, &[""])?;

    // When
    let notice = apps_notice(&roots, &["apps", "status"])?;

    // Then
    assert!(notice.contains("- authoring/flow/: symlink, not followed"));
    assert!(
        notice.contains("- apps/app-supervisor/: 4 journals, 18 files, 3 symlinks not followed")
    );
    let recover = app_line(&notice, "demo.recover")?;
    assert!(
        recover.contains("last journal state=unreadable"),
        "{recover}"
    );
    assert!(recover.contains("may still be running"), "{recover}");
    assert!(!notice.contains("OUTSIDE"), "{notice}");
    assert_eq!(tree(&roots.data_dir, &[""])?, before);
    assert_eq!(tree(&outside, &[""])?, outside_before);
    Ok(())
}

#[test]
fn unreadable_leftover_folders_are_reported_as_unreadable() -> TestResult {
    // Given
    let roots = roots(true)?;
    let apps = roots.data_dir.join("apps");
    let journals = apps.join("app-supervisor");
    let drafts = roots.data_dir.join("authoring/apps");

    // When
    set_mode(&[&journals, &drafts], 0o000)?;
    let nested = apps_notice(&roots, &["apps"]);
    set_mode(&[&journals, &drafts], 0o755)?;
    set_mode(&[&apps], 0o000)?;
    let top = apps_notice(&roots, &["apps"]);
    set_mode(&[&apps], 0o755)?;
    let (nested, top) = (nested?, top?);

    // Then
    assert!(
        nested.contains("- apps/app-supervisor/: present, unreadable"),
        "{nested}"
    );
    assert!(
        nested.contains("- authoring/apps/: present, unreadable"),
        "{nested}"
    );
    assert!(
        top.contains("- apps/<id>.shacsapp/: present, unreadable"),
        "{top}"
    );
    for notice in [&nested, &top] {
        assert!(!notice.contains("No leftover app data was found"));
    }
    Ok(())
}

fn set_mode(paths: &[&Path], mode: u32) -> TestResult {
    for path in paths {
        fs::set_permissions(path, fs::Permissions::from_mode(mode))?;
    }
    Ok(())
}

#[test]
fn missing_config_still_reads_only_and_creates_nothing() -> TestResult {
    // Given
    let leftover = roots(true)?;
    fs::remove_file(leftover.data_dir.join("config.json"))?;
    let before = tree(&leftover.data_dir, &[""])?;
    let absent = roots(false)?;
    fs::remove_dir_all(&absent.data_dir)?;

    // When
    let notice = apps_notice(&leftover, &["apps", "list"])?;
    let empty = apps_notice(&absent, &["apps", "list"])?;

    // Then
    assert!(app_line(&notice, "demo.clock")?.contains("may still be running"));
    assert_eq!(tree(&leftover.data_dir, &[""])?, before);
    assert!(empty.contains("No leftover app data was found in this data dir."));
    assert!(!absent.data_dir.exists());
    Ok(())
}

#[test]
fn tasks_json_ignores_leftover_app_data() -> TestResult {
    // Given
    let empty = roots(false)?;
    let leftover = roots(true)?;
    let before = tree(&leftover.data_dir, &PRESERVED)?;

    // When
    let without = tasks(&empty, &[])?;
    let with = tasks(&leftover, &[])?;

    // Then
    assert!(without.status.success() && with.status.success());
    assert_eq!(with.stdout, without.stdout);
    let projection = Spec035TasksProjection::parse_json(std::str::from_utf8(&with.stdout)?)?;
    assert_eq!(projection.coverage().app, Spec035TaskCount::Unavailable);
    assert!(projection
        .rows()
        .iter()
        .all(|row| row.owner().kind() != Spec035TaskOwnerKind::App));
    assert_eq!(tree(&leftover.data_dir, &PRESERVED)?, before);
    Ok(())
}

#[test]
fn tasks_app_actions_are_unsupported_without_touching_leftover_app_data() -> TestResult {
    // Given
    let leftover = roots(true)?;
    let before = tree(&leftover.data_dir, &PRESERVED)?;

    for (action, locator) in [("stop", "demo.clock"), ("recover", "demo.recover")] {
        // When
        let output = tasks(
            &leftover,
            &[
                "--owner",
                "app",
                "--action",
                action,
                "--locator",
                locator,
                "--transport-hello",
                HELLO,
            ],
        )?;

        // Then
        assert!(!output.status.success());
        assert!(String::from_utf8(output.stderr)?.contains("unsupported tasks owner/action pair"));
    }
    assert_eq!(tree(&leftover.data_dir, &PRESERVED)?, before);
    Ok(())
}
