use serde::{Deserialize, Serialize};
use shacs_projection::{
    Spec031ReleaseCommandRecord, Spec031ReleaseCommandStatus, Spec031ReleaseGateKind,
    Spec031ReleaseTestCounts,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PortableProcessReceipt {
    pub reaped: bool,
    pub temp_paths_published: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PortableToolIdentity {
    pub name: String,
    pub version: String,
    pub executable_digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommandStreamSummary {
    pub schema: String,
    pub byte_count: u64,
    pub digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PortableCommandRecord {
    pub id: String,
    pub gate: Spec031ReleaseGateKind,
    pub package: Option<String>,
    pub filter: Option<String>,
    pub argv: Vec<String>,
    pub cwd: String,
    pub status: Spec031ReleaseCommandStatus,
    pub exit_code: Option<i32>,
    pub stdout_path: String,
    pub stderr_path: String,
    pub tests: Option<Spec031ReleaseTestCounts>,
}

impl From<Spec031ReleaseCommandRecord> for PortableCommandRecord {
    fn from(command: Spec031ReleaseCommandRecord) -> Self {
        Self {
            id: command.id,
            gate: command.gate,
            package: command.package,
            filter: command.filter,
            argv: command.argv,
            cwd: command.cwd,
            status: command.status,
            exit_code: command.exit_code,
            stdout_path: command.stdout_path,
            stderr_path: command.stderr_path,
            tests: command.tests,
        }
    }
}
