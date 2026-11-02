use serde_json::{json, Value};
use shacs_core::memory_search::{
    self, Embedder, EmbedderLoader, EmbeddingError, EmbeddingKind, FakeEmbedder, FakeEmbedding,
    FakeLoader, IndexModel, LazyEmbedder, LoadError, MemorySearch, MemorySearchPaths,
};
use std::collections::BTreeMap;
use std::error::Error;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

pub type TestResult = Result<(), Box<dyn Error>>;
pub const DIMENSION: usize = 16;

pub struct Fixture {
    _root: tempfile::TempDir,
    pub data_dir: PathBuf,
    pub workspace: PathBuf,
}

impl Fixture {
    pub fn new() -> Result<Self, Box<dyn Error>> {
        let root = tempfile::tempdir()?;
        let data_dir = root.path().join("data");
        let workspace = root.path().join("workspace");
        fs::create_dir_all(&data_dir)?;
        fs::create_dir_all(workspace.join("memory"))?;
        Ok(Self {
            _root: root,
            data_dir,
            workspace,
        })
    }

    pub fn paths(&self) -> Result<MemorySearchPaths, Box<dyn Error>> {
        Ok(MemorySearchPaths::new(&self.data_dir)?)
    }

    pub fn memory_file(&self) -> PathBuf {
        self.workspace.join("memory").join("MEMORY.md")
    }

    pub fn history_file(&self) -> PathBuf {
        self.workspace.join("memory").join("history.jsonl")
    }

    pub fn index_file(&self) -> Result<PathBuf, Box<dyn Error>> {
        Ok(self
            .paths()?
            .index_file(&memory_search::workspace_digest(&self.workspace)?))
    }

    pub fn write_memory(&self, text: &str) -> TestResult {
        fs::write(self.memory_file(), text)?;
        Ok(())
    }

    pub fn write_history(&self, entries: &[(u64, &str, &str)]) -> TestResult {
        let lines: Vec<String> = entries
            .iter()
            .map(|(cursor, timestamp, content)| {
                json!({"cursor": cursor, "timestamp": timestamp, "content": content}).to_string()
            })
            .collect();
        fs::write(self.history_file(), format!("{}\n", lines.join("\n")))?;
        Ok(())
    }

    /// Semantic search on and no result limit.
    pub fn search(
        &self,
        loader: impl EmbedderLoader + 'static,
    ) -> Result<MemorySearch, Box<dyn Error>> {
        Ok(MemorySearch::new(
            self.paths()?,
            self.workspace.clone(),
            Arc::new(LazyEmbedder::new(Arc::new(loader))),
        ))
    }
}

pub fn fake() -> FakeEmbedder {
    FakeEmbedder::new("test/fake", "r1", DIMENSION, FakeEmbedding::Hashed)
}

pub fn fake_loader() -> FakeLoader {
    FakeLoader::new(fake())
}

pub fn fake_with(embedding: FakeEmbedding) -> FakeLoader {
    FakeLoader::new(FakeEmbedder::new("test/fake", "r1", DIMENSION, embedding))
}

pub fn run(search: &MemorySearch, query: &str, limit: usize) -> Result<Value, Box<dyn Error>> {
    Ok(serde_json::from_str(&search.search(query, limit))?)
}

fn results(result: &Value) -> &[Value] {
    result["results"].as_array().map_or(&[], Vec::as_slice)
}

pub fn texts(result: &Value) -> Vec<String> {
    strings(result, "/text")
}

pub fn matched_by(result: &Value) -> Vec<String> {
    strings(result, "/matched_by")
}

pub fn keys(result: &Value) -> Vec<String> {
    strings(result, "/key")
}

/// The first line of each result from `MEMORY.md`, 0 for a history result.
pub fn first_lines(result: &Value) -> Vec<u64> {
    results(result)
        .iter()
        .map(|item| item["source"]["first_line"].as_u64().unwrap_or(0))
        .collect()
}

fn strings(result: &Value, pointer: &str) -> Vec<String> {
    results(result)
        .iter()
        .map(|item| {
            item.pointer(pointer)
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned()
        })
        .collect()
}

pub struct CountingLoader {
    pub loads: Arc<AtomicUsize>,
}

impl EmbedderLoader for CountingLoader {
    fn model(&self) -> IndexModel {
        IndexModel::of(&fake())
    }

    fn load(&self) -> Result<Arc<dyn Embedder>, LoadError> {
        self.loads.fetch_add(1, Ordering::SeqCst);
        Ok(Arc::new(fake()))
    }
}

/// Each passage text gets the vector of its row, any other text a zero vector, and every query
/// [`at_rank`]`(0)`.
#[derive(Clone)]
pub struct TableEmbedder {
    pub passages: BTreeMap<String, Vec<f32>>,
}

impl Embedder for TableEmbedder {
    fn model_id(&self) -> &str {
        "test/table"
    }

    fn revision(&self) -> &str {
        "r1"
    }

    fn dimension(&self) -> usize {
        2
    }

    fn embed(&self, kind: EmbeddingKind, texts: &[&str]) -> Result<Vec<Vec<f32>>, EmbeddingError> {
        Ok(texts
            .iter()
            .map(|text| match kind {
                EmbeddingKind::Query => at_rank(0),
                EmbeddingKind::Passage => self
                    .passages
                    .get(*text)
                    .cloned()
                    .unwrap_or_else(|| vec![0.0, 0.0]),
            })
            .collect())
    }
}

impl EmbedderLoader for TableEmbedder {
    fn model(&self) -> IndexModel {
        IndexModel::of(self)
    }

    fn load(&self) -> Result<Arc<dyn Embedder>, LoadError> {
        Ok(Arc::new(self.clone()))
    }
}

/// A unit vector `rank` steps away from the query's: the higher the rank, the lower the cosine.
pub fn at_rank(rank: usize) -> Vec<f32> {
    let angle = rank as f32 * 0.02;
    vec![angle.cos(), angle.sin()]
}
