use super::command_process::{configure_process_group, wait_status_with_timeout};
use super::model::{
    Spec031ReleaseArtifactError, Spec031ReleaseCommandRecord, Spec031ReleaseCommandSpec,
    Spec031ReleaseCommandStatus, Spec031ReleaseTestCounts,
};
use crate::release_evidence::EvidenceWriter;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

pub fn execute_spec031_release_command(
    spec: &Spec031ReleaseCommandSpec,
    output_dir: &Path,
) -> Result<Spec031ReleaseCommandRecord, Spec031ReleaseArtifactError> {
    let writer =
        EvidenceWriter::open_existing(output_dir).map_err(|_| Spec031ReleaseArtifactError::Io)?;
    execute_command(&writer, spec, "")
}

pub(crate) fn execute_spec031_release_command_with(
    writer: &EvidenceWriter,
    spec: &Spec031ReleaseCommandSpec,
) -> Result<Spec031ReleaseCommandRecord, Spec031ReleaseArtifactError> {
    execute_command(writer, spec, "commands/")
}

fn execute_command(
    writer: &EvidenceWriter,
    spec: &Spec031ReleaseCommandSpec,
    record_prefix: &str,
) -> Result<Spec031ReleaseCommandRecord, Spec031ReleaseArtifactError> {
    let (program, args) = spec
        .argv
        .split_first()
        .ok_or(Spec031ReleaseArtifactError::EmptyCommand)?;
    validate_command_id(&spec.id)?;
    let stdout_path = format!("{record_prefix}{}.stdout", spec.id);
    let stderr_path = format!("{record_prefix}{}.stderr", spec.id);
    let stdout_temp = writer
        .create_temp_file(Path::new(&stdout_path))
        .map_err(|_| Spec031ReleaseArtifactError::Io)?;
    let stderr_temp = writer
        .create_temp_file(Path::new(&stderr_path))
        .map_err(|_| Spec031ReleaseArtifactError::Io)?;
    let stdout_temp_path = stdout_temp.path().to_path_buf();
    let stderr_temp_path = stderr_temp.path().to_path_buf();
    let start = Instant::now();
    let mut command = Command::new(program);
    command
        .args(args)
        .current_dir(&spec.cwd)
        .stdout(Stdio::from(stdout_temp.into_std()))
        .stderr(Stdio::from(stderr_temp.into_std()));
    configure_process_group(&mut command);
    let mut child = command
        .spawn()
        .map_err(|_| Spec031ReleaseArtifactError::Io)?;
    let pid = child.id();
    let (timed_out, exit_status) = wait_status_with_timeout(&mut child, spec.timeout)?;
    let status = command_status(timed_out, exit_status.success());
    let stdout = writer
        .read_to_string(&stdout_temp_path)
        .map_err(|_| Spec031ReleaseArtifactError::Io)?;
    writer
        .publish_temp(&stdout_temp_path, Path::new(&stdout_path))
        .map_err(|_| Spec031ReleaseArtifactError::Io)?;
    writer
        .publish_temp(&stderr_temp_path, Path::new(&stderr_path))
        .map_err(|_| Spec031ReleaseArtifactError::Io)?;
    let tests = if is_cargo_test_command(&spec.argv) {
        parse_cargo_test_counts(&stdout)
    } else {
        None
    };
    Ok(Spec031ReleaseCommandRecord {
        id: spec.id.clone(),
        gate: spec.gate,
        package: spec.package.clone(),
        filter: spec.filter.clone(),
        argv: spec.argv.clone(),
        cwd: spec.cwd.display().to_string(),
        status,
        exit_code: exit_status.code(),
        duration_ms: millis_u64(start.elapsed()),
        stdout_path,
        stderr_path,
        tests,
        process_receipt: Some(super::model::Spec031CommandProcessReceipt {
            pid,
            reaped: true,
            stdout_temp_path: stdout_temp_path.display().to_string(),
            stderr_temp_path: stderr_temp_path.display().to_string(),
            temp_paths_published: true,
        }),
    })
}

pub fn parse_cargo_test_counts(output: &str) -> Option<Spec031ReleaseTestCounts> {
    parse_cargo_test_counts_strict(output).ok()
}

pub fn parse_cargo_test_counts_strict(
    output: &str,
) -> Result<Spec031ReleaseTestCounts, Spec031ReleaseArtifactError> {
    let mut counts = Spec031ReleaseTestCounts {
        tests_run: 0,
        tests_failed: 0,
    };
    let mut found = false;
    for line in output.lines() {
        if !line.trim_start().starts_with("test result:") {
            continue;
        }
        let summary = parse_cargo_test_line(line)?;
        found = true;
        counts.tests_run += summary.tests_run;
        counts.tests_failed += summary.tests_failed;
    }
    if !found {
        return Err(Spec031ReleaseArtifactError::InvalidCommandEvidence);
    }
    Ok(counts)
}

fn parse_cargo_test_line(
    line: &str,
) -> Result<Spec031ReleaseTestCounts, Spec031ReleaseArtifactError> {
    let summary = line
        .trim()
        .strip_prefix("test result: ")
        .ok_or(Spec031ReleaseArtifactError::InvalidCommandEvidence)?;
    let (status, rest) = summary
        .split_once(". ")
        .ok_or(Spec031ReleaseArtifactError::InvalidCommandEvidence)?;
    if !matches!(status, "ok" | "FAILED") {
        return Err(Spec031ReleaseArtifactError::InvalidCommandEvidence);
    }
    let parts: Vec<&str> = rest.split("; ").collect();
    if parts.len() != 6 {
        return Err(Spec031ReleaseArtifactError::InvalidCommandEvidence);
    }
    let passed = exact_count(parts[0], " passed")?;
    let failed = exact_count(parts[1], " failed")?;
    exact_count(parts[2], " ignored")?;
    exact_count(parts[3], " measured")?;
    exact_count(parts[4], " filtered out")?;
    if !parts[5].starts_with("finished in ") || !parts[5].ends_with('s') {
        return Err(Spec031ReleaseArtifactError::InvalidCommandEvidence);
    }
    let status_matches_counts = matches!((status, failed), ("ok", 0) | ("FAILED", 1..));
    if !status_matches_counts {
        return Err(Spec031ReleaseArtifactError::InvalidCommandEvidence);
    }
    Ok(Spec031ReleaseTestCounts {
        tests_run: passed + failed,
        tests_failed: failed,
    })
}

fn command_status(timed_out: bool, success: bool) -> Spec031ReleaseCommandStatus {
    match (timed_out, success) {
        (true, _) => Spec031ReleaseCommandStatus::TimedOut,
        (false, true) => Spec031ReleaseCommandStatus::Passed,
        (false, false) => Spec031ReleaseCommandStatus::Failed,
    }
}

fn validate_command_id(id: &str) -> Result<(), Spec031ReleaseArtifactError> {
    let safe = !id.is_empty()
        && id.len() <= 80
        && id
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'));
    if safe {
        Ok(())
    } else {
        Err(Spec031ReleaseArtifactError::InvalidArtifactPath)
    }
}

fn is_cargo_test_command(argv: &[String]) -> bool {
    matches!(argv, [program, subcommand, ..] if program == "cargo" && subcommand == "test")
}

fn exact_count(text: &str, suffix: &str) -> Result<u64, Spec031ReleaseArtifactError> {
    let digits = text
        .strip_suffix(suffix)
        .ok_or(Spec031ReleaseArtifactError::InvalidCommandEvidence)?;
    if digits.is_empty() || !digits.chars().all(|character| character.is_ascii_digit()) {
        return Err(Spec031ReleaseArtifactError::InvalidCommandEvidence);
    }
    digits
        .parse()
        .map_err(|_| Spec031ReleaseArtifactError::InvalidCommandEvidence)
}

fn millis_u64(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}
