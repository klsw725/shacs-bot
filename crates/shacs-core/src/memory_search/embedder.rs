use std::fmt;

/// Whether a text is a question or a memory item; models prefix them differently.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmbeddingKind {
    Query,
    Passage,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EmbeddingError {
    Load(String),
    Compute(String),
}

impl fmt::Display for EmbeddingError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Load(reason) => write!(formatter, "embedding model failed to load: {reason}"),
            Self::Compute(reason) => write!(formatter, "embedding failed: {reason}"),
        }
    }
}

impl std::error::Error for EmbeddingError {}

/// Turns texts into vectors. The index records `model_id`, `revision` and `dimension` with the
/// vectors and never mixes vectors of different values.
pub trait Embedder: Send + Sync {
    fn model_id(&self) -> &str;
    fn revision(&self) -> &str;
    fn dimension(&self) -> usize;
    /// One vector per text, in order.
    fn embed(&self, kind: EmbeddingKind, texts: &[&str]) -> Result<Vec<Vec<f32>>, EmbeddingError>;
}
