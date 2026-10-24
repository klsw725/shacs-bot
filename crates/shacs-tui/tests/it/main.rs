//! Single integration test binary for this crate.
//!
//! Each test module here was a separate `tests/*.rs` target; `#[path]` keeps
//! its original module layout. Declare a support file used by several modules
//! here once and import it with `use crate::...` (clippy rejects loading one
//! file as several modules). Add new integration tests here as modules instead
//! of new `tests/*.rs` files, which each link a separate test binary.

#[path = "approval_terminal.rs"]
pub mod approval_terminal;
#[path = "recent_retry_gates.rs"]
pub mod recent_retry_gates;
#[path = "../support/recent_retry.rs"]
pub mod recent_retry_support;
#[path = "recent_retry_terminal.rs"]
pub mod recent_retry_terminal;
#[path = "spec031_tui_layout.rs"]
pub mod spec031_tui_layout;
#[path = "spec031_tui_live.rs"]
pub mod spec031_tui_live;
#[path = "spec031_tui_workflow.rs"]
pub mod spec031_tui_workflow;
#[path = "spec034_media.rs"]
pub mod spec034_media;
#[path = "spec035_revised_adapter.rs"]
pub mod spec035_revised_adapter;
#[path = "spec035_tasks_live.rs"]
pub mod spec035_tasks_live;
#[path = "spec035_tasks_view.rs"]
pub mod spec035_tasks_view;
