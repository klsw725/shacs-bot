//! Single integration test binary for this crate.
//!
//! Each test module here was a separate `tests/*.rs` target; `#[path]` keeps
//! its original module layout. Declare a support file used by several modules
//! here once and import it with `use crate::...` (clippy rejects loading one
//! file as several modules). Add new integration tests here as modules instead
//! of new `tests/*.rs` files, which each link a separate test binary.

#[path = "readiness_owner_parity.rs"]
pub mod readiness_owner_parity;
#[path = "remembered_permissions_cli.rs"]
pub mod remembered_permissions_cli;
#[path = "remembered_permissions_e2e.rs"]
pub mod remembered_permissions_e2e;
#[path = "spec031_activation_production_builder.rs"]
pub mod spec031_activation_production_builder;
#[path = "spec031_black_box_qa.rs"]
pub mod spec031_black_box_qa;
#[path = "spec031_owner_remediation.rs"]
pub mod spec031_owner_remediation;
#[path = "spec033_improvement_cli.rs"]
pub mod spec033_improvement_cli;
#[path = "spec033_trajectory_cli.rs"]
pub mod spec033_trajectory_cli;
#[path = "spec035_accounting_parity.rs"]
pub mod spec035_accounting_parity;
#[path = "spec035_revised_adapter.rs"]
pub mod spec035_revised_adapter;
#[path = "spec035_tasks_cli.rs"]
pub mod spec035_tasks_cli;
#[path = "spec035_tasks_fail_closed.rs"]
pub mod spec035_tasks_fail_closed;
#[path = "spec037_image_repl.rs"]
pub mod spec037_image_repl;
#[path = "spec037_image_verbose.rs"]
pub mod spec037_image_verbose;
#[path = "spec037_method_repl.rs"]
pub mod spec037_method_repl;
#[path = "spec038_learn_repl.rs"]
pub mod spec038_learn_repl;
#[path = "spec043_leftover_apps.rs"]
pub mod spec043_leftover_apps;
