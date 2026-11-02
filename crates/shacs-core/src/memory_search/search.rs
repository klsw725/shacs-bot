use std::fmt::Display;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use super::embedder::{Embedder, EmbeddingKind};
use super::memory_index::{self, IndexError, IndexedItem, MemoryIndex, ReconcileError};
use super::memory_search_paths::MemorySearchPaths;
use super::memory_snapshot::{self, MemorySnapshot};
use super::model_loader::{LazyEmbedder, LoadError, VerifiedModelLoader};
use super::query_terms;
use super::search_ranking::{self, SearchItem};
use super::search_result::{IndexState, SearchResult, SemanticReason, SemanticStatus};
use super::search_settings::MemorySearchSettings;
use super::verified_model::ModelFileError;

pub const DEFAULT_SEARCH_RESULTS: usize = 5;
pub const MAX_SEARCH_RESULTS: usize = 20;

/// Searches the memory files of one workspace by their words and by meaning.
pub struct MemorySearch {
    paths: MemorySearchPaths,
    workspace: PathBuf,
    embedder: Arc<LazyEmbedder>,
    semantic: bool,
    max_result_chars: usize,
    running: Mutex<()>,
}

impl MemorySearch {
    /// Searches with semantic search on and no result limit. The index holds `embedder`'s
    /// model, which is loaded by the first search that needs it.
    pub fn new(paths: MemorySearchPaths, workspace: PathBuf, embedder: Arc<LazyEmbedder>) -> Self {
        Self {
            paths,
            workspace,
            embedder,
            semantic: true,
            max_result_chars: 0,
            running: Mutex::new(()),
        }
    }

    pub fn configured(
        settings: &MemorySearchSettings,
        paths: MemorySearchPaths,
        workspace: PathBuf,
    ) -> Self {
        let loader = VerifiedModelLoader::new(paths.clone(), settings.manifest);
        Self::new(
            paths,
            workspace,
            Arc::new(LazyEmbedder::new(Arc::new(loader))),
        )
        .with_semantic(settings.semantic)
        .with_max_result_chars(settings.max_result_chars)
    }

    /// Off never loads the model; the index is still kept for it.
    pub fn with_semantic(mut self, semantic: bool) -> Self {
        self.semantic = semantic;
        self
    }

    /// 0 is no limit. Configuration refuses 1 to 999, where even the fixed result of an
    /// overlong search may not fit.
    pub fn with_max_result_chars(mut self, max_result_chars: usize) -> Self {
        self.max_result_chars = max_result_chars;
        self
    }

    /// Reconciles the index with the memory files and returns, as JSON, the `limit` items (1 to
    /// [`MAX_SEARCH_RESULTS`]) that match `query` best, with the semantic search and index
    /// status. Searches in one process run one at a time; the memory files are only read.
    pub fn search(&self, query: &str, limit: usize) -> String {
        let limit = limit.clamp(1, MAX_SEARCH_RESULTS);
        let result = match self.running.lock() {
            Ok(_running) => self.run(query, limit),
            Err(_) => SearchResult::search_unavailable(),
        };
        result.render(self.max_result_chars)
    }

    fn run(&self, query: &str, limit: usize) -> SearchResult {
        let model = self.embedder.model();
        let terms = query_terms::query_terms(query);
        let mut semantic = SemanticStatus::Available;
        let mut index = match MemoryIndex::reconcile(&self.paths, &self.workspace, &model) {
            Ok(index) => index,
            Err(ReconcileError::SourceUnreadable(error)) => {
                return SearchResult::source_unreadable(error)
            }
            Err(ReconcileError::IndexUnavailable { snapshot, reason }) => {
                semantic.fail(SemanticReason::IndexUnavailable, &reason);
                return search_snapshot(snapshot, &terms, limit, semantic, &reason);
            }
        };
        if terms.is_empty() {
            semantic.fail(SemanticReason::NotRequested, "the query has no terms");
            return match index.checked_status() {
                Ok(status) => SearchResult::empty_query(semantic, IndexState::of(&status)),
                Err(error) => self.without_index(&terms, limit, semantic, &error),
            };
        }
        let embedder = self.load_embedder(&mut semantic);
        if let Some(embedder) = &embedder {
            match index.compute_vectors(embedder.as_ref(), memory_index::VECTOR_BATCH_LIMIT) {
                Ok(_) | Err(IndexError::Superseded) => {}
                Err(IndexError::Embedding(error)) => {
                    semantic.fail(SemanticReason::EmbeddingFailed, error);
                }
                Err(IndexError::ModelMismatch) => {
                    semantic.fail(SemanticReason::ModelLoadFailed, IndexError::ModelMismatch);
                }
                Err(error) => return self.without_index(&terms, limit, semantic, &error),
            }
        }
        let view = match index.search_view(query_terms::fts_query(&terms).as_deref()) {
            Ok(view) => view,
            Err(error) => return self.without_index(&terms, limit, semantic, &error),
        };
        if view.status.model != model {
            semantic.fail(
                SemanticReason::IndexSuperseded,
                format!(
                    "the memory index was rebuilt for {}@{}",
                    view.status.model.id, view.status.model.revision
                ),
            );
        }
        let items = indexed_items(view.items, model.dimension);
        let by_text = search_ranking::rank_text(&items, &terms, Some(&view.text_scores));
        let mut by_meaning = Vec::new();
        if let (true, Some(embedder)) = (semantic.is_available(), &embedder) {
            match query_vector(embedder.as_ref(), query, model.dimension) {
                Ok(vector) => by_meaning = search_ranking::rank_semantic(&items, &vector),
                Err((reason, detail)) => semantic.fail(reason, detail),
            }
        }
        SearchResult::found(
            search_ranking::merge(&by_text, &by_meaning, limit),
            semantic,
            IndexState::of(&view.status),
        )
    }

    fn load_embedder(&self, semantic: &mut SemanticStatus) -> Option<Arc<dyn Embedder>> {
        if !self.semantic {
            semantic.fail(
                SemanticReason::Disabled,
                "agents.defaults.memorySearch.enable is false",
            );
            return None;
        }
        let error = match self.embedder.get() {
            Ok(embedder) => return Some(embedder),
            Err(error) => error,
        };
        let install = format!(
            "{error}; run `shacs-bot memory model install --model {}`",
            self.embedder.model().id
        );
        match error {
            LoadError::File(ModelFileError::Missing { .. }) => {
                semantic.fail(SemanticReason::ModelMissing, install);
            }
            LoadError::File(_) => semantic.fail(SemanticReason::ModelUnverified, install),
            LoadError::Model(error) => semantic.fail(SemanticReason::ModelLoadFailed, error),
        }
        None
    }

    /// After the index failed past reconciling: the memory files are read again and searched
    /// by text without it.
    fn without_index(
        &self,
        terms: &[String],
        limit: usize,
        mut semantic: SemanticStatus,
        error: &IndexError,
    ) -> SearchResult {
        semantic.fail(SemanticReason::IndexUnavailable, error);
        match memory_snapshot::read_memory_snapshot(&self.workspace) {
            Ok(snapshot) => search_snapshot(snapshot, terms, limit, semantic, error),
            Err(error) => SearchResult::source_unreadable(error),
        }
    }
}

fn search_snapshot(
    snapshot: MemorySnapshot,
    terms: &[String],
    limit: usize,
    semantic: SemanticStatus,
    reason: impl Display,
) -> SearchResult {
    let index = IndexState::unavailable(reason, Some(snapshot.skipped_history_lines));
    if terms.is_empty() {
        return SearchResult::empty_query(semantic, index);
    }
    let items: Vec<SearchItem> = snapshot
        .items
        .into_iter()
        .map(|item| SearchItem {
            search_text: memory_snapshot::search_text(&item.text),
            key: item.key,
            source: item.source,
            text: item.text,
            vector: None,
        })
        .collect();
    let by_text = search_ranking::rank_text(&items, terms, None);
    SearchResult::found(search_ranking::merge(&by_text, &[], limit), semantic, index)
}

fn indexed_items(items: Vec<IndexedItem>, dimension: usize) -> Vec<SearchItem> {
    items
        .into_iter()
        .map(|item| SearchItem {
            vector: item
                .vector
                .filter(|vector| memory_index::is_valid_vector(vector, dimension)),
            key: item.key,
            source: item.source,
            text: item.text,
            search_text: item.search_text,
        })
        .collect()
}

fn query_vector(
    embedder: &dyn Embedder,
    query: &str,
    dimension: usize,
) -> Result<Vec<f32>, (SemanticReason, String)> {
    let vectors = embedder
        .embed(EmbeddingKind::Query, &[query])
        .map_err(|error| (SemanticReason::EmbeddingFailed, error.to_string()))?;
    let count = vectors.len();
    let Ok([vector]) = <[Vec<f32>; 1]>::try_from(vectors) else {
        return Err((
            SemanticReason::EmbeddingFailed,
            format!("{count} vectors for one query"),
        ));
    };
    let problem = if vector.len() != dimension {
        format!(
            "the query vector has {} dimensions, not {dimension}",
            vector.len()
        )
    } else if !vector.iter().all(|value| value.is_finite()) {
        "the query vector has a NaN or infinite value".to_owned()
    } else if vector.iter().all(|value| *value == 0.0) {
        "the query vector has length 0".to_owned()
    } else {
        return Ok(vector);
    };
    Err((SemanticReason::InvalidQueryEmbedding, problem))
}
