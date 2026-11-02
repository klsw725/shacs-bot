//! Search index over the workspace memory files, with vectors from an embedding model run in
//! this process (Spec 047).

mod candle_embedder;
mod embedder;
mod fake_embedder;
mod memory_index;
mod memory_search_paths;
mod memory_snapshot;
mod model_installer;
mod model_loader;
mod model_manifest;
mod query_terms;
mod search;
mod search_ranking;
mod search_result;
mod search_settings;
mod verified_model;

pub use candle_embedder::CandleEmbedder;
pub use embedder::{Embedder, EmbeddingError, EmbeddingKind};
pub use fake_embedder::{FakeEmbedder, FakeEmbedding, FakeLoader};
pub use memory_index::{
    IndexError, IndexModel, IndexStatus, IndexedItem, MemoryIndex, ReconcileError, VectorProgress,
    INDEX_SCHEMA_VERSION, VECTOR_BATCH_LIMIT,
};
pub use memory_search_paths::{workspace_digest, MemorySearchPaths};
pub use memory_snapshot::{
    read_memory_snapshot, search_text, ItemSource, MemoryItem, MemorySnapshot, SnapshotError,
    CHUNK_RULE_VERSION,
};
pub use model_installer::{
    install_model, HuggingFaceSource, InstallError, InstallReport, InstalledFile, ModelFileSource,
};
pub use model_loader::{EmbedderLoader, LazyEmbedder, LoadError, VerifiedModelLoader};
pub use model_manifest::{
    model_manifest, ModelArchitecture, ModelFile, ModelManifest, Pooling, DEFAULT_MEMORY_MODEL_ID,
    MODEL_MANIFESTS,
};
pub use query_terms::{query_terms, MAX_QUERY_TERMS};
pub use search::{MemorySearch, DEFAULT_SEARCH_RESULTS, MAX_SEARCH_RESULTS};
pub use search_ranking::{tie_order, RANKED_LIST_LIMIT};
pub use search_settings::{configured_model, MemorySearchSettings, MIN_RESULT_CHARS};
pub use verified_model::{read_verified_model, ModelFileError, VerifiedModel};
