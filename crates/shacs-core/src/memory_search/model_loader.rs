use std::fmt;
use std::sync::{Arc, Mutex};

use super::candle_embedder::CandleEmbedder;
use super::embedder::{Embedder, EmbeddingError};
use super::memory_index::IndexModel;
use super::memory_search_paths::MemorySearchPaths;
use super::model_manifest::ModelManifest;
use super::verified_model::{self, ModelFileError};

#[derive(Debug)]
pub enum LoadError {
    File(ModelFileError),
    Model(EmbeddingError),
}

impl fmt::Display for LoadError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::File(error) => error.fmt(formatter),
            Self::Model(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for LoadError {}

pub trait EmbedderLoader: Send + Sync {
    /// The model `load` builds, known without building it.
    fn model(&self) -> IndexModel;
    fn load(&self) -> Result<Arc<dyn Embedder>, LoadError>;
}

pub struct VerifiedModelLoader {
    paths: MemorySearchPaths,
    manifest: &'static ModelManifest,
}

impl VerifiedModelLoader {
    pub fn new(paths: MemorySearchPaths, manifest: &'static ModelManifest) -> Self {
        Self { paths, manifest }
    }
}

impl EmbedderLoader for VerifiedModelLoader {
    fn model(&self) -> IndexModel {
        IndexModel::from_manifest(self.manifest)
    }

    fn load(&self) -> Result<Arc<dyn Embedder>, LoadError> {
        let model = verified_model::read_verified_model(&self.paths, self.manifest)
            .map_err(LoadError::File)?;
        let embedder = CandleEmbedder::load(&model).map_err(LoadError::Model)?;
        Ok(Arc::new(embedder))
    }
}

/// Builds the model the first time it is needed and gives every later caller in this process
/// the same one. A failed build is not kept, so a model installed later is picked up.
pub struct LazyEmbedder {
    loader: Arc<dyn EmbedderLoader>,
    loaded: Mutex<Option<Arc<dyn Embedder>>>,
}

impl LazyEmbedder {
    pub fn new(loader: Arc<dyn EmbedderLoader>) -> Self {
        Self {
            loader,
            loaded: Mutex::new(None),
        }
    }

    pub fn model(&self) -> IndexModel {
        self.loader.model()
    }

    /// The lock is held while building, so callers arriving meanwhile wait for that build
    /// instead of starting another.
    pub fn get(&self) -> Result<Arc<dyn Embedder>, LoadError> {
        let Ok(mut loaded) = self.loaded.lock() else {
            return Err(LoadError::Model(EmbeddingError::Load(
                "an earlier load of this model panicked".to_owned(),
            )));
        };
        if let Some(embedder) = loaded.as_ref() {
            return Ok(Arc::clone(embedder));
        }
        let embedder = self.loader.load()?;
        *loaded = Some(Arc::clone(&embedder));
        Ok(embedder)
    }
}
