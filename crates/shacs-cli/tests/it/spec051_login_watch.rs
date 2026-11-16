use super::spec049_browser_runtime::Runner;
use super::spec051_rendered_browser::{pause_at, Fault, Rig};
use fs2::FileExt;
use serde_json::json;
use shacs_cli::browser::{
    self, BrowserChildKind, BrowserClock, BrowserControl, ForgetConfirmation, ForgetRequest,
    ForgetStage,
};
use shacs_cli::ScriptedPrompt;
use shacs_core::runtime::{
    BrowserCloseOutcome, BrowserError, BrowserHost, BrowserTakeoverError, BrowserTakeoverRequest,
    BrowserUnavailableReason, RenderedWebReader, RenderedWebRequest, WebCheckFailure,
    WebFetchLimits,
};
use std::error::Error;
use std::fs::{self, File};
use std::os::unix::fs::{symlink, PermissionsExt};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

type TestResult = Result<(), Box<dyn Error>>;
const URL: &str = "https://example.com/item";

fn request(saved: bool) -> RenderedWebRequest<'static> {
    RenderedWebRequest {
        url: URL,
        limits: &WebFetchLimits::PRODUCTION,
        use_saved_login: saved,
    }
}

fn runner(rig: &Rig) -> Runner {
    Runner {
        facts: rig.facts.clone(),
        clock: rig.clock.clone(),
        chromium_dead: Arc::new(AtomicBool::new(false)),
        xvfb_dead: Arc::new(AtomicBool::new(false)),
        advance_start: 0,
        advance_read: 0,
    }
}

fn forget(rig: &Rig, clock: &dyn BrowserClock) -> browser::ForgetReport {
    browser::forget(
        &ForgetRequest {
            data_dir: rig.root.path(),
            confirmation: ForgetConfirmation::Skip,
            runner: &runner(rig),
            clock,
        },
        &mut ScriptedPrompt::answering(None),
    )
}

fn replace(path: &std::path::Path) {
    fs::remove_file(path).expect("remove old lock file");
    fs::write(path, "").expect("replace lock file");
}

#[test]
fn saved_watch_uses_locked_profile_and_ephemeral_http_cache() -> TestResult {
    for saved in [false, true] {
        let rig = Rig::new()?;
        let (entered, release) = rig.block();
        let (stopping, finish) = pause_at(&rig.stopping);
        std::thread::scope(|scope| {
            let read = scope.spawn(|| rig.host.read(&request(saved)));
            entered.recv_timeout(Duration::from_secs(5)).unwrap();
            let folder = rig.folder();
            for path in [folder.active_lock(), folder.root().to_path_buf()] {
                assert!(File::open(path).unwrap().try_lock_exclusive().is_err());
            }
            let facts = rig.facts.lock().unwrap();
            let chromium = facts
                .spawns
                .iter()
                .find(|spec| spec.kind == BrowserChildKind::Chromium)
                .unwrap();
            let profile = chromium
                .arguments
                .iter()
                .find_map(|arg| arg.strip_prefix("--user-data-dir="))
                .unwrap();
            if saved {
                assert_eq!(std::path::Path::new(profile), folder.profile());
                for path in [folder.profile_lock(), folder.profile()] {
                    assert!(File::open(path).unwrap().try_lock_exclusive().is_err());
                }
                let cache = chromium
                    .arguments
                    .iter()
                    .find_map(|arg| arg.strip_prefix("--disk-cache-dir="))
                    .unwrap();
                assert!(!std::path::Path::new(cache).starts_with(folder.profile()));
                assert!(chromium
                    .arguments
                    .contains(&"--password-store=basic".to_owned()));
            } else {
                assert!(!folder.profile().exists());
                assert!(!folder.profile_lock().exists());
            }
            drop(facts);
            let before_close = rig.clock.now_ms();
            release.send(()).unwrap();
            stopping.recv_timeout(Duration::from_secs(5)).unwrap();
            assert_eq!(
                rig.clock.now_ms() - before_close,
                if saved { 5000 } else { 0 }
            );
            assert!(File::open(folder.root())
                .unwrap()
                .try_lock_exclusive()
                .is_err());
            if saved {
                assert!(File::open(folder.profile())
                    .unwrap()
                    .try_lock_exclusive()
                    .is_err());
            }
            assert_eq!(
                rig.host.read(&request(saved)).unwrap_err().failure,
                WebCheckFailure::BrowserBusy
            );
            finish.send(()).unwrap();
            read.join().unwrap().unwrap();
            rig.cleaned();
            for path in [folder.active_lock(), folder.root().to_path_buf()] {
                File::open(path).unwrap().try_lock_exclusive().unwrap();
            }
            assert_eq!(folder.profile().exists(), saved);
            assert_eq!(
                rig.page
                    .lock()
                    .unwrap()
                    .commands
                    .iter()
                    .any(|(method, _, _)| method == "Browser.close"),
                saved
            );
            if saved {
                assert!(
                    !rig.page
                        .lock()
                        .unwrap()
                        .commands
                        .iter()
                        .any(|(method, _, _)| method == "Target.closeTarget"),
                    "saved generations must force cleanup immediately after Browser.close's budget"
                );
                assert_eq!(
                    fs::metadata(folder.profile()).unwrap().permissions().mode() & 0o777,
                    0o700
                );
                File::open(folder.profile())
                    .unwrap()
                    .try_lock_exclusive()
                    .unwrap();
            }
        });
    }
    Ok(())
}

#[test]
fn login_watch_obeys_singleton_and_profile_identity_checks() -> TestResult {
    let disabled = Rig::with_profile(browser::ProfileUse::Temporary)?;
    assert_eq!(
        disabled
            .host
            .read(&request(true))
            .unwrap_err()
            .browser_reason,
        Some(BrowserUnavailableReason::PersistLoginDisabled)
    );
    assert!(disabled.facts.lock().unwrap().spawns.is_empty());
    for case in [
        "directory",
        "file",
        "elsewhere",
        "live",
        "unreadable",
        "orphan",
    ] {
        let rig = Rig::new()?;
        fs::create_dir_all(rig.folder().profile())?;
        fs::set_permissions(rig.folder().profile(), fs::Permissions::from_mode(0o700))?;
        fs::write(rig.folder().profile().join("keep"), "profile")?;
        let held = match case {
            "directory" => Some(File::open(rig.folder().profile())?),
            "file" => {
                fs::write(rig.folder().profile_lock(), "")?;
                Some(File::open(rig.folder().profile_lock())?)
            }
            "elsewhere" => {
                symlink(
                    "other-host-123",
                    rig.folder().profile().join("SingletonLock"),
                )?;
                None
            }
            "live" => {
                symlink(
                    "spec051-host-123",
                    rig.folder().profile().join("SingletonLock"),
                )?;
                None
            }
            "orphan" => {
                symlink(
                    "spec051-host-456",
                    rig.folder().profile().join("SingletonLock"),
                )?;
                None
            }
            "unreadable" => {
                fs::write(rig.folder().profile().join("SingletonLock"), "invalid")?;
                None
            }
            _ => unreachable!(),
        };
        if let Some(file) = &held {
            file.try_lock_exclusive()?;
        }
        let result = rig.host.read(&request(true));
        if case == "orphan" {
            result?;
            assert_eq!(
                fs::read_link(rig.folder().profile().join("SingletonLock"))?.to_str(),
                Some("spec051-host-456")
            );
        } else {
            assert_eq!(
                result.unwrap_err().browser_reason,
                Some(if matches!(case, "file" | "directory") {
                    BrowserUnavailableReason::ProfileInUse
                } else {
                    BrowserUnavailableReason::ProfileLocked
                }),
                "{case}"
            );
            assert!(rig.facts.lock().unwrap().spawns.is_empty(), "{case}");
        }
        assert_eq!(
            fs::read_to_string(rig.folder().profile().join("keep"))?,
            "profile"
        );
    }
    Ok(())
}

#[test]
fn watch_start_loses_profile_during_directory_lock_wait() -> TestResult {
    for recreate in [false, true] {
        let rig = Rig::new()?;
        fs::create_dir_all(rig.folder().profile())?;
        fs::set_permissions(rig.folder().profile(), fs::Permissions::from_mode(0o700))?;
        let held = File::open(rig.folder().profile())?;
        held.try_lock_exclusive()?;
        let (waiting, release) = pause_at(&rig.pause);
        std::thread::scope(|scope| {
            let read = scope.spawn(|| rig.host.read(&request(true)));
            waiting.recv_timeout(Duration::from_secs(5)).unwrap();
            FileExt::unlock(&held).unwrap();
            replace(&rig.folder().active_lock());
            replace(&rig.folder().profile_lock());
            let report = forget(&rig, rig.clock.as_ref());
            assert_eq!(report.stage, Some(ForgetStage::Removed));
            if recreate {
                fs::create_dir(rig.folder().profile()).unwrap();
            }
            release.send(()).unwrap();
            assert_eq!(
                read.join().unwrap().unwrap_err().browser_reason,
                Some(BrowserUnavailableReason::ProfileUnreadable)
            );
            assert!(rig.facts.lock().unwrap().spawns.is_empty());
            assert_eq!(rig.folder().profile().exists(), recreate);
        });
    }
    Ok(())
}

struct ClosingClock<'a> {
    rig: &'a Rig,
    release: Mutex<Option<mpsc::Sender<()>>>,
    finished: Mutex<mpsc::Receiver<()>>,
    first_advance: u64,
}

impl BrowserClock for ClosingClock<'_> {
    fn now_ms(&self) -> u64 {
        self.rig.clock.now_ms()
    }
    fn pause(&self, ms: u64) {
        if let Some(release) = self.release.lock().expect("release lock poisoned").take() {
            self.rig
                .clock
                .0
                .fetch_add(self.first_advance, Ordering::SeqCst);
            self.rig.host.maintenance().expect("lock maintenance");
            release.send(()).expect("release reader");
            self.finished
                .lock()
                .expect("completion lock poisoned")
                .recv_timeout(Duration::from_secs(5))
                .expect("reader cleanup deadline");
            self.rig.cleaned();
        } else {
            self.rig.clock.0.fetch_add(ms, Ordering::SeqCst);
        }
    }
}

#[test]
fn forget_and_maintenance_close_saved_watch_before_profile_removal() -> TestResult {
    for way in ["forget", "active", "profile", "close", "shutdown", "panic"] {
        let rig = Rig::new()?;
        let (entered, release) = rig.block();
        let (finished, receive) = mpsc::channel();
        std::thread::scope(|scope| {
            let read = scope.spawn(|| {
                let result = rig.host.read(&request(true));
                finished.send(()).unwrap();
                result
            });
            let cancellation = entered.recv_timeout(Duration::from_secs(5)).unwrap();
            assert!(rig.folder().profile().exists());
            let before = rig.clock.now_ms();
            assert!(matches!(
                rig.host
                    .prepare("cli:other", &json!({"action":"open","url":URL})),
                Err(BrowserError::Busy)
            ));
            assert!(matches!(
                rig.host.takeover(&BrowserTakeoverRequest {
                    session_id: "cli:other".to_owned(),
                    channel: "cli".to_owned(),
                    chat_id: "other".to_owned(),
                    reply_route: Default::default(),
                    tool_call_id: None,
                }),
                Err(BrowserTakeoverError::Browser(BrowserError::Busy))
            ));
            assert_eq!(rig.clock.now_ms(), before);
            assert_eq!(
                rig.host.status().unwrap().control,
                BrowserControl::Assistant
            );
            assert_eq!(
                rig.host.read(&request(true)).unwrap_err().failure,
                WebCheckFailure::BrowserBusy
            );
            match way {
                "forget" => {
                    let clock = ClosingClock {
                        rig: &rig,
                        release: Mutex::new(Some(release)),
                        finished: Mutex::new(receive),
                        first_advance: 1000,
                    };
                    assert_eq!(forget(&rig, &clock).stage, Some(ForgetStage::Removed));
                }
                "active" | "profile" => {
                    replace(&if way == "active" {
                        rig.folder().active_lock()
                    } else {
                        rig.folder().profile_lock()
                    });
                    rig.clock.0.fetch_add(1000, Ordering::SeqCst);
                    rig.host.maintenance().unwrap();
                    assert!(cancellation.check().is_err());
                    release.send(()).unwrap();
                }
                "close" => {
                    assert_eq!(rig.host.close_by_user(), BrowserCloseOutcome::Scheduled);
                    assert!(cancellation.check().is_err());
                    release.send(()).unwrap();
                }
                "shutdown" => {
                    let shutdown = scope.spawn(|| rig.host.shutdown());
                    let deadline = Instant::now() + Duration::from_secs(2);
                    while cancellation.check().is_ok() {
                        assert!(Instant::now() < deadline, "shutdown did not cancel");
                        std::thread::yield_now();
                    }
                    release.send(()).unwrap();
                    shutdown.join().unwrap().unwrap();
                }
                "panic" => {
                    rig.page.lock().unwrap().fault = Fault::Panic;
                    release.send(()).unwrap();
                }
                _ => unreachable!(),
            }
            let result = read.join();
            if way == "panic" {
                assert!(result.is_err());
            } else {
                assert_eq!(
                    result.unwrap().unwrap_err().browser_reason,
                    Some(BrowserUnavailableReason::Closed)
                );
            }
            rig.cleaned();
            assert_eq!(rig.folder().profile().exists(), way != "forget");
            for path in [
                rig.folder().active_lock(),
                rig.folder().profile_lock(),
                rig.folder().root().to_path_buf(),
            ] {
                File::open(path).unwrap().try_lock_exclusive().unwrap();
            }
            assert!(rig
                .page
                .lock()
                .unwrap()
                .commands
                .iter()
                .any(|(method, _, _)| method == "Browser.close"));
        });
        if way != "shutdown" {
            rig.next_peer();
            rig.page.lock().unwrap().fault = Fault::None;
            rig.host.read(&request(true))?;
        }
    }
    Ok(())
}

#[test]
fn saved_watch_forget_shares_fifteen_second_budget() -> TestResult {
    let rig = Rig::new()?;
    let (entered, release) = rig.block();
    let (finished, receive) = mpsc::channel();
    std::thread::scope(|scope| {
        let read = scope.spawn(|| {
            let result = rig.host.read(&request(true));
            finished.send(()).unwrap();
            result
        });
        entered.recv_timeout(Duration::from_secs(5)).unwrap();
        replace(&rig.folder().profile_lock());
        let replacement = File::open(rig.folder().profile_lock()).unwrap();
        replacement.try_lock_exclusive().unwrap();
        let clock = ClosingClock {
            rig: &rig,
            release: Mutex::new(Some(release)),
            finished: Mutex::new(receive),
            first_advance: 9900,
        };
        let started = clock.now_ms();
        let report = forget(&rig, &clock);
        assert_eq!(report.stage, Some(ForgetStage::ProfileLockBusy));
        assert_eq!(clock.now_ms() - started, 15000);
        assert!(rig.folder().profile().exists());
        assert_eq!(
            read.join().unwrap().unwrap_err().browser_reason,
            Some(BrowserUnavailableReason::Closed)
        );
    });
    Ok(())
}

#[test]
fn watch_start_keeps_profile_when_lock_files_are_replaced() -> TestResult {
    let rig = Rig::new()?;
    fs::create_dir_all(rig.folder().root())?;
    fs::set_permissions(rig.folder().root(), fs::Permissions::from_mode(0o700))?;
    let directory = File::open(rig.folder().root())?;
    directory.try_lock_exclusive()?;
    let (waiting, release) = pause_at(&rig.pause);
    std::thread::scope(|scope| {
        let read = scope.spawn(|| rig.host.read(&request(true)));
        waiting.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(rig.facts.lock().unwrap().spawns.is_empty());
        FileExt::unlock(&directory).unwrap();
        replace(&rig.folder().active_lock());
        replace(&rig.folder().profile_lock());
        let report = forget(&rig, rig.clock.as_ref());
        assert_eq!(report.stage, Some(ForgetStage::ProfileDirectoryBusy));
        assert!(rig.folder().profile().exists());
        release.send(()).unwrap();
        read.join().unwrap().unwrap();
        rig.cleaned();
    });
    Ok(())
}
