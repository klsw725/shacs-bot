//! Single integration test binary for this crate.
//!
//! Each test module here was a separate `tests/*.rs` target; `#[path]` keeps
//! its original module layout. Declare a support file used by several modules
//! here once and import it with `use crate::...` (clippy rejects loading one
//! file as several modules). Add new integration tests here as modules instead
//! of new `tests/*.rs` files, which each link a separate test binary.

#[path = "remembered_permission_contract.rs"]
pub mod remembered_permission_contract;
#[path = "remembered_permission_store.rs"]
pub mod remembered_permission_store;
#[path = "remembered_permission_store_failures.rs"]
pub mod remembered_permission_store_failures;
#[path = "remembered_permission_store_support/mod.rs"]
pub mod remembered_permission_store_support;
#[path = "spec031_migration_cas.rs"]
pub mod spec031_migration_cas;
