use super::source_manifest::sha256_bytes;
use crate::{parse_cargo_test_counts, Spec031ReleaseCommandRecord, Spec031ReleaseTestCounts};
use serde::{Deserialize, Serialize};
use std::fmt::{Display, Formatter};
use std::path::{Path, PathBuf};

pub const SPEC030_BWRAP_RECORD_SCHEMA: &str = "spec030.bwrap_record.v4";
pub const BWRAP_TEST_NAME: &str = "real_bwrap_lane_runs_only_when_required";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Spec030BwrapRecord {
    pub schema: String,
    pub source_digest: String,
    pub platform: Spec030BwrapPlatform,
    pub producer: Spec030BwrapProducer,
    #[serde(skip)]
    pub observed_tests: Spec031ReleaseTestCounts,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Spec030BwrapProducer {
    pub producer_id: String,
    pub command_id: String,
    pub command_sha256: String,
    pub image_digest: String,
    pub pid: u32,
    pub argv: Vec<String>,
    pub exit_code: i32,
    pub stdout_sha256: String,
    pub stderr_sha256: String,
    pub stdout_temp_path: String,
    pub stderr_temp_path: String,
    pub transcript_sha256: String,
}

pub(crate) struct TrustedBwrapExecution<'a> {
    pub source_digest: &'a str,
    pub image_digest: &'a str,
    pub command: &'a Spec031ReleaseCommandRecord,
    pub stdout: &'a [u8],
    pub stderr: &'a [u8],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Spec030BwrapPlatform {
    Linux,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Spec030BwrapRecordError {
    InvalidPath,
    Malformed,
    Failed,
    ZeroTests,
}

impl Display for Spec030BwrapRecordError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for Spec030BwrapRecordError {}

pub(crate) fn build_spec030_bwrap_record(
    execution: TrustedBwrapExecution<'_>,
) -> Result<Spec030BwrapRecord, Spec030BwrapRecordError> {
    let receipt = execution
        .command
        .process_receipt
        .as_ref()
        .ok_or(Spec030BwrapRecordError::Malformed)?;
    let mut producer = Spec030BwrapProducer {
        producer_id: "spec030-linux-runner-v1".to_owned(),
        command_id: execution.command.id.clone(),
        command_sha256: String::new(),
        image_digest: execution.image_digest.to_owned(),
        pid: receipt.pid,
        argv: execution.command.argv.clone(),
        exit_code: execution.command.exit_code.unwrap_or(-1),
        stdout_sha256: sha256_bytes(execution.stdout),
        stderr_sha256: sha256_bytes(execution.stderr),
        stdout_temp_path: receipt.stdout_temp_path.clone(),
        stderr_temp_path: receipt.stderr_temp_path.clone(),
        transcript_sha256: String::new(),
    };
    producer.command_sha256 = command_hash(&producer)?;
    producer.transcript_sha256 = transcript_hash(execution.source_digest, &producer)?;
    Ok(Spec030BwrapRecord {
        schema: SPEC030_BWRAP_RECORD_SCHEMA.to_owned(),
        source_digest: execution.source_digest.to_owned(),
        platform: Spec030BwrapPlatform::Linux,
        producer,
        observed_tests: Spec031ReleaseTestCounts {
            tests_run: 0,
            tests_failed: 0,
        },
    })
}

pub fn validate_spec030_bwrap_record(
    path: &Path,
) -> Result<Spec030BwrapRecord, Spec030BwrapRecordError> {
    require_file(path)?;
    let bytes = std::fs::read(path).map_err(|_| Spec030BwrapRecordError::InvalidPath)?;
    let mut record = serde_json::from_slice::<Spec030BwrapRecord>(&bytes)
        .map_err(|_| Spec030BwrapRecordError::Malformed)?;
    let stdout =
        std::fs::read(stdout_path(path)).map_err(|_| Spec030BwrapRecordError::InvalidPath)?;
    let stderr =
        std::fs::read(stderr_path(path)).map_err(|_| Spec030BwrapRecordError::InvalidPath)?;
    if record.schema != SPEC030_BWRAP_RECORD_SCHEMA
        || !valid_sha256(&record.source_digest)
        || !valid_sha256(&record.producer.image_digest)
        || record.producer.producer_id != "spec030-linux-runner-v1"
        || record.producer.command_id != "spec030-bwrap-active"
        || record.producer.command_sha256 != command_hash(&record.producer)?
        || record.producer.pid == 0
        || record.producer.argv != producer_argv()
        || record.producer.stdout_sha256 != sha256_bytes(&stdout)
        || record.producer.stderr_sha256 != sha256_bytes(&stderr)
        || record.producer.transcript_sha256
            != transcript_hash(&record.source_digest, &record.producer)?
    {
        return Err(Spec030BwrapRecordError::Malformed);
    }
    if record.producer.exit_code != 0 {
        return Err(Spec030BwrapRecordError::Failed);
    }
    let tests = parse_cargo_test_counts(
        std::str::from_utf8(&stdout).map_err(|_| Spec030BwrapRecordError::Malformed)?,
    )
    .ok_or(Spec030BwrapRecordError::Malformed)?;
    if tests.tests_failed > 0 {
        return Err(Spec030BwrapRecordError::Failed);
    }
    if tests.tests_run == 0 {
        return Err(Spec030BwrapRecordError::ZeroTests);
    }
    record.observed_tests = tests;
    Ok(record)
}

pub(super) fn stdout_path(path: &Path) -> PathBuf {
    path.with_extension("stdout")
}

pub(super) fn stderr_path(path: &Path) -> PathBuf {
    path.with_extension("stderr")
}

fn require_file(path: &Path) -> Result<(), Spec030BwrapRecordError> {
    let metadata =
        std::fs::symlink_metadata(path).map_err(|_| Spec030BwrapRecordError::InvalidPath)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(Spec030BwrapRecordError::InvalidPath);
    }
    Ok(())
}

fn transcript_hash(
    source_digest: &str,
    producer: &Spec030BwrapProducer,
) -> Result<String, Spec030BwrapRecordError> {
    let value = serde_json::json!({
        "source_digest": source_digest,
        "producer_id": producer.producer_id,
        "command_id": producer.command_id,
        "command_sha256": producer.command_sha256,
        "image_digest": producer.image_digest,
        "pid": producer.pid,
        "argv": producer.argv,
        "exit_code": producer.exit_code,
        "stdout_sha256": producer.stdout_sha256,
        "stderr_sha256": producer.stderr_sha256,
        "stdout_temp_path": producer.stdout_temp_path,
        "stderr_temp_path": producer.stderr_temp_path,
    });
    let bytes = serde_json::to_vec(&value).map_err(|_| Spec030BwrapRecordError::Malformed)?;
    Ok(sha256_bytes(&bytes))
}

fn command_hash(producer: &Spec030BwrapProducer) -> Result<String, Spec030BwrapRecordError> {
    let bytes = serde_json::to_vec(&serde_json::json!({
        "command_id":producer.command_id,"pid":producer.pid,"argv":producer.argv,
        "exit_code":producer.exit_code,"stdout_sha256":producer.stdout_sha256,
        "stderr_sha256":producer.stderr_sha256,"stdout_temp_path":producer.stdout_temp_path,
        "stderr_temp_path":producer.stderr_temp_path
    }))
    .map_err(|_| Spec030BwrapRecordError::Malformed)?;
    Ok(sha256_bytes(&bytes))
}

fn valid_sha256(value: &str) -> bool {
    value
        .strip_prefix("sha256:")
        .is_some_and(|hex| hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit()))
}

fn producer_argv() -> Vec<String> {
    [
        "env",
        "SHACS_REQUIRE_BWRAP=1",
        "cargo",
        "test",
        "--manifest-path",
        "crates/Cargo.toml",
        "-p",
        "shacs-core",
        "--test",
        "spec030_sandbox_adapter",
        BWRAP_TEST_NAME,
    ]
    .into_iter()
    .map(str::to_owned)
    .collect()
}

#[cfg(test)]
#[path = "bwrap_provenance_tests.rs"]
mod tests;
