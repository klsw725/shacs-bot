use rusqlite::{
    params, Connection, OpenFlags, OptionalExtension, Row, Transaction, TransactionBehavior,
};
use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::path::{Path, PathBuf};
use std::time::Duration;
use std::{fmt, io};

use super::embedder::{Embedder, EmbeddingError, EmbeddingKind};
use super::memory_search_paths::{self, MemorySearchPaths};
use super::memory_snapshot::{
    self, ItemSource, MemoryItem, MemorySnapshot, SnapshotError, CHUNK_RULE_VERSION,
};
use super::model_manifest::ModelManifest;

pub const INDEX_SCHEMA_VERSION: i64 = 1;
/// Vectors one call of [`MemoryIndex::compute_vectors`] computes at most, so that one search
/// never waits for a whole index to be embedded.
pub const VECTOR_BATCH_LIMIT: usize = 256;
const MAX_STALLED_ROUNDS: usize = 3;
const SKIPPED_HISTORY_LINES: &str = "skipped_history_lines";
/// Meta values that record the state of the index rather than what it was built for, so a
/// change in them does not rebuild it.
const STATE_META_KEYS: [&str; 2] = ["generation", SKIPPED_HISTORY_LINES];
const BUSY_TIMEOUT: Duration = Duration::from_millis(5000);
const SCHEMA: &str = "
    CREATE TABLE meta (key TEXT PRIMARY KEY NOT NULL, value TEXT NOT NULL) STRICT, WITHOUT ROWID;
    CREATE TABLE items (
        id INTEGER PRIMARY KEY,
        key TEXT NOT NULL UNIQUE,
        source TEXT NOT NULL CHECK (source IN ('history', 'memory')),
        cursor INTEGER,
        timestamp TEXT,
        item_ordinal INTEGER,
        first_line INTEGER,
        last_line INTEGER,
        text TEXT NOT NULL,
        search_text TEXT NOT NULL,
        digest TEXT NOT NULL,
        vector BLOB,
        vector_failed INTEGER NOT NULL DEFAULT 0
    ) STRICT;
    CREATE VIRTUAL TABLE items_fts USING fts5(search_text, tokenize = 'trigram');
";

/// The model whose vectors an index holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexModel {
    pub id: String,
    pub revision: String,
    pub dimension: usize,
}

impl IndexModel {
    pub fn of(embedder: &dyn Embedder) -> Self {
        Self {
            id: embedder.model_id().to_owned(),
            revision: embedder.revision().to_owned(),
            dimension: embedder.dimension(),
        }
    }

    pub fn from_manifest(manifest: &ModelManifest) -> Self {
        Self {
            id: manifest.id.to_owned(),
            revision: manifest.revision.to_owned(),
            dimension: manifest.dimension,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexStatus {
    /// The model the index file records now, which another process may have changed.
    pub model: IndexModel,
    pub items: usize,
    pub vectors_remaining: usize,
    pub vectors_failed: usize,
    pub skipped_history_lines: usize,
    pub generation: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct VectorProgress {
    pub stored: usize,
    /// Vectors that were not finite, had length 0 or the wrong dimension.
    pub failed: usize,
    /// Vectors computed for rows that changed, or for a generation the index has left.
    pub discarded: usize,
    /// Rows still without a vector, counted while the index was found to hold this model.
    pub remaining: usize,
}

/// One row of the index.
#[derive(Debug, Clone, PartialEq)]
pub struct IndexedItem {
    pub key: String,
    pub source: ItemSource,
    pub text: String,
    pub search_text: String,
    pub digest: String,
    pub vector: Option<Vec<f32>>,
    pub vector_failed: bool,
}

#[derive(Debug)]
pub enum ReconcileError {
    /// A memory file could not be read, so there is nothing to search.
    SourceUnreadable(SnapshotError),
    /// The index could not be brought in line with the memory files and must not be read. The
    /// snapshot just read is returned in its place.
    IndexUnavailable {
        snapshot: MemorySnapshot,
        reason: String,
    },
}

impl fmt::Display for ReconcileError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SourceUnreadable(error) => error.fmt(formatter),
            Self::IndexUnavailable { reason, .. } => {
                write!(formatter, "memory index is unavailable: {reason}")
            }
        }
    }
}

impl std::error::Error for ReconcileError {}

#[derive(Debug)]
pub enum IndexError {
    Sqlite(rusqlite::Error),
    Io(io::Error),
    Embedding(EmbeddingError),
    /// The embedder is not the model the index was opened for.
    ModelMismatch,
    InvalidMeta(&'static str),
    /// Another process has rebuilt the index for another model.
    Superseded,
    /// Reconciling again failed; the index must not be read.
    Reconcile(Box<ReconcileError>),
    Stalled,
}

impl fmt::Display for IndexError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Sqlite(error) => write!(formatter, "memory index SQLite failed: {error}"),
            Self::Io(error) => write!(formatter, "memory index I/O failed: {error}"),
            Self::Embedding(error) => error.fmt(formatter),
            Self::ModelMismatch => formatter.write_str("embedder differs from the index model"),
            Self::InvalidMeta(key) => {
                write!(formatter, "memory index meta {key} is missing or invalid")
            }
            Self::Superseded => {
                formatter.write_str("the memory index was rebuilt for another model")
            }
            Self::Reconcile(error) => error.fmt(formatter),
            Self::Stalled => formatter
                .write_str("the memory index kept changing while its vectors were computed"),
        }
    }
}

impl std::error::Error for IndexError {}

impl From<rusqlite::Error> for IndexError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Sqlite(error)
    }
}

impl From<io::Error> for IndexError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

/// The search index of one workspace's memory files, rebuildable from them at any time.
pub struct MemoryIndex {
    connection: Connection,
    paths: MemorySearchPaths,
    workspace: PathBuf,
    model: IndexModel,
}

impl MemoryIndex {
    /// Reads both memory files and, in one transaction, removes the items they no longer have,
    /// sets the sources of the others to their current values and adds the new ones. An index
    /// whose schema, model, chunk rules or workspace differ is emptied first and gets the next
    /// generation.
    pub fn reconcile(
        paths: &MemorySearchPaths,
        workspace: &Path,
        model: &IndexModel,
    ) -> Result<Self, ReconcileError> {
        let snapshot = memory_snapshot::read_memory_snapshot(workspace)
            .map_err(ReconcileError::SourceUnreadable)?;
        match Self::open_and_apply(paths, workspace, model, &snapshot) {
            Ok(index) => Ok(index),
            Err(error) => Err(ReconcileError::IndexUnavailable {
                snapshot,
                reason: error.to_string(),
            }),
        }
    }

    fn open_and_apply(
        paths: &MemorySearchPaths,
        workspace: &Path,
        model: &IndexModel,
        snapshot: &MemorySnapshot,
    ) -> Result<Self, IndexError> {
        let workspace_digest = memory_search_paths::workspace_digest(workspace)?;
        let mut connection = open_index(paths, &workspace_digest)?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        prepare_schema(&transaction, model, &workspace_digest)?;
        apply_snapshot(&transaction, snapshot, model.dimension)?;
        transaction.execute(
            "INSERT INTO meta (key, value) VALUES (?1, ?2) \
             ON CONFLICT (key) DO UPDATE SET value = excluded.value",
            params![
                SKIPPED_HISTORY_LINES,
                snapshot.skipped_history_lines.to_string()
            ],
        )?;
        transaction.commit()?;
        Ok(Self {
            connection,
            paths: paths.clone(),
            workspace: workspace.to_path_buf(),
            model: model.clone(),
        })
    }

    pub fn model(&self) -> &IndexModel {
        &self.model
    }

    /// Every value read from one snapshot of the index file.
    pub fn status(&self) -> Result<IndexStatus, IndexError> {
        let transaction = self.connection.unchecked_transaction()?;
        let status = read_status(&transaction, read_model(&transaction)?)?;
        transaction.commit()?;
        Ok(status)
    }

    /// [`Self::status`] read the way a search reads it: see `read_checked_model`.
    pub(super) fn checked_status(&self) -> Result<IndexStatus, IndexError> {
        let transaction = self.connection.unchecked_transaction()?;
        let status = read_status(&transaction, read_checked_model(&transaction)?)?;
        transaction.commit()?;
        Ok(status)
    }

    /// The status, every row and the bm25 of the rows `fts_query` matches, all read from one
    /// snapshot of the index file.
    pub(super) fn search_view(&self, fts_query: Option<&str>) -> Result<SearchView, IndexError> {
        let transaction = self.connection.unchecked_transaction()?;
        let status = read_status(&transaction, read_checked_model(&transaction)?)?;
        let items = read_items(&transaction)?;
        let mut text_scores = BTreeMap::new();
        if let Some(fts_query) = fts_query {
            let mut statement = transaction.prepare(
                "SELECT items.key, bm25(items_fts) FROM items_fts \
                 JOIN items ON items.id = items_fts.rowid WHERE items_fts MATCH ?1",
            )?;
            let rows = statement.query_map(params![fts_query], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, f64>(1)?))
            })?;
            for row in rows {
                let (key, score) = row?;
                text_scores.insert(key, score);
            }
        }
        transaction.commit()?;
        Ok(SearchView {
            status,
            items,
            text_scores,
        })
    }

    /// Computes the vectors of at most `limit` items that have none, and never more than
    /// [`VECTOR_BATCH_LIMIT`], outside any transaction, then stores each only if the index
    /// generation and the item's text are still the ones the computation started from. Fails
    /// with [`IndexError::Superseded`] when another process has rebuilt the index for another
    /// model.
    pub fn compute_vectors(
        &mut self,
        embedder: &dyn Embedder,
        limit: usize,
    ) -> Result<VectorProgress, IndexError> {
        if IndexModel::of(embedder) != self.model {
            return Err(IndexError::ModelMismatch);
        }
        let (generation, pending, remaining) = {
            let transaction = self.connection.transaction()?;
            if read_model(&transaction)? != self.model {
                return Err(IndexError::Superseded);
            }
            let generation = read_generation(&transaction)?;
            let mut statement = transaction.prepare(
                "SELECT id, text, digest FROM items \
                 WHERE vector IS NULL AND vector_failed = 0 ORDER BY id LIMIT ?1",
            )?;
            let pending = statement
                .query_map(params![batch_size(limit)], |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                    ))
                })?
                .collect::<Result<Vec<_>, _>>()?;
            (generation, pending, count_remaining(&transaction)?)
        };
        let mut progress = VectorProgress {
            remaining,
            ..VectorProgress::default()
        };
        if !pending.is_empty() {
            let texts: Vec<&str> = pending.iter().map(|(_, text, _)| text.as_str()).collect();
            let vectors = embedder
                .embed(EmbeddingKind::Passage, &texts)
                .map_err(IndexError::Embedding)?;
            if vectors.len() != pending.len() {
                return Err(IndexError::Embedding(EmbeddingError::Compute(format!(
                    "{} vectors for {} texts",
                    vectors.len(),
                    pending.len()
                ))));
            }
            let transaction = self
                .connection
                .transaction_with_behavior(TransactionBehavior::Immediate)?;
            if read_model(&transaction)? != self.model {
                return Err(IndexError::Superseded);
            }
            if read_generation(&transaction)? == generation {
                for ((id, _, digest), vector) in pending.iter().zip(&vectors) {
                    let stored = if is_valid_vector(vector, self.model.dimension) {
                        transaction.execute(
                            "UPDATE items SET vector = ?1, vector_failed = 0 \
                             WHERE id = ?2 AND digest = ?3",
                            params![vector_bytes(vector), id, digest],
                        )?
                    } else {
                        transaction.execute(
                            "UPDATE items SET vector_failed = 1 WHERE id = ?1 AND digest = ?2",
                            params![id, digest],
                        )?
                    };
                    match (stored, is_valid_vector(vector, self.model.dimension)) {
                        (0, _) => progress.discarded += 1,
                        (_, true) => progress.stored += 1,
                        (_, false) => progress.failed += 1,
                    }
                }
            } else {
                progress.discarded = pending.len();
            }
            progress.remaining = count_remaining(&transaction)?;
            transaction.commit()?;
        }
        Ok(progress)
    }

    /// Computes every missing vector and returns the status in which the index holds this
    /// model and no row lacks a vector. After a batch that stores nothing because the index
    /// changed meanwhile, or that finds the index rebuilt for another model, the index is
    /// reconciled again; after `MAX_STALLED_ROUNDS` such batches in a row this gives up with
    /// [`IndexError::Stalled`].
    pub fn complete_vectors(&mut self, embedder: &dyn Embedder) -> Result<IndexStatus, IndexError> {
        let mut stalled = 0;
        loop {
            let progressed = match self.compute_vectors(embedder, VECTOR_BATCH_LIMIT) {
                Ok(progress) => progress.stored + progress.failed > 0,
                Err(IndexError::Superseded) => false,
                Err(error) => return Err(error),
            };
            let status = self.status()?;
            if status.model == self.model {
                if status.vectors_remaining == 0 {
                    return Ok(status);
                }
                if progressed {
                    stalled = 0;
                    continue;
                }
            }
            stalled += 1;
            if stalled == MAX_STALLED_ROUNDS {
                return Err(IndexError::Stalled);
            }
            *self = Self::reconcile(&self.paths, &self.workspace, &self.model)
                .map_err(|error| IndexError::Reconcile(Box::new(error)))?;
        }
    }

    /// Every row, in key order.
    pub fn items(&self) -> Result<Vec<IndexedItem>, IndexError> {
        read_items(&self.connection)
    }
}

pub(super) struct SearchView {
    pub status: IndexStatus,
    pub items: Vec<IndexedItem>,
    /// By item key; smaller is a better match.
    pub text_scores: BTreeMap<String, f64>,
}

fn read_status(connection: &Connection, model: IndexModel) -> Result<IndexStatus, IndexError> {
    let (items, remaining, failed): (i64, i64, i64) = connection.query_row(
        "SELECT count(*), \
         coalesce(sum(vector IS NULL AND vector_failed = 0), 0), \
         coalesce(sum(vector_failed), 0) FROM items",
        [],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    )?;
    Ok(IndexStatus {
        model,
        items: count(items),
        vectors_remaining: count(remaining),
        vectors_failed: count(failed),
        skipped_history_lines: read_meta_count(connection, SKIPPED_HISTORY_LINES)?,
        generation: read_generation(connection)?,
    })
}

fn read_items(connection: &Connection) -> Result<Vec<IndexedItem>, IndexError> {
    let mut statement = connection.prepare(
        "SELECT key, source, cursor, timestamp, item_ordinal, first_line, last_line, text, \
         search_text, digest, vector, vector_failed FROM items ORDER BY key",
    )?;
    let rows = statement.query_map([], |row| {
        let vector: Option<Vec<u8>> = row.get(10)?;
        Ok(IndexedItem {
            key: row.get(0)?,
            source: read_source(row, 1)?,
            text: row.get(7)?,
            search_text: row.get(8)?,
            digest: row.get(9)?,
            vector: vector.as_deref().map(vector_from_bytes),
            vector_failed: row.get::<_, i64>(11)? != 0,
        })
    })?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

/// Opens the index file of the workspace with `workspace_digest`, creating it 0600 when it
/// does not exist. The file, its SQLite sidecars and the folders above them must not be
/// symbolic links.
fn open_index(paths: &MemorySearchPaths, workspace_digest: &str) -> Result<Connection, IndexError> {
    let dir = paths.index_dir();
    paths.create_private_dir(&dir)?;
    let file = paths.index_file(workspace_digest);
    let sidecars = [sidecar(&file, "-wal"), sidecar(&file, "-shm")];
    for path in std::iter::once(&file).chain(&sidecars) {
        paths.check_no_symlinks(path)?;
        match fs::symlink_metadata(path) {
            Ok(metadata) if !metadata.is_file() => {
                return Err(IndexError::Io(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("memory index path is not a file: {}", path.display()),
                )));
            }
            Ok(_) => memory_search_paths::set_mode(path, 0o600)?,
            Err(error) if error.kind() == io::ErrorKind::NotFound && *path == file => {
                let mut options = OpenOptions::new();
                options.write(true).create_new(true);
                match memory_search_paths::open_private_file(path, &mut options) {
                    Ok(_) => {}
                    Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                    Err(error) => return Err(error.into()),
                }
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    let connection = Connection::open_with_flags(
        &file,
        OpenFlags::SQLITE_OPEN_READ_WRITE
            | OpenFlags::SQLITE_OPEN_NO_MUTEX
            | OpenFlags::SQLITE_OPEN_NOFOLLOW,
    )?;
    connection.busy_timeout(BUSY_TIMEOUT)?;
    let journal_mode: String =
        connection.query_row("PRAGMA journal_mode = WAL", [], |row| row.get(0))?;
    if !journal_mode.eq_ignore_ascii_case("wal") {
        return Err(IndexError::Io(io::Error::other(format!(
            "memory index journal mode is {journal_mode}, not wal"
        ))));
    }
    connection.pragma_update(None, "synchronous", "FULL")?;
    if cfg!(target_vendor = "apple") {
        connection.pragma_update(None, "fullfsync", true)?;
        connection.pragma_update(None, "checkpoint_fullfsync", true)?;
    }
    for path in &sidecars {
        paths.check_no_symlinks(path)?;
        if fs::symlink_metadata(path).is_ok() {
            memory_search_paths::set_mode(path, 0o600)?;
        }
    }
    Ok(connection)
}

fn sidecar(file: &Path, suffix: &str) -> PathBuf {
    let mut name = file.as_os_str().to_owned();
    name.push(suffix);
    PathBuf::from(name)
}

/// Keeps the index when its meta matches; otherwise drops every table of it and starts the
/// next generation.
fn prepare_schema(
    transaction: &Transaction<'_>,
    model: &IndexModel,
    workspace_digest: &str,
) -> Result<(), IndexError> {
    let expected = expected_meta(model, workspace_digest);
    let has_meta: bool = transaction.query_row(
        "SELECT EXISTS (SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'meta')",
        [],
        |row| row.get(0),
    )?;
    let current = if has_meta {
        read_meta(transaction)?
    } else {
        BTreeMap::new()
    };
    let generation = current
        .get("generation")
        .and_then(|value| parse_generation(value));
    let current_identity: BTreeMap<&str, &str> = current
        .iter()
        .filter(|(key, _)| !STATE_META_KEYS.contains(&key.as_str()))
        .map(|(key, value)| (key.as_str(), value.as_str()))
        .collect();
    let expected_view: BTreeMap<&str, &str> = expected
        .iter()
        .map(|(key, value)| (*key, value.as_str()))
        .collect();
    if generation.is_some() && current_identity == expected_view {
        return Ok(());
    }
    let generation = generation
        .and_then(|generation| generation.checked_add(1))
        .unwrap_or(1);
    transaction.execute_batch(
        "DROP TABLE IF EXISTS items_fts; DROP TABLE IF EXISTS items; DROP TABLE IF EXISTS meta;",
    )?;
    transaction.execute_batch(SCHEMA)?;
    let mut insert = transaction.prepare("INSERT INTO meta (key, value) VALUES (?1, ?2)")?;
    for (key, value) in &expected {
        insert.execute(params![key, value])?;
    }
    insert.execute(params!["generation", generation.to_string()])?;
    Ok(())
}

fn expected_meta(model: &IndexModel, workspace_digest: &str) -> Vec<(&'static str, String)> {
    vec![
        ("chunk_rule_version", CHUNK_RULE_VERSION.to_string()),
        ("model_dimension", model.dimension.to_string()),
        ("model_id", model.id.clone()),
        ("model_revision", model.revision.clone()),
        ("schema_version", INDEX_SCHEMA_VERSION.to_string()),
        ("workspace_digest", workspace_digest.to_owned()),
    ]
}

fn read_meta(connection: &Connection) -> Result<BTreeMap<String, String>, IndexError> {
    let mut statement = connection.prepare("SELECT key, value FROM meta")?;
    let rows = statement.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
    Ok(rows.collect::<Result<BTreeMap<_, _>, _>>()?)
}

fn read_generation(connection: &Connection) -> Result<i64, IndexError> {
    let value: Option<String> = connection
        .query_row(
            "SELECT value FROM meta WHERE key = 'generation'",
            [],
            |row| row.get(0),
        )
        .optional()?;
    value
        .as_deref()
        .and_then(parse_generation)
        .ok_or(IndexError::InvalidMeta("generation"))
}

fn read_meta_count(connection: &Connection, key: &'static str) -> Result<usize, IndexError> {
    let value: Option<String> = connection
        .query_row(
            "SELECT value FROM meta WHERE key = ?1",
            params![key],
            |row| row.get(0),
        )
        .optional()?;
    value
        .and_then(|value| value.parse().ok())
        .ok_or(IndexError::InvalidMeta(key))
}

fn parse_generation(value: &str) -> Option<i64> {
    value
        .parse::<i64>()
        .ok()
        .filter(|generation| *generation > 0)
}

fn read_model(connection: &Connection) -> Result<IndexModel, IndexError> {
    let meta = read_meta(connection)?;
    let value = |key: &str| meta.get(key).cloned().unwrap_or_default();
    Ok(IndexModel {
        id: value("model_id"),
        revision: value("model_revision"),
        dimension: value("model_dimension").parse().unwrap_or(0),
    })
}

/// The model the meta records, refusing with [`IndexError::InvalidMeta`] a meta that lacks a
/// value, holds a malformed one, or was written for other schema or chunk rules. Only a whole,
/// valid model that differs means another process rebuilt the index.
fn read_checked_model(connection: &Connection) -> Result<IndexModel, IndexError> {
    let meta = read_meta(connection)?;
    let value = |key: &'static str| {
        meta.get(key)
            .filter(|value| !value.is_empty())
            .ok_or(IndexError::InvalidMeta(key))
    };
    for (key, expected) in [
        ("schema_version", INDEX_SCHEMA_VERSION),
        ("chunk_rule_version", CHUNK_RULE_VERSION),
    ] {
        if value(key)?.parse::<i64>().ok() != Some(expected) {
            return Err(IndexError::InvalidMeta(key));
        }
    }
    let workspace_digest = value("workspace_digest")?;
    if workspace_digest.len() != 64
        || !workspace_digest
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(IndexError::InvalidMeta("workspace_digest"));
    }
    Ok(IndexModel {
        id: value("model_id")?.clone(),
        revision: value("model_revision")?.clone(),
        dimension: value("model_dimension")?
            .parse()
            .ok()
            .filter(|dimension| *dimension > 0)
            .ok_or(IndexError::InvalidMeta("model_dimension"))?,
    })
}

struct StoredRow {
    id: i64,
    digest: String,
    text: String,
    search_text: String,
    source: ItemSource,
}

/// Brings the rows in line with `snapshot`: removes the rows it no longer has, rewrites those
/// whose text differs from it (their vectors are computed again), updates changed sources and
/// adds new rows. Then the FTS rows are made to match the rows, and vectors whose size is not
/// the model's are dropped.
fn apply_snapshot(
    transaction: &Transaction<'_>,
    snapshot: &MemorySnapshot,
    dimension: usize,
) -> Result<(), IndexError> {
    let stored = stored_rows(transaction)?;
    let wanted: BTreeMap<&str, &MemoryItem> = snapshot
        .items
        .iter()
        .map(|item| (item.key.as_str(), item))
        .collect();
    for (key, row) in &stored {
        if !wanted.contains_key(key.as_str()) {
            transaction.execute("DELETE FROM items WHERE id = ?1", params![row.id])?;
        }
    }
    for item in &snapshot.items {
        let columns = SourceColumns::of(&item.source);
        let search_text = memory_snapshot::search_text(&item.text);
        match stored.get(&item.key) {
            Some(row)
                if row.digest == item.digest
                    && row.text == item.text
                    && row.search_text == search_text =>
            {
                if row.source != item.source {
                    transaction.execute(
                        "UPDATE items SET source = ?1, cursor = ?2, timestamp = ?3, \
                         item_ordinal = ?4, first_line = ?5, last_line = ?6 WHERE id = ?7",
                        params![
                            columns.kind,
                            columns.cursor,
                            columns.timestamp,
                            columns.ordinal,
                            columns.first_line,
                            columns.last_line,
                            row.id
                        ],
                    )?;
                }
            }
            Some(row) => {
                transaction.execute(
                    "UPDATE items SET source = ?1, cursor = ?2, timestamp = ?3, item_ordinal = ?4, \
                     first_line = ?5, last_line = ?6, text = ?7, search_text = ?8, digest = ?9, \
                     vector = NULL, vector_failed = 0 WHERE id = ?10",
                    params![
                        columns.kind,
                        columns.cursor,
                        columns.timestamp,
                        columns.ordinal,
                        columns.first_line,
                        columns.last_line,
                        item.text,
                        search_text,
                        item.digest,
                        row.id
                    ],
                )?;
            }
            None => {
                transaction.execute(
                    "INSERT INTO items (key, source, cursor, timestamp, item_ordinal, first_line, \
                     last_line, text, search_text, digest) \
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                    params![
                        item.key,
                        columns.kind,
                        columns.cursor,
                        columns.timestamp,
                        columns.ordinal,
                        columns.first_line,
                        columns.last_line,
                        item.text,
                        search_text,
                        item.digest
                    ],
                )?;
            }
        }
    }
    transaction.execute_batch(
        "DELETE FROM items_fts WHERE rowid NOT IN (SELECT id FROM items) \
             OR rowid IN (SELECT items.id FROM items JOIN items_fts ON items_fts.rowid = items.id \
                 WHERE items_fts.search_text IS NOT items.search_text); \
         INSERT INTO items_fts (rowid, search_text) \
             SELECT id, search_text FROM items WHERE id NOT IN (SELECT rowid FROM items_fts);",
    )?;
    clear_invalid_vectors(transaction, dimension)
}

/// Clears every stored vector that is not a valid vector of `dimension`, and any vector of a
/// row marked as failed, so that those rows are computed again.
fn clear_invalid_vectors(
    transaction: &Transaction<'_>,
    dimension: usize,
) -> Result<(), IndexError> {
    let invalid: Vec<i64> = {
        let mut statement = transaction
            .prepare("SELECT id, vector, vector_failed FROM items WHERE vector IS NOT NULL")?;
        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, Vec<u8>>(1)?,
                row.get::<_, i64>(2)?,
            ))
        })?;
        let mut invalid = Vec::new();
        for row in rows {
            let (id, bytes, failed) = row?;
            let valid = bytes.len() == dimension.saturating_mul(4)
                && is_valid_vector(&vector_from_bytes(&bytes), dimension);
            if !valid || failed != 0 {
                invalid.push(id);
            }
        }
        invalid
    };
    for id in invalid {
        transaction.execute(
            "UPDATE items SET vector = NULL, vector_failed = 0 WHERE id = ?1",
            params![id],
        )?;
    }
    Ok(())
}

fn stored_rows(transaction: &Transaction<'_>) -> Result<BTreeMap<String, StoredRow>, IndexError> {
    let mut statement = transaction.prepare(
        "SELECT id, key, digest, source, cursor, timestamp, item_ordinal, first_line, last_line, \
         text, search_text FROM items",
    )?;
    let rows = statement.query_map([], |row| {
        Ok((
            row.get::<_, String>(1)?,
            StoredRow {
                id: row.get(0)?,
                digest: row.get(2)?,
                source: read_source(row, 3)?,
                text: row.get(9)?,
                search_text: row.get(10)?,
            },
        ))
    })?;
    Ok(rows.collect::<Result<BTreeMap<_, _>, _>>()?)
}

/// The source in the six columns from `first`: source, cursor, timestamp, item_ordinal,
/// first_line and last_line. A value that does not fit reads as 0, so the next reconcile
/// finds the source changed and writes it again.
fn read_source(row: &Row<'_>, first: usize) -> rusqlite::Result<ItemSource> {
    let number = |index: usize| -> rusqlite::Result<usize> {
        Ok(row
            .get::<_, Option<i64>>(index)?
            .and_then(|value| usize::try_from(value).ok())
            .unwrap_or(0))
    };
    if row.get::<_, String>(first)? == "history" {
        Ok(ItemSource::History {
            cursor: row
                .get::<_, Option<i64>>(first + 1)?
                .map_or(0, i64::cast_unsigned),
            timestamp: row.get::<_, Option<String>>(first + 2)?.unwrap_or_default(),
            ordinal: number(first + 3)?,
        })
    } else {
        Ok(ItemSource::Memory {
            first_line: number(first + 4)?,
            last_line: number(first + 5)?,
        })
    }
}

struct SourceColumns<'a> {
    kind: &'static str,
    cursor: Option<i64>,
    timestamp: Option<&'a str>,
    ordinal: Option<i64>,
    first_line: Option<i64>,
    last_line: Option<i64>,
}

impl<'a> SourceColumns<'a> {
    fn of(source: &'a ItemSource) -> Self {
        let number = |value: usize| i64::try_from(value).ok();
        match source {
            ItemSource::History {
                cursor,
                timestamp,
                ordinal,
            } => Self {
                kind: "history",
                cursor: Some(cursor.cast_signed()),
                timestamp: Some(timestamp),
                ordinal: number(*ordinal),
                first_line: None,
                last_line: None,
            },
            ItemSource::Memory {
                first_line,
                last_line,
            } => Self {
                kind: "memory",
                cursor: None,
                timestamp: None,
                ordinal: None,
                first_line: number(*first_line),
                last_line: number(*last_line),
            },
        }
    }
}

fn count_remaining(connection: &Connection) -> Result<usize, IndexError> {
    let remaining: i64 = connection.query_row(
        "SELECT count(*) FROM items WHERE vector IS NULL AND vector_failed = 0",
        [],
        |row| row.get(0),
    )?;
    Ok(count(remaining))
}

fn batch_size(limit: usize) -> i64 {
    i64::try_from(limit.min(VECTOR_BATCH_LIMIT)).unwrap_or(0)
}

pub(super) fn is_valid_vector(vector: &[f32], dimension: usize) -> bool {
    vector.len() == dimension
        && vector.iter().all(|value| value.is_finite())
        && vector.iter().any(|value| *value != 0.0)
}

fn vector_bytes(vector: &[f32]) -> Vec<u8> {
    vector
        .iter()
        .flat_map(|value| value.to_le_bytes())
        .collect()
}

fn vector_from_bytes(bytes: &[u8]) -> Vec<f32> {
    let (chunks, _) = bytes.as_chunks::<4>();
    chunks
        .iter()
        .map(|chunk| f32::from_le_bytes(*chunk))
        .collect()
}

fn count(value: i64) -> usize {
    usize::try_from(value).unwrap_or(0)
}
