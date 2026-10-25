//! Single integration test binary for this crate.
//!
//! Each test module here was a separate `tests/*.rs` target; `#[path]` keeps
//! its original module layout. Declare a support file used by several modules
//! here once and import it with `use crate::...` (clippy rejects loading one
//! file as several modules). Add new integration tests here as modules instead
//! of new `tests/*.rs` files, which each link a separate test binary.

#[path = "common.rs"]
pub mod common;
#[path = "spec031_channel_projection.rs"]
pub mod spec031_channel_projection;
#[path = "spec035_capability_channel.rs"]
pub mod spec035_capability_channel;
#[path = "spec035_production_channel.rs"]
pub mod spec035_production_channel;
#[path = "spec035_revised_adapter.rs"]
pub mod spec035_revised_adapter;
