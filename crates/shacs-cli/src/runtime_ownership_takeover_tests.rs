//! Spec 052 PRD 060 acceptance 3: starting runtime takes over a fresh
//! marker that an earlier process with its own pid left (PID 1 of a
//! re-created container) only with evidence that it started after that
//! marker. A fake procfs gives that evidence on machines without `/proc`.

use super::{
    acquire_runtime_ownership_marker_with_proc_root, clock_ticks_per_second, now_millis,
    read_runtime_ownership_marker, runtime_durable_event_root, runtime_ownership_marker_path,
    runtime_ownership_marker_value, runtime_stop_request_marker_path,
    write_runtime_marker_atomically, DurableEventStore, RUNTIME_OWNER_LIFECYCLE,
};
use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};

type TestResult = Result<(), Box<dyn Error>>;

fn fake_procfs(root: &Path, pid: u32, started_at_ms: u64) -> Result<PathBuf, Box<dyn Error>> {
    let ticks = clock_ticks_per_second().ok_or("no clock ticks")?;
    let boot_seconds = started_at_ms / 1_000 - 100;
    let start_ticks = (started_at_ms - boot_seconds * 1_000) * ticks / 1_000;
    let proc_root = root.join("proc");
    let pid_root = proc_root.join(pid.to_string());
    fs::create_dir_all(&pid_root)?;
    fs::write(
        proc_root.join("stat"),
        format!("cpu 0 0 0 0\nbtime {boot_seconds}\n"),
    )?;
    let mut fields = vec!["S".to_owned()];
    fields.extend((0..18).map(|_| "0".to_owned()));
    fields.push(start_ticks.to_string());
    fs::write(
        pid_root.join("stat"),
        format!("{pid} (shacs-bot) {}", fields.join(" ")),
    )?;
    Ok(proc_root)
}

fn lifecycle_records(data: &Path) -> Result<usize, Box<dyn Error>> {
    Ok(DurableEventStore::open(runtime_durable_event_root(data))?
        .scan(10)?
        .records
        .iter()
        .filter(|record| record.kind == RUNTIME_OWNER_LIFECYCLE)
        .count())
}

#[test]
fn a_fresh_marker_with_this_pid_is_taken_over_only_with_later_start_evidence() -> TestResult {
    let pid = std::process::id();
    for (started_after_marker, takes_over) in [(true, true), (false, false)] {
        // Given
        let root = tempfile::tempdir()?;
        let data = root.path().join("data");
        let config = data.join("config.json");
        let workspace = data.join("workspace");
        let marker_path = runtime_ownership_marker_path(&data);
        let now = now_millis();
        let earlier_start = now - 60_000;
        let renewed_far_ahead = now + 3_600_000;
        let earlier = runtime_ownership_marker_value(
            pid,
            earlier_start,
            renewed_far_ahead,
            "run",
            &config,
            &workspace,
        );
        write_runtime_marker_atomically(&marker_path, &earlier)?;
        let before = fs::read(&marker_path)?;
        let process_start = if started_after_marker {
            earlier_start + 10_000
        } else {
            earlier_start - 10_000
        };
        let proc_root = fake_procfs(root.path(), pid, process_start)?;
        let new_start = now + 1;
        let new =
            runtime_ownership_marker_value(pid, new_start, new_start, "run", &config, &workspace);

        // When
        let result = acquire_runtime_ownership_marker_with_proc_root(
            &data,
            &marker_path,
            &new,
            &runtime_stop_request_marker_path(&data),
            &proc_root,
        );

        // Then
        if takes_over {
            result?;
            let current =
                read_runtime_ownership_marker(&marker_path)?.ok_or("the marker is missing")?;
            assert_eq!(current.pid, pid);
            assert_eq!(current.started_at_ms, new_start);
            assert_eq!(lifecycle_records(&data)?, 1);
        } else {
            let error = result
                .err()
                .ok_or("a fresh marker without the evidence must block")?
                .to_string();
            assert!(error.contains("runtime ownership"), "{error}");
            assert_eq!(fs::read(&marker_path)?, before);
            assert_eq!(lifecycle_records(&data)?, 0);
        }
    }
    Ok(())
}
