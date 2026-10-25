//! Single integration test binary for this crate.
//!
//! Each test module here was a separate `tests/*.rs` target; `#[path]` keeps
//! its original module layout. Declare a support file used by several modules
//! here once and import it with `use crate::...` (clippy rejects loading one
//! file as several modules). Add new integration tests here as modules instead
//! of new `tests/*.rs` files, which each link a separate test binary.

#[path = "approval_terminal.rs"]
pub mod approval_terminal;
#[path = "spec033_improvement_api.rs"]
pub mod spec033_improvement_api;
#[path = "spec034_media_api.rs"]
pub mod spec034_media_api;
#[path = "spec035_accounting_api.rs"]
pub mod spec035_accounting_api;
#[path = "spec035_capability_api.rs"]
pub mod spec035_capability_api;
#[path = "spec035_reconnect_api.rs"]
pub mod spec035_reconnect_api;
#[path = "spec035_reconnect_support/mod.rs"]
pub mod spec035_reconnect_support;
#[path = "spec035_revised_adapter.rs"]
pub mod spec035_revised_adapter;
#[path = "spec035_tasks_api.rs"]
pub mod spec035_tasks_api;
#[path = "spec035_tasks_fail_closed.rs"]
pub mod spec035_tasks_fail_closed;
#[path = "spec035_websocket_query_api.rs"]
pub mod spec035_websocket_query_api;
#[path = "spec042_web_watch_api.rs"]
pub mod spec042_web_watch_api;
