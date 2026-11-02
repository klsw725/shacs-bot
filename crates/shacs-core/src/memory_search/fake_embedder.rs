use sha2::{Digest, Sha256};
use std::sync::Arc;

use super::embedder::{Embedder, EmbeddingError, EmbeddingKind};
use super::memory_index::IndexModel;
use super::model_loader::{EmbedderLoader, LoadError};

/// What a [`FakeEmbedder`] returns, for tests that run without a real model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FakeEmbedding {
    /// A deterministic unit vector built from the character trigrams of the text.
    Hashed,
    /// Every call fails.
    Failing,
    /// Vectors with a NaN.
    NotFinite,
    /// All-zero vectors.
    Zero,
    /// Vectors one dimension short.
    WrongDimension,
}

/// A stand-in model for tests. Production code never constructs one.
#[derive(Debug, Clone)]
pub struct FakeEmbedder {
    model_id: String,
    revision: String,
    dimension: usize,
    embedding: FakeEmbedding,
}

impl FakeEmbedder {
    pub fn new(model_id: &str, revision: &str, dimension: usize, embedding: FakeEmbedding) -> Self {
        Self {
            model_id: model_id.to_owned(),
            revision: revision.to_owned(),
            dimension,
            embedding,
        }
    }

    fn vector(&self, text: &str) -> Vec<f32> {
        match self.embedding {
            FakeEmbedding::Hashed | FakeEmbedding::Failing => hashed_vector(text, self.dimension),
            FakeEmbedding::NotFinite => {
                let mut vector = hashed_vector(text, self.dimension);
                if let Some(first) = vector.first_mut() {
                    *first = f32::NAN;
                }
                vector
            }
            FakeEmbedding::Zero => vec![0.0; self.dimension],
            FakeEmbedding::WrongDimension => {
                hashed_vector(text, self.dimension.saturating_sub(1).max(1))
            }
        }
    }
}

impl Embedder for FakeEmbedder {
    fn model_id(&self) -> &str {
        &self.model_id
    }

    fn revision(&self) -> &str {
        &self.revision
    }

    fn dimension(&self) -> usize {
        self.dimension
    }

    fn embed(&self, _kind: EmbeddingKind, texts: &[&str]) -> Result<Vec<Vec<f32>>, EmbeddingError> {
        if self.embedding == FakeEmbedding::Failing {
            return Err(EmbeddingError::Compute("fake embedding failure".to_owned()));
        }
        Ok(texts.iter().map(|text| self.vector(text)).collect())
    }
}

pub struct FakeLoader {
    embedder: FakeEmbedder,
    fails: bool,
}

impl FakeLoader {
    pub fn new(embedder: FakeEmbedder) -> Self {
        Self {
            embedder,
            fails: false,
        }
    }

    pub fn failing(embedder: FakeEmbedder) -> Self {
        Self {
            embedder,
            fails: true,
        }
    }
}

impl EmbedderLoader for FakeLoader {
    fn model(&self) -> IndexModel {
        IndexModel::of(&self.embedder)
    }

    fn load(&self) -> Result<Arc<dyn Embedder>, LoadError> {
        if self.fails {
            return Err(LoadError::Model(EmbeddingError::Load(
                "fake load failure".to_owned(),
            )));
        }
        Ok(Arc::new(self.embedder.clone()))
    }
}

/// Each character trigram of `text` adds one to the dimension its SHA-256 picks; the sum is
/// scaled to length 1. Texts sharing trigrams point in similar directions.
fn hashed_vector(text: &str, dimension: usize) -> Vec<f32> {
    let characters: Vec<char> = text.chars().collect();
    let slots = dimension.max(1);
    let mut vector = vec![0.0_f32; slots];
    for window in characters.windows(3.min(characters.len()).max(1)) {
        let trigram: String = window.iter().collect();
        let digest = Sha256::digest(trigram.as_bytes());
        let slot = u64::from_le_bytes(digest[..8].try_into().unwrap_or([0; 8]));
        vector[(slot % slots as u64) as usize] += 1.0;
    }
    let norm = vector.iter().map(|value| value * value).sum::<f32>().sqrt();
    if norm > 0.0 {
        for value in &mut vector {
            *value /= norm;
        }
    }
    vector
}
