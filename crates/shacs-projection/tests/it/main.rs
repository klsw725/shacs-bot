//! Single integration test binary for this crate.
//!
//! Each test module here was a separate `tests/*.rs` target; `#[path]` keeps
//! its original module layout. Declare a support file used by several modules
//! here once and import it with `use crate::...` (clippy rejects loading one
//! file as several modules). Add new integration tests here as modules instead
//! of new `tests/*.rs` files, which each link a separate test binary.

#[path = "remembered_permission_projection.rs"]
pub mod remembered_permission_projection;
#[path = "spec031_baseline.rs"]
pub mod spec031_baseline;
#[path = "spec031_command_process.rs"]
pub mod spec031_command_process;
#[path = "spec031_extension.rs"]
pub mod spec031_extension;
#[path = "spec031_external_owner.rs"]
pub mod spec031_external_owner;
#[path = "spec031_external_owner_artifacts.rs"]
pub mod spec031_external_owner_artifacts;
#[path = "spec031_external_refs.rs"]
pub mod spec031_external_refs;
#[path = "spec031_fixtures.rs"]
pub mod spec031_fixtures;
#[path = "spec031_lifecycle.rs"]
pub mod spec031_lifecycle;
#[path = "spec031_model.rs"]
pub mod spec031_model;
#[path = "spec031_readiness.rs"]
pub mod spec031_readiness;
#[path = "spec031_readiness_duplicates.rs"]
pub mod spec031_readiness_duplicates;
#[path = "spec031_readiness_failures.rs"]
pub mod spec031_readiness_failures;
#[path = "spec031_readiness_truth_table.rs"]
pub mod spec031_readiness_truth_table;
#[path = "spec031_reason_code.rs"]
pub mod spec031_reason_code;
#[path = "spec031_redaction.rs"]
pub mod spec031_redaction;
#[path = "spec031_redaction_boundaries.rs"]
pub mod spec031_redaction_boundaries;
#[path = "spec031_release_runner_cli.rs"]
pub mod spec031_release_runner_cli;
#[path = "spec031_release_runner_schema.rs"]
pub mod spec031_release_runner_schema;
#[path = "spec031_release_runner_triage.rs"]
pub mod spec031_release_runner_triage;
#[path = "spec033_snapshot.rs"]
pub mod spec033_snapshot;
#[path = "spec035_capability_matrix.rs"]
pub mod spec035_capability_matrix;
#[path = "spec035_reconnect_accounting.rs"]
pub mod spec035_reconnect_accounting;
#[path = "spec035_revised_characterization.rs"]
pub mod spec035_revised_characterization;
#[path = "spec035_revised_contract.rs"]
pub mod spec035_revised_contract;
#[path = "spec035_revised_owner.rs"]
pub mod spec035_revised_owner;
#[path = "spec035_tasks_model.rs"]
pub mod spec035_tasks_model;
#[path = "spec035_transport.rs"]
pub mod spec035_transport;
