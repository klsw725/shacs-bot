//! Single integration test binary for this crate.
//!
//! Each test module here was a separate `tests/*.rs` target; `#[path]` keeps
//! its original module layout. Declare a support file used by several modules
//! here once and import it with `use crate::...` (clippy rejects loading one
//! file as several modules). Add new integration tests here as modules instead
//! of new `tests/*.rs` files, which each link a separate test binary.

#[path = "anthropic.rs"]
pub mod anthropic;
#[path = "azure_openai.rs"]
pub mod azure_openai;
#[path = "codex.rs"]
pub mod codex;
#[path = "codex_error_security.rs"]
pub mod codex_error_security;
#[path = "image_generation_security.rs"]
pub mod image_generation_security;
#[path = "image_metadata_security.rs"]
pub mod image_metadata_security;
#[path = "openai_compatible.rs"]
pub mod openai_compatible;
#[path = "spec034_codex_media.rs"]
pub mod spec034_codex_media;
#[path = "spec034_image_operations.rs"]
pub mod spec034_image_operations;
#[path = "spec034_media_response.rs"]
pub mod spec034_media_response;
#[path = "transcription.rs"]
pub mod transcription;
