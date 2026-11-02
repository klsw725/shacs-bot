use rusqlite::Connection;
use serde_json::json;
use shacs_core::memory_search::{
    self, Embedder, EmbeddingError, EmbeddingKind, FakeEmbedder, FakeEmbedding, IndexError,
    IndexModel, IndexedItem, ItemSource, MemoryIndex, MemorySearchPaths, ReconcileError,
    VECTOR_BATCH_LIMIT,
};
use shacs_core::runtime::MemoryStore;
use std::error::Error;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use unicode_normalization::UnicodeNormalization;

const DIMENSION: usize = 16;

struct Fixture {
    _root: tempfile::TempDir,
    data_dir: PathBuf,
    workspace: PathBuf,
}

impl Fixture {
    fn new() -> Result<Self, Box<dyn Error>> {
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

    fn paths(&self) -> Result<MemorySearchPaths, Box<dyn Error>> {
        Ok(MemorySearchPaths::new(&self.data_dir)?)
    }

    fn write_history(&self, entries: &[(u64, &str, &str)]) -> Result<(), Box<dyn Error>> {
        let lines: Vec<String> = entries
            .iter()
            .map(|(cursor, timestamp, content)| {
                json!({"cursor": cursor, "timestamp": timestamp, "content": content}).to_string()
            })
            .collect();
        fs::write(self.history_path(), format!("{}\n", lines.join("\n")))?;
        Ok(())
    }

    fn history_path(&self) -> PathBuf {
        self.workspace.join("memory").join("history.jsonl")
    }

    fn write_memory(&self, text: &str) -> Result<(), Box<dyn Error>> {
        fs::write(self.workspace.join("memory").join("MEMORY.md"), text)?;
        Ok(())
    }

    fn reconcile(&self, embedder: &dyn Embedder) -> Result<MemoryIndex, Box<dyn Error>> {
        Ok(MemoryIndex::reconcile(
            &self.paths()?,
            &self.workspace,
            &IndexModel::of(embedder),
        )?)
    }

    fn index_file(&self) -> Result<PathBuf, Box<dyn Error>> {
        Ok(self
            .paths()?
            .index_file(&memory_search::workspace_digest(&self.workspace)?))
    }
}

fn fake() -> FakeEmbedder {
    FakeEmbedder::new("test/fake", "r1", DIMENSION, FakeEmbedding::Hashed)
}

fn keys(items: &[IndexedItem]) -> Vec<&str> {
    items.iter().map(|item| item.key.as_str()).collect()
}

fn item<'a>(items: &'a [IndexedItem], text: &str) -> Option<&'a IndexedItem> {
    items.iter().find(|item| item.text == text)
}

fn complete(index: &mut MemoryIndex, embedder: &dyn Embedder) -> Result<(), Box<dyn Error>> {
    while index
        .compute_vectors(embedder, VECTOR_BATCH_LIMIT)?
        .remaining
        > 0
    {}
    Ok(())
}

#[test]
fn an_index_built_again_from_the_memory_files_is_the_same() -> Result<(), Box<dyn Error>> {
    // Given
    let fixture = Fixture::new()?;
    fixture.write_history(&[
        (
            1,
            "2026-10-01 09:00",
            "회의록을 정리했다\n- 다음 회의는 금요일",
        ),
        (2, "2026-10-02 10:00", "포폴 업데이트"),
    ])?;
    fixture.write_memory("# Memory\n\n- 커피는 라떼\n\n사용자는 서울에 산다.\n출근은 9시.\n")?;
    let embedder = fake();
    let mut first = fixture.reconcile(&embedder)?;
    complete(&mut first, &embedder)?;
    let before = first.items()?;
    drop(first);

    // When
    let file = fixture.index_file()?;
    for suffix in ["", "-wal", "-shm"] {
        let mut path = file.clone().into_os_string();
        path.push(suffix);
        let _ = fs::remove_file(PathBuf::from(path));
    }
    let mut second = fixture.reconcile(&embedder)?;
    complete(&mut second, &embedder)?;

    // Then
    assert_eq!(second.items()?, before);
    assert_eq!(before.len(), 5);
    assert!(before.iter().all(|item| item.vector.is_some()));
    Ok(())
}

#[test]
fn reconcile_follows_compaction_edits_and_moves_and_embeds_only_new_items(
) -> Result<(), Box<dyn Error>> {
    // Given
    let fixture = Fixture::new()?;
    let store = MemoryStore::with_max_history_entries(&fixture.workspace, 3)?;
    for content in ["첫 기록", "둘째 기록", "셋째 기록"] {
        store.append_history(content, None)?;
    }
    fixture.write_memory("- 지울 항목\n- 바꿀 항목\n- 그대로 둘 항목\n")?;
    let embedder = fake();
    let mut index = fixture.reconcile(&embedder)?;
    complete(&mut index, &embedder)?;
    let before = index.items()?;
    drop(index);

    // When
    store.append_history("넷째 기록", None)?;
    store.compact_history()?;
    let history = fs::read_to_string(fixture.history_path())?;
    let mut entries: Vec<serde_json::Value> = history
        .lines()
        .map(serde_json::from_str)
        .collect::<Result<_, _>>()?;
    entries[0]["content"] = json!("손으로 고친 둘째 기록");
    entries[1]["timestamp"] = json!("2030-01-01 00:00");
    let rewritten: Vec<String> = entries.iter().map(|entry| entry.to_string()).collect();
    fs::write(
        fixture.history_path(),
        format!("{}\n", rewritten.join("\n")),
    )?;
    fixture.write_memory("# 제목\n\n- 바꾼 항목\n\n- 그대로 둘 항목\n")?;
    let mut index = fixture.reconcile(&embedder)?;
    let after = index.items()?;
    let progress = index.compute_vectors(&embedder, VECTOR_BATCH_LIMIT)?;

    // Then
    for gone in [
        "첫 기록",
        "둘째 기록",
        "지울 항목",
        "- 지울 항목",
        "- 바꿀 항목",
    ] {
        assert!(item(&after, gone).is_none(), "{gone} must leave the index");
    }
    let kept_history = item(&after, "셋째 기록").ok_or("셋째 기록 missing")?;
    assert!(matches!(
        &kept_history.source,
        ItemSource::History { timestamp, .. } if timestamp == "2030-01-01 00:00"
    ));
    assert_eq!(
        kept_history.vector,
        item(&before, "셋째 기록").and_then(|item| item.vector.clone())
    );
    let kept_memory = item(&after, "- 그대로 둘 항목").ok_or("memory item missing")?;
    assert_eq!(
        kept_memory.source,
        ItemSource::Memory {
            first_line: 5,
            last_line: 5
        }
    );
    assert!(kept_memory.vector.is_some());
    let new_texts = ["손으로 고친 둘째 기록", "넷째 기록", "- 바꾼 항목"];
    for text in new_texts {
        assert!(
            item(&after, text).is_some_and(|item| item.vector.is_none()),
            "{text}"
        );
    }
    assert_eq!(progress.stored, new_texts.len());
    assert_eq!(progress.remaining, 0);
    Ok(())
}

#[test]
fn a_failed_reconcile_gives_the_snapshot_instead_of_the_old_index() -> Result<(), Box<dyn Error>> {
    // Given: an insert that fails inside the reconcile transaction.
    let fixture = Fixture::new()?;
    fixture.write_memory("- 남은 항목\n- 지울 항목\n")?;
    let embedder = fake();
    drop(fixture.reconcile(&embedder)?);
    Connection::open(fixture.index_file()?)?.execute_batch(
        "CREATE TRIGGER fail_insert BEFORE INSERT ON items \
         BEGIN SELECT RAISE(ABORT, 'injected failure'); END;",
    )?;
    fixture.write_memory("- 남은 항목\n- 새 항목\n")?;

    // When
    let reconciled = MemoryIndex::reconcile(
        &fixture.paths()?,
        &fixture.workspace,
        &IndexModel::of(&embedder),
    );

    // Then
    let Err(ReconcileError::IndexUnavailable { snapshot, reason }) = reconciled else {
        return Err("a failed reconcile must leave the index unavailable".into());
    };
    assert_eq!(
        snapshot,
        memory_search::read_memory_snapshot(&fixture.workspace)?
    );
    assert!(reason.contains("injected failure"), "{reason}");
    let connection = Connection::open(fixture.index_file()?)?;
    let mut statement = connection.prepare("SELECT text FROM items ORDER BY text")?;
    let texts = statement
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    assert_eq!(texts, ["- 남은 항목", "- 지울 항목"]);
    drop(statement);
    drop(connection);

    // Given: a file that is not a SQLite database.
    fs::write(
        fixture.index_file()?,
        b"not a SQLite database at all, only text",
    )?;

    // When
    let reconciled = MemoryIndex::reconcile(
        &fixture.paths()?,
        &fixture.workspace,
        &IndexModel::of(&embedder),
    );

    // Then
    let Err(ReconcileError::IndexUnavailable { snapshot, reason }) = reconciled else {
        return Err("a damaged index must be unavailable".into());
    };
    assert_eq!(
        snapshot,
        memory_search::read_memory_snapshot(&fixture.workspace)?
    );
    assert_eq!(snapshot.items.len(), 2);
    assert!(!reason.is_empty());
    Ok(())
}

#[test]
fn vectors_are_computed_in_bounded_batches_and_invalid_ones_are_never_stored(
) -> Result<(), Box<dyn Error>> {
    // Given
    let fixture = Fixture::new()?;
    let lines: Vec<String> = (0..300).map(|number| format!("- 항목 {number}")).collect();
    fixture.write_memory(&lines.join("\n"))?;
    let embedder = fake();
    let mut index = fixture.reconcile(&embedder)?;

    // When
    let first = index.compute_vectors(&embedder, usize::MAX)?;
    let second = index.compute_vectors(&embedder, VECTOR_BATCH_LIMIT)?;

    // Then
    assert_eq!(
        (first.stored, first.remaining),
        (VECTOR_BATCH_LIMIT, 300 - VECTOR_BATCH_LIMIT)
    );
    assert_eq!(
        (second.stored, second.remaining),
        (300 - VECTOR_BATCH_LIMIT, 0)
    );
    for invalid in [
        FakeEmbedding::NotFinite,
        FakeEmbedding::Zero,
        FakeEmbedding::WrongDimension,
    ] {
        let fixture = Fixture::new()?;
        fixture.write_memory("- 하나\n- 둘\n")?;
        let embedder = FakeEmbedder::new("test/fake", "r1", DIMENSION, invalid);
        let mut index = fixture.reconcile(&embedder)?;
        let progress = index.compute_vectors(&embedder, VECTOR_BATCH_LIMIT)?;
        assert_eq!(
            (progress.stored, progress.failed, progress.remaining),
            (0, 2, 0),
            "{invalid:?}"
        );
        assert_eq!(index.status()?.vectors_failed, 2, "{invalid:?}");
        assert!(index
            .items()?
            .iter()
            .all(|item| item.vector.is_none() && item.vector_failed));
    }
    let failing = FakeEmbedder::new("test/fake", "r1", DIMENSION, FakeEmbedding::Failing);
    let fixture = Fixture::new()?;
    fixture.write_memory("- 하나\n")?;
    let mut index = fixture.reconcile(&failing)?;
    assert!(index.compute_vectors(&failing, VECTOR_BATCH_LIMIT).is_err());
    assert_eq!(index.status()?.vectors_remaining, 1);
    Ok(())
}

#[test]
fn reconcile_puts_back_index_text_and_fts_rows_that_differ_from_the_memory_files(
) -> Result<(), Box<dyn Error>> {
    // Given
    let fixture = Fixture::new()?;
    fixture.write_memory("- 첫째\n- 둘째\n- 셋째\n- 넷째\n- 다섯째\n- 여섯째\n- 일곱째\n")?;
    let embedder = fake();
    let mut index = fixture.reconcile(&embedder)?;
    complete(&mut index, &embedder)?;
    let before = index.items()?;
    drop(index);
    let not_a_number = "0000C07F".repeat(DIMENSION);
    Connection::open(fixture.index_file()?)?.execute_batch(&format!(
        "UPDATE items SET text = '- 원본에 없는 글' WHERE text = '- 첫째';
         UPDATE items SET search_text = '- 원본에 없는 검색어' WHERE text = '- 둘째';
         UPDATE items_fts SET search_text = '- 남의 검색어'
             WHERE rowid = (SELECT id FROM items WHERE text = '- 셋째');
         INSERT INTO items_fts (rowid, search_text) VALUES (999, '- 원본에 없는 항목');
         UPDATE items SET vector = zeroblob(3) WHERE text = '- 넷째';
         UPDATE items SET vector = zeroblob({}) WHERE text = '- 다섯째';
         UPDATE items SET vector = x'{not_a_number}' WHERE text = '- 여섯째';
         UPDATE items SET vector_failed = 1 WHERE text = '- 일곱째';",
        DIMENSION * 4
    ))?;

    // When
    let index = fixture.reconcile(&embedder)?;

    // Then
    let after = index.items()?;
    let restored = |item: &IndexedItem| {
        (
            item.key.clone(),
            item.text.clone(),
            item.search_text.clone(),
        )
    };
    assert_eq!(
        after.iter().map(restored).collect::<Vec<_>>(),
        before.iter().map(restored).collect::<Vec<_>>()
    );
    for (text, kept) in [
        ("- 첫째", false),
        ("- 둘째", false),
        ("- 셋째", true),
        ("- 넷째", false),
        ("- 다섯째", false),
        ("- 여섯째", false),
        ("- 일곱째", false),
    ] {
        let found = item(&after, text).ok_or(text)?;
        assert_eq!(found.vector.is_some(), kept, "{text}");
        assert!(!found.vector_failed, "{text}");
    }
    assert_eq!(index.status()?.vectors_remaining, 6);
    let connection = Connection::open(fixture.index_file()?)?;
    let rows = |sql: &str| -> Result<Vec<(i64, String)>, rusqlite::Error> {
        let mut statement = connection.prepare(sql)?;
        let rows = statement
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect();
        rows
    };
    assert_eq!(
        rows("SELECT rowid, search_text FROM items_fts ORDER BY rowid")?,
        rows("SELECT id, search_text FROM items ORDER BY id")?
    );
    Ok(())
}

/// Rewrites the memory file and reconciles it, as another process could, during its first
/// computation.
struct EditingEmbedder<'a> {
    inner: FakeEmbedder,
    fixture: &'a Fixture,
    calls: AtomicUsize,
}

impl Embedder for EditingEmbedder<'_> {
    fn model_id(&self) -> &str {
        self.inner.model_id()
    }

    fn revision(&self) -> &str {
        self.inner.revision()
    }

    fn dimension(&self) -> usize {
        self.inner.dimension()
    }

    fn embed(&self, kind: EmbeddingKind, texts: &[&str]) -> Result<Vec<Vec<f32>>, EmbeddingError> {
        if self.calls.fetch_add(1, Ordering::SeqCst) == 0 {
            let compute = |error: Box<dyn Error>| EmbeddingError::Compute(error.to_string());
            self.fixture
                .write_memory("- 새 하나\n- 새 둘\n")
                .map_err(compute)?;
            let paths = self.fixture.paths().map_err(compute)?;
            MemoryIndex::reconcile(
                &paths,
                &self.fixture.workspace,
                &IndexModel::of(&self.inner),
            )
            .map_err(|error| EmbeddingError::Compute(error.to_string()))?;
        }
        self.inner.embed(kind, texts)
    }
}

#[test]
fn completing_vectors_reconciles_again_when_the_memory_files_change_meanwhile(
) -> Result<(), Box<dyn Error>> {
    // Given
    let fixture = Fixture::new()?;
    fixture.write_memory("- 하나\n- 둘\n")?;
    let embedder = EditingEmbedder {
        inner: fake(),
        fixture: &fixture,
        calls: AtomicUsize::new(0),
    };
    let mut index = fixture.reconcile(&embedder)?;

    // When
    index.complete_vectors(&embedder)?;

    // Then
    let items = index.items()?;
    let mut texts: Vec<&str> = items.iter().map(|item| item.text.as_str()).collect();
    texts.sort_unstable();
    assert_eq!(texts, ["- 새 둘", "- 새 하나"]);
    assert!(items.iter().all(|item| item.vector.is_some()));
    assert_eq!(index.status()?.vectors_remaining, 0);
    assert_eq!(embedder.calls.load(Ordering::SeqCst), 2);
    Ok(())
}

/// Adds a memory item and reconciles it, as another process could, during each of its first
/// `additions` computations.
struct GrowingEmbedder<'a> {
    inner: FakeEmbedder,
    fixture: &'a Fixture,
    additions: usize,
    calls: AtomicUsize,
}

impl Embedder for GrowingEmbedder<'_> {
    fn model_id(&self) -> &str {
        self.inner.model_id()
    }

    fn revision(&self) -> &str {
        self.inner.revision()
    }

    fn dimension(&self) -> usize {
        self.inner.dimension()
    }

    fn embed(&self, kind: EmbeddingKind, texts: &[&str]) -> Result<Vec<Vec<f32>>, EmbeddingError> {
        let call = self.calls.fetch_add(1, Ordering::SeqCst);
        if call < self.additions {
            self.add_item(call)
                .map_err(|error| EmbeddingError::Compute(error.to_string()))?;
        }
        self.inner.embed(kind, texts)
    }
}

impl GrowingEmbedder<'_> {
    fn add_item(&self, call: usize) -> Result<(), Box<dyn Error>> {
        let path = self.fixture.workspace.join("memory").join("MEMORY.md");
        let memory = fs::read_to_string(&path)?;
        self.fixture
            .write_memory(&format!("{memory}- 추가 {call}\n"))?;
        drop(self.fixture.reconcile(&self.inner)?);
        Ok(())
    }
}

#[test]
fn completing_vectors_includes_items_added_while_it_runs() -> Result<(), Box<dyn Error>> {
    // Given
    let fixture = Fixture::new()?;
    fixture.write_memory("- 처음\n")?;
    let embedder = GrowingEmbedder {
        inner: fake(),
        fixture: &fixture,
        additions: 2,
        calls: AtomicUsize::new(0),
    };
    let mut index = fixture.reconcile(&embedder)?;

    // When
    let status = index.complete_vectors(&embedder)?;

    // Then
    assert_eq!((status.items, status.vectors_remaining), (3, 0));
    assert_eq!(status, index.status()?);
    assert!(index.items()?.iter().all(|item| item.vector.is_some()));
    Ok(())
}

#[test]
fn completing_vectors_gives_up_when_the_index_keeps_changing() -> Result<(), Box<dyn Error>> {
    // Given
    let fixture = Fixture::new()?;
    fixture.write_memory("- 하나\n- 둘\n- 셋\n")?;
    let embedder = rebuilding(&fixture, usize::MAX, other_model(), true);
    let mut index = fixture.reconcile(&embedder)?;

    // When
    let completed = index.complete_vectors(&embedder);

    // Then
    assert!(matches!(completed, Err(IndexError::Stalled)));
    Ok(())
}

#[test]
fn the_status_reports_the_skipped_history_lines_of_the_latest_reconcile(
) -> Result<(), Box<dyn Error>> {
    // Given
    let fixture = Fixture::new()?;
    fixture.write_history(&[(1, "2026-10-01 09:00", "읽을 수 있는 기록")])?;
    let embedder = fake();
    let mut index = fixture.reconcile(&embedder)?;
    let before = index.status()?.skipped_history_lines;
    fs::OpenOptions::new()
        .append(true)
        .open(fixture.history_path())?
        .write_all(b"not json\n")?;
    drop(fixture.reconcile(&embedder)?);

    // When
    let status = index.status()?;
    let completed = index.complete_vectors(&embedder)?;

    // Then
    assert_eq!(before, 0);
    assert_eq!(status.skipped_history_lines, 1);
    assert_eq!(completed.skipped_history_lines, 1);
    assert_eq!(completed, fixture.reconcile(&embedder)?.status()?);
    Ok(())
}

#[test]
fn history_cursors_beyond_the_signed_range_keep_their_source() -> Result<(), Box<dyn Error>> {
    // Given
    let fixture = Fixture::new()?;
    let past_signed = u64::try_from(i64::MAX)? + 1;
    fixture.write_history(&[
        (past_signed, "2026-10-01 09:00", "큰 cursor"),
        (u64::MAX, "2026-10-02 09:00", "가장 큰 cursor"),
    ])?;
    let embedder = fake();
    let mut index = fixture.reconcile(&embedder)?;
    complete(&mut index, &embedder)?;
    drop(index);

    // When
    let index = fixture.reconcile(&embedder)?;

    // Then
    let items = index.items()?;
    for (text, cursor) in [("큰 cursor", past_signed), ("가장 큰 cursor", u64::MAX)] {
        let found = item(&items, text).ok_or(text)?;
        assert!(
            matches!(&found.source, ItemSource::History { cursor: stored, .. } if *stored == cursor),
            "{text}"
        );
        assert!(found.vector.is_some(), "{text}");
    }
    Ok(())
}

#[test]
fn an_index_whose_generation_is_missing_or_unusable_is_rebuilt() -> Result<(), Box<dyn Error>> {
    // Given
    let fixture = Fixture::new()?;
    fixture.write_memory("- 하나\n")?;
    let embedder = fake();
    for update in [
        "DELETE FROM meta WHERE key = 'generation'",
        "UPDATE meta SET value = 'first' WHERE key = 'generation'",
        "UPDATE meta SET value = '0' WHERE key = 'generation'",
    ] {
        let mut index = fixture.reconcile(&embedder)?;
        complete(&mut index, &embedder)?;
        Connection::open(fixture.index_file()?)?.execute(update, [])?;

        // When
        let status = index.status();
        let computed = index.compute_vectors(&embedder, VECTOR_BATCH_LIMIT);
        drop(index);
        let rebuilt = fixture.reconcile(&embedder)?.status()?;

        // Then
        assert!(
            matches!(status, Err(IndexError::InvalidMeta("generation"))),
            "{update}"
        );
        assert!(
            matches!(computed, Err(IndexError::InvalidMeta("generation"))),
            "{update}"
        );
        assert_eq!(
            (rebuilt.generation, rebuilt.vectors_remaining),
            (1, 1),
            "{update}"
        );
    }

    // Given: the last generation number.
    Connection::open(fixture.index_file()?)?.execute(
        "UPDATE meta SET value = '9223372036854775807' WHERE key = 'generation'",
        [],
    )?;
    let other = FakeEmbedder::new("test/second", "r1", DIMENSION, FakeEmbedding::Hashed);

    // When
    let rebuilt = fixture.reconcile(&other)?.status()?;

    // Then
    assert_eq!(rebuilt.generation, 1);
    Ok(())
}

#[test]
fn an_embedder_of_another_model_is_refused() -> Result<(), Box<dyn Error>> {
    // Given
    let fixture = Fixture::new()?;
    fixture.write_memory("- 하나\n")?;
    let mut index = fixture.reconcile(&fake())?;
    let other = FakeEmbedder::new("test/fake", "r2", DIMENSION, FakeEmbedding::Hashed);

    // When
    let refused = index.compute_vectors(&other, VECTOR_BATCH_LIMIT);

    // Then
    assert!(matches!(refused, Err(IndexError::ModelMismatch)));
    assert_eq!(index.status()?.vectors_remaining, 1);
    Ok(())
}

/// Rebuilds the index for `model` while it computes, as another process could, and fills in
/// that model's vectors when `complete` is set.
struct RebuildingEmbedder<'a> {
    inner: FakeEmbedder,
    fixture: &'a Fixture,
    rebuilds_left: AtomicUsize,
    model: FakeEmbedder,
    complete: bool,
    after_rebuild: Option<AfterRebuild>,
}

type AfterRebuild = fn(&Fixture) -> Result<(), Box<dyn Error>>;

impl Embedder for RebuildingEmbedder<'_> {
    fn model_id(&self) -> &str {
        self.inner.model_id()
    }

    fn revision(&self) -> &str {
        self.inner.revision()
    }

    fn dimension(&self) -> usize {
        self.inner.dimension()
    }

    fn embed(&self, kind: EmbeddingKind, texts: &[&str]) -> Result<Vec<Vec<f32>>, EmbeddingError> {
        if self.rebuilds_left.load(Ordering::SeqCst) > 0 {
            self.rebuilds_left.fetch_sub(1, Ordering::SeqCst);
            self.rebuild()
                .map_err(|error| EmbeddingError::Compute(error.to_string()))?;
        }
        self.inner.embed(kind, texts)
    }
}

impl RebuildingEmbedder<'_> {
    fn rebuild(&self) -> Result<(), Box<dyn Error>> {
        Connection::open(self.fixture.index_file()?)?.execute(
            "UPDATE meta SET value = '0' WHERE key = 'schema_version'",
            [],
        )?;
        let mut rebuilt = self.fixture.reconcile(&self.model)?;
        if self.complete {
            complete(&mut rebuilt, &self.model)?;
        }
        if let Some(after_rebuild) = self.after_rebuild {
            after_rebuild(self.fixture)?;
        }
        Ok(())
    }
}

fn other_model() -> FakeEmbedder {
    FakeEmbedder::new("test/other", "r9", DIMENSION, FakeEmbedding::Hashed)
}

fn rebuilding(
    fixture: &Fixture,
    rebuilds: usize,
    model: FakeEmbedder,
    complete: bool,
) -> RebuildingEmbedder<'_> {
    RebuildingEmbedder {
        inner: fake(),
        fixture,
        rebuilds_left: AtomicUsize::new(rebuilds),
        model,
        complete,
        after_rebuild: None,
    }
}

#[test]
fn completing_vectors_returns_the_failure_of_reconciling_again_without_reading_the_index(
) -> Result<(), Box<dyn Error>> {
    // Given
    let fixture = Fixture::new()?;
    fixture.write_memory("- 하나\n")?;
    let mut embedder = rebuilding(&fixture, 1, other_model(), false);
    embedder.after_rebuild = Some(|fixture| {
        let memory = fixture.workspace.join("memory").join("MEMORY.md");
        fs::remove_file(&memory)?;
        fs::create_dir(&memory)?;
        Ok(())
    });
    let mut index = fixture.reconcile(&embedder)?;

    // When
    let completed = index.complete_vectors(&embedder);

    // Then
    assert!(matches!(
        completed,
        Err(IndexError::Reconcile(error))
            if matches!(error.as_ref(), ReconcileError::SourceUnreadable(_))
    ));
    Ok(())
}

#[test]
fn only_lines_that_start_with_a_dash_or_a_hash_are_items_or_headings() -> Result<(), Box<dyn Error>>
{
    // Given
    let fixture = Fixture::new()?;
    fixture.write_history(&[(
        1,
        "2026-10-01 09:00",
        "할 일\n  - 하위 1\n  - 하위 2\n  # 제목 아님",
    )])?;
    fixture.write_memory(
        "- 맨 앞 항목\n  - 들여쓴 줄 1\n  - 들여쓴 줄 2\n\n  # 들여쓴 줄\n# 제목\n",
    )?;

    // When
    let items = fixture.reconcile(&fake())?.items()?;

    // Then
    let mut texts: Vec<&str> = items.iter().map(|item| item.text.as_str()).collect();
    texts.sort_unstable();
    assert_eq!(
        texts,
        [
            "  # 들여쓴 줄",
            "  - 들여쓴 줄 1\n  - 들여쓴 줄 2",
            "- 맨 앞 항목",
            "할 일\n  - 하위 1\n  - 하위 2\n  # 제목 아님",
        ]
    );
    let indented = item(&items, "  - 들여쓴 줄 1\n  - 들여쓴 줄 2").ok_or("paragraph missing")?;
    assert_eq!(
        indented.source,
        ItemSource::Memory {
            first_line: 2,
            last_line: 3
        }
    );
    Ok(())
}

fn meta_model(fixture: &Fixture) -> Result<String, Box<dyn Error>> {
    Ok(Connection::open(fixture.index_file()?)?.query_row(
        "SELECT value FROM meta WHERE key = 'model_id'",
        [],
        |row| row.get(0),
    )?)
}

#[test]
fn completing_vectors_takes_the_index_back_after_another_model_completed_it_meanwhile(
) -> Result<(), Box<dyn Error>> {
    // Given
    let fixture = Fixture::new()?;
    fixture.write_memory("- 하나\n- 둘\n")?;
    let embedder = rebuilding(&fixture, 1, other_model(), true);
    let mut index = fixture.reconcile(&embedder)?;

    // When
    index.complete_vectors(&embedder)?;

    // Then
    let status = index.status()?;
    assert_eq!(status.model, IndexModel::of(&fake()));
    assert_eq!(status.vectors_remaining, 0);
    assert!(index.items()?.iter().all(|item| item.vector.is_some()));
    assert_eq!(meta_model(&fixture)?, "test/fake");
    Ok(())
}

#[test]
fn completing_vectors_takes_back_an_index_another_model_completed_before_the_first_batch(
) -> Result<(), Box<dyn Error>> {
    // Given
    let fixture = Fixture::new()?;
    fixture.write_memory("- 하나\n- 둘\n")?;
    let embedder = fake();
    let mut index = fixture.reconcile(&embedder)?;
    let mut theirs = fixture.reconcile(&other_model())?;
    complete(&mut theirs, &other_model())?;
    drop(theirs);

    // When
    let first = index.compute_vectors(&embedder, VECTOR_BATCH_LIMIT);
    let completed = index.complete_vectors(&embedder);

    // Then
    assert!(matches!(first, Err(IndexError::Superseded)));
    assert!(completed.is_ok());
    assert_eq!(index.status()?.model, IndexModel::of(&embedder));
    assert_eq!(meta_model(&fixture)?, "test/fake");
    Ok(())
}

#[test]
fn vectors_computed_across_a_rebuild_for_another_model_are_not_stored() -> Result<(), Box<dyn Error>>
{
    // Given
    let fixture = Fixture::new()?;
    fixture.write_memory("- 하나\n- 둘\n- 셋\n")?;
    let embedder = rebuilding(&fixture, 1, other_model(), false);
    let mut index = fixture.reconcile(&embedder)?;
    let generation = index.status()?.generation;

    // When
    let computed = index.compute_vectors(&embedder, VECTOR_BATCH_LIMIT);

    // Then
    assert!(matches!(computed, Err(IndexError::Superseded)));
    let stored: i64 = Connection::open(fixture.index_file()?)?.query_row(
        "SELECT count(*) FROM items WHERE vector IS NOT NULL",
        [],
        |row| row.get(0),
    )?;
    assert_eq!(stored, 0);
    assert_eq!(meta_model(&fixture)?, "test/other");
    assert!(index.status()?.generation > generation);
    Ok(())
}

#[test]
fn vectors_computed_across_a_rebuild_for_the_same_model_are_discarded() -> Result<(), Box<dyn Error>>
{
    // Given
    let fixture = Fixture::new()?;
    fixture.write_memory("- 하나\n- 둘\n- 셋\n")?;
    let embedder = rebuilding(&fixture, 1, fake(), false);
    let mut index = fixture.reconcile(&embedder)?;
    let generation = index.status()?.generation;

    // When
    let progress = index.compute_vectors(&embedder, VECTOR_BATCH_LIMIT)?;

    // Then
    assert_eq!(
        (progress.stored, progress.discarded, progress.remaining),
        (0, 3, 3)
    );
    assert!(index.items()?.iter().all(|item| item.vector.is_none()));
    assert!(index.status()?.generation > generation);
    Ok(())
}

#[test]
fn search_text_is_the_item_in_nfc_and_lowercase() -> Result<(), Box<dyn Error>> {
    // Given
    let fixture = Fixture::new()?;
    let decomposed: String = "- 한글 회의록 Café MEETING".nfd().collect();
    fixture.write_memory(&format!("{decomposed}\n"))?;

    // When
    let index = fixture.reconcile(&fake())?;
    let items = index.items()?;

    // Then
    let [only] = items.as_slice() else {
        return Err(format!("expected one item, found {}", items.len()).into());
    };
    assert_eq!(only.text, decomposed);
    assert_eq!(only.search_text, "- 한글 회의록 café meeting");
    assert_eq!(
        only.search_text,
        only.search_text.nfc().collect::<String>(),
        "the search text must already be NFC"
    );
    Ok(())
}

#[test]
fn an_index_whose_meta_or_workspace_differs_is_rebuilt_as_a_new_generation(
) -> Result<(), Box<dyn Error>> {
    // Given
    let fixture = Fixture::new()?;
    fixture.write_memory("- 하나\n")?;
    let embedder = fake();
    let mut index = fixture.reconcile(&embedder)?;
    complete(&mut index, &embedder)?;
    let first_generation = index.status()?.generation;
    drop(index);

    // When / Then: another model, then another revision.
    let mut generation = first_generation;
    for other in [
        FakeEmbedder::new("test/second", "r1", DIMENSION, FakeEmbedding::Hashed),
        FakeEmbedder::new("test/second", "r2", DIMENSION, FakeEmbedding::Hashed),
    ] {
        let index = fixture.reconcile(&other)?;
        let status = index.status()?;
        assert_eq!(status.generation, generation + 1, "{}", other.revision());
        assert_eq!(status.vectors_remaining, 1);
        generation = status.generation;
    }

    // When / Then: a changed schema version, then a changed workspace digest.
    for update in [
        "UPDATE meta SET value = '99' WHERE key = 'schema_version'",
        "UPDATE meta SET value = 'another' WHERE key = 'workspace_digest'",
    ] {
        Connection::open(fixture.index_file()?)?.execute(update, [])?;
        let index = fixture.reconcile(&embedder)?;
        assert_eq!(index.status()?.generation, generation + 1, "{update}");
        generation += 1;
    }

    // When / Then: an index copied from another workspace.
    let other_workspace = Fixture::new()?;
    other_workspace.write_memory("- 다른 작업 공간\n")?;
    let other_file = MemorySearchPaths::new(&fixture.data_dir)?.index_file(
        &memory_search::workspace_digest(&other_workspace.workspace)?,
    );
    assert_ne!(other_file, fixture.index_file()?);
    fs::copy(fixture.index_file()?, &other_file)?;
    let copied = MemoryIndex::reconcile(
        &fixture.paths()?,
        &other_workspace.workspace,
        &IndexModel::of(&embedder),
    )?;
    assert_eq!(copied.status()?.generation, generation + 1);
    assert_eq!(keys(&copied.items()?).len(), 1);
    assert!(copied
        .items()?
        .iter()
        .all(|item| item.text == "- 다른 작업 공간"));
    Ok(())
}

#[cfg(unix)]
#[test]
fn index_folders_and_files_are_private_and_symbolic_links_are_refused() -> Result<(), Box<dyn Error>>
{
    use std::os::unix::fs::{symlink, PermissionsExt};

    // Given
    let fixture = Fixture::new()?;
    fixture.write_memory("- 하나\n")?;
    let embedder = fake();
    let index = fixture.reconcile(&embedder)?;
    let file = fixture.index_file()?;
    let mode = |path: &Path| -> Result<u32, Box<dyn Error>> {
        Ok(fs::symlink_metadata(path)?.permissions().mode() & 0o777)
    };

    // Then
    let memory_search_dir = fixture.data_dir.join("runtime").join("memory-search");
    for dir in [memory_search_dir.clone(), memory_search_dir.join("index")] {
        assert_eq!(mode(&dir)?, 0o700, "{}", dir.display());
    }
    for suffix in ["", "-wal", "-shm"] {
        let mut path = file.clone().into_os_string();
        path.push(suffix);
        assert_eq!(mode(Path::new(&path))?, 0o600, "{suffix}");
    }
    drop(index);

    // When / Then: each of the file, its sidecars and the folder as a symbolic link.
    for suffix in ["", "-wal", "-shm"] {
        let fixture = Fixture::new()?;
        fixture.write_memory("- 하나\n")?;
        drop(fixture.reconcile(&embedder)?);
        let mut path = fixture.index_file()?.into_os_string();
        path.push(suffix);
        let path = PathBuf::from(path);
        let target = fixture.data_dir.join("elsewhere");
        fs::write(&target, b"")?;
        let _ = fs::remove_file(&path);
        symlink(&target, &path)?;
        let reconciled = MemoryIndex::reconcile(
            &fixture.paths()?,
            &fixture.workspace,
            &IndexModel::of(&embedder),
        );
        assert!(
            matches!(&reconciled, Err(ReconcileError::IndexUnavailable { reason, .. }) if reason.contains("symbolic link")),
            "{suffix}"
        );
    }
    let fixture = Fixture::new()?;
    fixture.write_memory("- 하나\n")?;
    let elsewhere = fixture.data_dir.join("elsewhere-dir");
    fs::create_dir_all(&elsewhere)?;
    fs::create_dir_all(fixture.data_dir.join("runtime"))?;
    symlink(
        &elsewhere,
        fixture.data_dir.join("runtime").join("memory-search"),
    )?;
    let reconciled = MemoryIndex::reconcile(
        &fixture.paths()?,
        &fixture.workspace,
        &IndexModel::of(&embedder),
    );
    assert!(matches!(
        &reconciled,
        Err(ReconcileError::IndexUnavailable { reason, .. }) if reason.contains("symbolic link")
    ));
    assert_eq!(fs::read_dir(&elsewhere)?.count(), 0);
    Ok(())
}
