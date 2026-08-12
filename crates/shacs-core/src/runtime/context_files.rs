#[path = "context_files/discovery.rs"]
mod discovery;
#[path = "context_files/reader.rs"]
mod reader;
#[path = "context_files/types.rs"]
mod types;

pub use discovery::discover_context_files;
pub use types::*;

#[cfg(test)]
#[path = "context_files/tests.rs"]
mod tests;
