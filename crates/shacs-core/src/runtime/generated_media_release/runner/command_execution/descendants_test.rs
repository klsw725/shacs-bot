use super::*;
use std::path::PathBuf;

#[test]
fn recursive_observation_retains_descendant_after_it_escapes_parent_tree(
) -> Result<(), Spec034ReleaseArtifactError> {
    let mut tracker = DescendantTracker {
        root: 10,
        tracked: BTreeMap::new(),
        closed: false,
    };
    tracker.observe_with(
        |parent| match parent {
            10 => Ok(vec![20]),
            20 => Ok(vec![30]),
            _ => Ok(Vec::new()),
        },
        |_| Ok(()),
        |pid| Ok(identity(pid, 1)),
    )?;
    tracker.observe_with(
        |parent| match parent {
            10 => Ok(vec![20]),
            _ => Ok(Vec::new()),
        },
        |_| Ok(()),
        |pid| Ok(identity(pid, 1)),
    )?;

    assert_eq!(tracker.tracked.keys().copied().collect::<Vec<_>>(), vec![20, 30]);
    Ok(())
}

#[cfg(target_vendor = "apple")]
#[test]
fn observable_setsid_double_fork_is_retained_and_killed(
) -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let pid_path = root.path().join("escaped.pid");
    let script = r#"
import os, pathlib, signal, sys
pid_path = pathlib.Path(sys.argv[1])
first = os.fork()
if first == 0:
    os.setsid()
    second = os.fork()
    if second == 0:
        pid_path.write_text(str(os.getpid()))
        signal.pause()
    signal.pause()
os.waitpid(first, 0)
"#;
    let mut child = std::process::Command::new("/usr/bin/python3")
        .args(["-c", script])
        .arg(&pid_path)
        .spawn()?;
    let mut tracker = DescendantTracker::new(child.id())?;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while !pid_path.exists() {
        tracker.observe()?;
        if std::time::Instant::now() >= deadline {
            child.kill()?;
            return Err("escaped descendant was not observed".into());
        }
        std::thread::yield_now();
    }
    tracker.observe()?;
    for pid in tracker.tracked.keys().rev().copied() {
        signal(pid)?;
    }
    child.wait()?;
    tracker.terminate_and_verify()?;
    Ok(())
}

#[test]
fn pid_reuse_never_signals_the_replacement() {
    let tracker = DescendantTracker {
        root: 10,
        tracked: BTreeMap::from([(20, identity(20, 1))]),
        closed: false,
    };
    let mut signalled = Vec::new();

    let result = tracker.terminate_with(
        |pid| Ok(identity(pid, 2)),
        |pid| {
            signalled.push(pid);
            Ok(())
        },
        |_| Ok(true),
    );

    assert!(matches!(result, Err(Spec034ReleaseArtifactError::CombinedFailure { .. })));
    assert!(signalled.is_empty());
}

#[test]
fn every_descendant_cleanup_is_attempted_after_failure() {
    let tracker = DescendantTracker {
        root: 10,
        tracked: BTreeMap::from([(20, identity(20, 1)), (30, identity(30, 1))]),
        closed: false,
    };
    let mut attempts = Vec::new();

    let result = tracker.terminate_with(
        |pid| Ok(identity(pid, 1)),
        |pid| {
            attempts.push(pid);
            Err(Spec034ReleaseArtifactError::CommandFailed)
        },
        |_| Ok(false),
    );

    assert!(result.is_err());
    assert_eq!(attempts, vec![30, 20]);
}

#[test]
fn child_exit_between_listing_and_identity_capture_is_skipped() {
    let mut tracker = DescendantTracker {
        root: 10,
        tracked: BTreeMap::new(),
        closed: false,
    };

    let result = tracker.observe_with(
        |parent| Ok(if parent == 10 { vec![20] } else { Vec::new() }),
        |_| Ok(()),
        |_| {
            Err(Spec034ReleaseArtifactError::Io(
                std::io::Error::from_raw_os_error(libc::ESRCH),
            ))
        },
    );

    assert!(result.is_ok());
    assert!(tracker.tracked.is_empty());
}

fn identity(pid: i32, start: u64) -> ProcessIdentity {
    ProcessIdentity {
        pid,
        parent_pid: 0,
        start_seconds: start,
        start_microseconds: 0,
        executable: PathBuf::from("/test"),
        device: 1,
        inode: 1,
        digest: "sha256:test".to_owned(),
        cdhash: vec![1],
    }
}
