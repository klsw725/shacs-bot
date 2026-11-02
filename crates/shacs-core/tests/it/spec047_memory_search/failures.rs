use super::support::*;
use rusqlite::Connection;
use serde_json::{json, Value};
use shacs_core::memory_search::{
    Embedder, EmbedderLoader, EmbeddingError, EmbeddingKind, FakeEmbedder, FakeEmbedding,
    FakeLoader, IndexModel, LazyEmbedder, LoadError, MemoryIndex, MemorySearch, MemorySearchPaths,
    ModelArchitecture, ModelFile, ModelManifest, Pooling, VerifiedModelLoader,
};
use shacs_core::runtime::{AgentLoop, AgentLoopConfig, ContextBuilder, InboundMessage, MessageBus};
use shacs_core::tools::{MemorySearchTool, ToolRegistry};
use shacs_providers::{
    LlmResponse, ProviderClient, ProviderError, ProviderEvent, ProviderRequest, ToolCallRequest,
};
use shacs_session::SessionManager;
use std::collections::{BTreeMap, VecDeque};
use std::error::Error;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::{fs, thread};

const QUERY: &str = "치과 예약";
/// More than the five items the query matches, so an item that must not match would show.
const LIMIT: usize = 10;
const ANSWER: &str = "치과 예약은 화요일 오후 3시입니다.";

/// Five items holding a term of the query and one holding none. The query's two terms are
/// short, so they rank the same with and without the index.
fn memory_fixture() -> Result<Fixture, Box<dyn Error>> {
    let fixture = Fixture::new()?;
    fixture.write_memory(
        "- 치과 예약은 화요일 오후 3시\n- 회의 예약 확인\n- 치과 치료비 영수증\n- 점심 메뉴\n",
    )?;
    fixture.write_history(&[
        (1, "2026-10-01 09:00", "치과 예약을 다시 잡았다"),
        (2, "2026-10-02 09:00", "주말 캠핑장 예약"),
    ])?;
    Ok(fixture)
}

fn text_results() -> Result<Value, Box<dyn Error>> {
    let fixture = memory_fixture()?;
    let result = run(
        &fixture.search(fake_loader())?.with_semantic(false),
        QUERY,
        LIMIT,
    )?;
    Ok(result["results"].clone())
}

fn test_manifest() -> &'static ModelManifest {
    let files = [
        "config.json",
        "tokenizer.json",
        "model.safetensors",
        "1_Pooling/config.json",
        "modules.json",
        "sentence_bert_config.json",
    ];
    Box::leak(Box::new(ModelManifest {
        id: "test/model",
        revision: "rev1",
        architecture: ModelArchitecture::Bert,
        dimension: 4,
        query_prefix: "",
        passage_prefix: "",
        pooling: Pooling::Mean,
        max_tokens: 8,
        files: files.map(|name| ModelFile {
            name,
            size: 4,
            sha256: "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08",
        }),
    }))
}

/// Fails for memory items only, so the query alone would still embed.
#[derive(Clone)]
struct PassageFailingEmbedder;

impl Embedder for PassageFailingEmbedder {
    fn model_id(&self) -> &str {
        "test/fake"
    }

    fn revision(&self) -> &str {
        "r1"
    }

    fn dimension(&self) -> usize {
        DIMENSION
    }

    fn embed(&self, kind: EmbeddingKind, texts: &[&str]) -> Result<Vec<Vec<f32>>, EmbeddingError> {
        match kind {
            EmbeddingKind::Passage => Err(EmbeddingError::Compute("memory items fail".to_owned())),
            EmbeddingKind::Query => fake().embed(kind, texts),
        }
    }
}

impl EmbedderLoader for PassageFailingEmbedder {
    fn model(&self) -> IndexModel {
        IndexModel::of(self)
    }

    fn load(&self) -> Result<Arc<dyn Embedder>, LoadError> {
        Ok(Arc::new(self.clone()))
    }
}

type Unavailable = (
    &'static str,
    fn(&Fixture) -> Result<MemorySearch, Box<dyn Error>>,
);

fn unavailable_cases() -> [Unavailable; 10] {
    [
        ("disabled", |fixture| {
            Ok(fixture.search(fake_loader())?.with_semantic(false))
        }),
        ("model_missing", |fixture| {
            fixture.search(VerifiedModelLoader::new(fixture.paths()?, test_manifest()))
        }),
        ("model_unverified", |fixture| {
            let manifest = test_manifest();
            let dir = fixture.paths()?.model_dir(manifest);
            fs::create_dir_all(&dir)?;
            fs::write(dir.join("config.json"), b"not the pinned file")?;
            fixture.search(VerifiedModelLoader::new(fixture.paths()?, manifest))
        }),
        ("model_load_failed", |fixture| {
            fixture.search(FakeLoader::failing(fake()))
        }),
        ("embedding_failed", |fixture| {
            fixture.search(fake_with(FakeEmbedding::Failing))
        }),
        ("embedding_failed", |fixture| {
            fixture.search(PassageFailingEmbedder)
        }),
        ("embedding_failed", |fixture| {
            run(&fixture.search(fake_loader())?, QUERY, LIMIT)?;
            fixture.search(fake_with(FakeEmbedding::Failing))
        }),
        ("invalid_query_embedding", |fixture| {
            fixture.search(fake_with(FakeEmbedding::NotFinite))
        }),
        ("invalid_query_embedding", |fixture| {
            fixture.search(fake_with(FakeEmbedding::Zero))
        }),
        ("invalid_query_embedding", |fixture| {
            fixture.search(fake_with(FakeEmbedding::WrongDimension))
        }),
    ]
}

#[test]
fn without_semantic_search_the_text_results_come_back_with_the_reason_and_the_turn_ends(
) -> TestResult {
    // Given
    let expected = text_results()?;

    for (index, (reason, build)) in unavailable_cases().into_iter().enumerate() {
        let fixture = memory_fixture()?;
        let search = Arc::new(build(&fixture)?);

        // When
        let found = run(&search, QUERY, LIMIT)?;
        let (answer, tool_result) = turn(&fixture, search)?;

        // Then
        let case = format!("case {index} ({reason})");
        assert_eq!(found["results"], expected, "{case}");
        assert_eq!(found["semantic"]["status"], "unavailable", "{case}");
        assert_eq!(found["semantic"]["reason"], reason, "{case}");
        assert!(
            found["semantic"]["detail"]
                .as_str()
                .is_some_and(|detail| !detail.is_empty()),
            "{case}"
        );
        assert!(
            ["complete", "incomplete"].contains(&found["index"]["status"].as_str().unwrap_or("")),
            "{case}"
        );
        assert!(found.get("error").is_none(), "{case}");
        assert_eq!(answer.as_deref(), Some(ANSWER), "{case}");
        assert_eq!(tool_result["results"], expected, "{case}");
        assert_eq!(tool_result["semantic"]["reason"], reason, "{case}");
    }
    Ok(())
}

#[test]
fn an_index_with_vectors_left_reports_how_many_remain_and_the_memory_files_stay_as_they_were(
) -> TestResult {
    // Given
    let fixture = Fixture::new()?;
    let lines: Vec<String> = (0..300)
        .map(|number| format!("- 항목 {number:03}"))
        .collect();
    fixture.write_memory(&lines.join("\n"))?;
    let before = fs::read(fixture.memory_file())?;
    let modified = fs::metadata(fixture.memory_file())?.modified()?;
    let search = fixture.search(fake_loader())?;

    // When
    let first = run(&search, "항목 001", 5)?;
    let second = run(&search, "항목 001", 5)?;

    // Then
    assert_eq!(first["semantic"], json!({"status": "available"}));
    assert_eq!(
        first["index"],
        json!({
            "status": "incomplete",
            "vectors_remaining": 44,
            "vectors_failed": 0,
            "skipped_history_lines": 0
        })
    );
    assert_eq!(
        second["index"],
        json!({
            "status": "complete",
            "vectors_remaining": 0,
            "vectors_failed": 0,
            "skipped_history_lines": 0
        })
    );
    assert_eq!(fs::read(fixture.memory_file())?, before);
    assert_eq!(fs::metadata(fixture.memory_file())?.modified()?, modified);
    Ok(())
}

#[test]
fn a_failed_reconcile_searches_the_memory_files_by_text_without_the_model() -> TestResult {
    // Given
    let fixture = memory_fixture()?;
    run(&fixture.search(fake_loader())?, QUERY, LIMIT)?;
    fs::write(fixture.index_file()?, b"not a SQLite database at all")?;
    let loads = Arc::new(AtomicUsize::new(0));
    let search = fixture.search(CountingLoader {
        loads: loads.clone(),
    })?;

    // When
    let found = run(&search, QUERY, LIMIT)?;
    let empty = run(&search, "?!", 5)?;

    // Then
    assert_eq!(found["results"], text_results()?);
    assert_eq!(found["semantic"]["reason"], "index_unavailable");
    assert_eq!(found["index"]["status"], "index_unavailable");
    assert_eq!(found["index"]["skipped_history_lines"], 0);
    assert!(found["index"]["reason"]
        .as_str()
        .is_some_and(|reason| !reason.is_empty()));
    assert!(found.get("error").is_none());
    assert_eq!(empty["results"], json!([]));
    assert_eq!(empty["semantic"]["reason"], "index_unavailable");
    assert_eq!(empty["index"], found["index"]);
    assert_eq!(empty["error"]["code"], "empty_query");
    assert_eq!(loads.load(Ordering::SeqCst), 0);
    Ok(())
}

#[test]
fn index_meta_damaged_after_reconcile_makes_the_index_unavailable_not_superseded() -> TestResult {
    for (damage, key) in [
        ("DELETE FROM meta WHERE key = 'model_id'", "model_id"),
        (
            "UPDATE meta SET value = '' WHERE key = 'model_revision'",
            "model_revision",
        ),
        (
            "UPDATE meta SET value = 'many' WHERE key = 'model_dimension'",
            "model_dimension",
        ),
        (
            "UPDATE meta SET value = '0' WHERE key = 'model_dimension'",
            "model_dimension",
        ),
        (
            "UPDATE meta SET value = '0' WHERE key = 'schema_version'",
            "schema_version",
        ),
        (
            "UPDATE meta SET value = '9' WHERE key = 'chunk_rule_version'",
            "chunk_rule_version",
        ),
        (
            "UPDATE meta SET value = 'short' WHERE key = 'workspace_digest'",
            "workspace_digest",
        ),
    ] {
        for query in [QUERY, "?!"] {
            // Given: a trigger that damages the meta while reconcile commits.
            let fixture = memory_fixture()?;
            run(&fixture.search(fake_loader())?, QUERY, LIMIT)?;
            Connection::open(fixture.index_file()?)?.execute_batch(&format!(
                "CREATE TRIGGER damage AFTER UPDATE ON meta \
                 WHEN NEW.key = 'skipped_history_lines' BEGIN {damage}; END;"
            ))?;

            // When
            let found = run(&fixture.search(fake_loader())?, query, LIMIT)?;

            // Then
            let case = format!("{key} {query}");
            assert_eq!(found["index"]["status"], "index_unavailable", "{case}");
            assert!(
                found["index"]["reason"]
                    .as_str()
                    .is_some_and(|reason| reason.contains(key)),
                "{case}"
            );
            assert_eq!(found["index"]["skipped_history_lines"], 0, "{case}");
            if query == QUERY {
                assert_eq!(found["semantic"]["reason"], "index_unavailable", "{case}");
                assert_eq!(found["results"], text_results()?, "{case}");
            } else {
                assert_eq!(found["semantic"]["reason"], "not_requested", "{case}");
                assert_eq!(found["error"]["code"], "empty_query", "{case}");
            }
        }
    }
    Ok(())
}

#[test]
fn details_and_reasons_keep_at_most_five_hundred_characters() -> TestResult {
    // Given: an unreadable MEMORY.md whose path alone is over 600 characters long.
    let root = tempfile::tempdir()?;
    let mut workspace = root.path().join("workspace");
    for part in 0..6 {
        workspace.push(format!("{part}{}", "w".repeat(99)));
    }
    fs::create_dir_all(workspace.join("memory").join("MEMORY.md"))?;
    let data_dir = root.path().join("data");
    fs::create_dir_all(&data_dir)?;
    let search = MemorySearch::new(
        MemorySearchPaths::new(&data_dir)?,
        workspace,
        Arc::new(LazyEmbedder::new(Arc::new(fake_loader()))),
    );

    // When
    let found = run(&search, QUERY, LIMIT)?;

    // Then
    assert_eq!(found["error"]["code"], "source_unreadable");
    for text in [
        &found["semantic"]["detail"],
        &found["index"]["reason"],
        &found["error"]["detail"],
    ] {
        assert_eq!(text.as_str().map(|text| text.chars().count()), Some(500));
    }
    Ok(())
}

/// Writes a vector holding NaN into the row of `target` during the first vector computation,
/// as a damaged index or another program could.
#[derive(Clone)]
struct CorruptingEmbedder {
    index_file: PathBuf,
    target: &'static str,
    corrupted: Arc<AtomicBool>,
}

impl CorruptingEmbedder {
    fn corrupt(&self) -> Result<(), Box<dyn Error>> {
        let not_a_number: Vec<u8> = (0..DIMENSION)
            .flat_map(|_| f32::NAN.to_le_bytes())
            .collect();
        Connection::open(&self.index_file)?.execute(
            "UPDATE items SET vector = ?1 WHERE text = ?2",
            rusqlite::params![not_a_number, self.target],
        )?;
        Ok(())
    }
}

impl Embedder for CorruptingEmbedder {
    fn model_id(&self) -> &str {
        "test/fake"
    }

    fn revision(&self) -> &str {
        "r1"
    }

    fn dimension(&self) -> usize {
        DIMENSION
    }

    fn embed(&self, kind: EmbeddingKind, texts: &[&str]) -> Result<Vec<Vec<f32>>, EmbeddingError> {
        if kind == EmbeddingKind::Passage && !self.corrupted.swap(true, Ordering::SeqCst) {
            self.corrupt()
                .map_err(|error| EmbeddingError::Compute(error.to_string()))?;
        }
        fake().embed(kind, texts)
    }
}

impl EmbedderLoader for CorruptingEmbedder {
    fn model(&self) -> IndexModel {
        IndexModel::of(self)
    }

    fn load(&self) -> Result<Arc<dyn Embedder>, LoadError> {
        Ok(Arc::new(self.clone()))
    }
}

#[test]
fn a_stored_vector_found_invalid_when_read_takes_no_part_in_semantic_search() -> TestResult {
    // Given: every vector stored, then a new item whose computation damages an old row.
    let fixture = Fixture::new()?;
    fixture.write_memory("- 사과 하나\n- 사과 둘\n")?;
    run(&fixture.search(fake_loader())?, "zzz", 5)?;
    fixture.write_memory("- 사과 하나\n- 사과 둘\n- 사과 셋\n")?;
    let search = fixture.search(CorruptingEmbedder {
        index_file: fixture.index_file()?,
        target: "- 사과 하나",
        corrupted: Arc::new(AtomicBool::new(false)),
    })?;

    // When
    let found = run(&search, "zzz", 5)?;

    // Then
    assert_eq!(found["semantic"]["status"], "available");
    let mut found_texts = texts(&found);
    found_texts.sort_unstable();
    assert_eq!(found_texts, ["- 사과 둘", "- 사과 셋"]);
    Ok(())
}

/// Drops the index's `meta` table during the first vector computation, and replaces
/// `MEMORY.md` by a folder too when `hide_memory` is set.
#[derive(Clone)]
struct BreakingEmbedder {
    index_file: PathBuf,
    memory_file: PathBuf,
    hide_memory: bool,
    broken: Arc<AtomicBool>,
}

impl BreakingEmbedder {
    fn break_index(&self) -> Result<(), Box<dyn Error>> {
        Connection::open(&self.index_file)?.execute_batch("DROP TABLE meta")?;
        if self.hide_memory {
            fs::remove_file(&self.memory_file)?;
            fs::create_dir(&self.memory_file)?;
        }
        Ok(())
    }
}

impl Embedder for BreakingEmbedder {
    fn model_id(&self) -> &str {
        "test/fake"
    }

    fn revision(&self) -> &str {
        "r1"
    }

    fn dimension(&self) -> usize {
        DIMENSION
    }

    fn embed(&self, kind: EmbeddingKind, texts: &[&str]) -> Result<Vec<Vec<f32>>, EmbeddingError> {
        if kind == EmbeddingKind::Passage && !self.broken.swap(true, Ordering::SeqCst) {
            self.break_index()
                .map_err(|error| EmbeddingError::Compute(error.to_string()))?;
        }
        fake().embed(kind, texts)
    }
}

impl EmbedderLoader for BreakingEmbedder {
    fn model(&self) -> IndexModel {
        IndexModel::of(self)
    }

    fn load(&self) -> Result<Arc<dyn Embedder>, LoadError> {
        Ok(Arc::new(self.clone()))
    }
}

#[test]
fn an_index_failing_after_reconcile_is_replaced_by_the_memory_files_read_again() -> TestResult {
    for hide_memory in [false, true] {
        // Given
        let fixture = memory_fixture()?;
        let embedder = BreakingEmbedder {
            index_file: fixture.index_file()?,
            memory_file: fixture.memory_file(),
            hide_memory,
            broken: Arc::new(AtomicBool::new(false)),
        };
        let search = fixture.search(embedder)?;

        // When
        let found = run(&search, QUERY, LIMIT)?;

        // Then
        if hide_memory {
            assert_eq!(found["results"], json!([]));
            assert_eq!(found["semantic"]["reason"], "source_unreadable");
            assert_eq!(found["index"]["status"], "index_unavailable");
            assert_eq!(found["error"]["code"], "source_unreadable");
        } else {
            assert_eq!(found["results"], text_results()?);
            assert_eq!(found["semantic"]["reason"], "index_unavailable");
            assert_eq!(found["index"]["status"], "index_unavailable");
            assert_eq!(found["index"]["skipped_history_lines"], 0);
            assert!(found["index"]["reason"]
                .as_str()
                .is_some_and(|reason| reason.contains("meta")));
            assert!(found.get("error").is_none());
        }
    }
    Ok(())
}

/// Rebuilds the index for another model during its first vector computation, as another
/// process configured with that model could.
#[derive(Clone)]
struct OvertakenEmbedder {
    fixture_paths: MemorySearchPaths,
    workspace: PathBuf,
    rebuilt: Arc<AtomicBool>,
}

impl Embedder for OvertakenEmbedder {
    fn model_id(&self) -> &str {
        "test/fake"
    }

    fn revision(&self) -> &str {
        "r1"
    }

    fn dimension(&self) -> usize {
        DIMENSION
    }

    fn embed(&self, kind: EmbeddingKind, texts: &[&str]) -> Result<Vec<Vec<f32>>, EmbeddingError> {
        if kind == EmbeddingKind::Passage && !self.rebuilt.swap(true, Ordering::SeqCst) {
            let other = FakeEmbedder::new("test/other", "r9", DIMENSION, FakeEmbedding::Hashed);
            MemoryIndex::reconcile(
                &self.fixture_paths,
                &self.workspace,
                &IndexModel::of(&other),
            )
            .map_err(|error| EmbeddingError::Compute(error.to_string()))?;
        }
        fake().embed(kind, texts)
    }
}

impl EmbedderLoader for OvertakenEmbedder {
    fn model(&self) -> IndexModel {
        IndexModel::of(self)
    }

    fn load(&self) -> Result<Arc<dyn Embedder>, LoadError> {
        Ok(Arc::new(self.clone()))
    }
}

#[test]
fn an_index_rebuilt_for_another_model_meanwhile_is_searched_by_text_and_called_superseded(
) -> TestResult {
    // Given
    let fixture = memory_fixture()?;
    let search = fixture.search(OvertakenEmbedder {
        fixture_paths: fixture.paths()?,
        workspace: fixture.workspace.clone(),
        rebuilt: Arc::new(AtomicBool::new(false)),
    })?;

    // When
    let found = run(&search, QUERY, LIMIT)?;

    // Then
    assert_eq!(found["results"], text_results()?);
    assert_eq!(found["semantic"]["reason"], "index_superseded");
    assert!(found["semantic"]["detail"]
        .as_str()
        .is_some_and(|detail| detail.contains("test/other@r9")));
    assert_eq!(found["index"]["status"], "incomplete");
    assert_eq!(found["index"]["vectors_remaining"], 6);
    Ok(())
}

#[derive(Clone)]
struct PanickingEmbedder;

impl Embedder for PanickingEmbedder {
    fn model_id(&self) -> &str {
        "test/fake"
    }

    fn revision(&self) -> &str {
        "r1"
    }

    fn dimension(&self) -> usize {
        DIMENSION
    }

    fn embed(&self, kind: EmbeddingKind, texts: &[&str]) -> Result<Vec<Vec<f32>>, EmbeddingError> {
        assert_eq!(
            kind,
            EmbeddingKind::Passage,
            "this embedder panics on queries"
        );
        fake().embed(kind, texts)
    }
}

impl EmbedderLoader for PanickingEmbedder {
    fn model(&self) -> IndexModel {
        IndexModel::of(self)
    }

    fn load(&self) -> Result<Arc<dyn Embedder>, LoadError> {
        Ok(Arc::new(self.clone()))
    }
}

#[test]
fn a_search_after_one_that_panicked_is_refused_without_reconciling() -> TestResult {
    // Given
    let fixture = memory_fixture()?;
    let search = Arc::new(fixture.search(PanickingEmbedder)?);
    let panicking = Arc::clone(&search);
    let panicked = thread::spawn(move || panicking.search(QUERY, LIMIT)).join();
    let items = |fixture: &Fixture| -> Result<i64, Box<dyn Error>> {
        Ok(
            Connection::open(fixture.index_file()?).and_then(|connection| {
                connection.query_row("SELECT count(*) FROM items", [], |row| row.get(0))
            })?,
        )
    };
    let before = items(&fixture)?;
    fixture.write_memory("- 새로 적은 항목\n")?;

    // When
    let refused = run(&search, QUERY, LIMIT)?;

    // Then
    assert!(panicked.is_err());
    let detail = "an earlier memory search in this process panicked; restart shacs-bot";
    assert_eq!(
        refused,
        json!({
            "results": [],
            "semantic": {"status": "unavailable", "reason": "index_unavailable", "detail": detail},
            "index": {"status": "index_unavailable", "reason": detail},
            "error": {"code": "search_unavailable", "detail": detail}
        })
    );
    assert_eq!(items(&fixture)?, before);
    Ok(())
}

#[test]
fn unreadable_memory_files_give_no_results_and_say_why() -> TestResult {
    // Given
    let fixture = Fixture::new()?;
    fs::create_dir(fixture.memory_file())?;
    let loads = Arc::new(AtomicUsize::new(0));
    let search = fixture.search(CountingLoader {
        loads: loads.clone(),
    })?;

    // When
    let found = run(&search, QUERY, LIMIT)?;

    // Then
    let detail = found["error"]["detail"].as_str().unwrap_or_default();
    assert!(detail.contains("MEMORY.md"), "{detail}");
    assert_eq!(
        found,
        json!({
            "results": [],
            "semantic": {"status": "unavailable", "reason": "source_unreadable", "detail": detail},
            "index": {"status": "index_unavailable", "reason": detail},
            "error": {"code": "source_unreadable", "detail": detail}
        })
    );
    assert_eq!(loads.load(Ordering::SeqCst), 0);
    Ok(())
}

struct ScriptedProvider {
    replies: Mutex<VecDeque<LlmResponse>>,
}

impl ProviderClient for ScriptedProvider {
    fn chat(&self, _: ProviderRequest) -> Result<LlmResponse, ProviderError> {
        self.replies
            .lock()
            .ok()
            .and_then(|mut replies| replies.pop_front())
            .ok_or_else(|| ProviderError::Api {
                status: None,
                message: "no scripted reply".to_owned(),
                retryable: false,
                headers: BTreeMap::new(),
                body: None,
            })
    }

    fn chat_stream(
        &self,
        request: ProviderRequest,
        on_event: &mut dyn FnMut(ProviderEvent),
    ) -> Result<LlmResponse, ProviderError> {
        let reply = self.chat(request)?;
        for call in &reply.tool_calls {
            let input = Value::Object(call.arguments.clone());
            on_event(ProviderEvent::ToolCallStart {
                id: call.id.clone(),
                name: call.name.clone(),
            });
            on_event(ProviderEvent::ToolCallDelta {
                id: call.id.clone(),
                delta: input.to_string(),
            });
            on_event(ProviderEvent::ToolCallReady {
                id: call.id.clone(),
                name: call.name.clone(),
                input,
            });
        }
        Ok(reply)
    }
}

/// One user turn in which the model calls `memory_search` and then answers: the answer and the
/// tool result the model was given.
fn turn(
    fixture: &Fixture,
    search: Arc<MemorySearch>,
) -> Result<(Option<String>, Value), Box<dyn Error>> {
    let mut tools = ToolRegistry::new();
    tools.register(MemorySearchTool::new(search));
    let arguments = json!({"query": QUERY});
    let provider = ScriptedProvider {
        replies: Mutex::new(VecDeque::from([
            LlmResponse {
                finish_reason: "tool_calls".to_owned(),
                tool_calls: vec![ToolCallRequest::new(
                    "call-memory",
                    "memory_search",
                    arguments.as_object().cloned().unwrap_or_default(),
                )],
                ..LlmResponse::default()
            },
            LlmResponse {
                content: Some(ANSWER.to_owned()),
                ..LlmResponse::default()
            },
        ])),
    };
    let mut agent = AgentLoop::new(
        MessageBus::new(),
        SessionManager::new(&fixture.workspace)?,
        ContextBuilder::new(&fixture.workspace),
        &tools,
        &provider,
        AgentLoopConfig::new(&fixture.workspace, "test-model"),
    );
    let outcome = agent.process_message(InboundMessage::new(
        "cli",
        "user",
        "direct",
        "치과 예약 언제였지?",
    ))?;
    let session = SessionManager::new(&fixture.workspace)?
        .load_existing(&outcome.session_key)
        .ok_or("the turn left no session")?;
    let content = session
        .messages
        .iter()
        .find(|message| message["role"] == "tool" && message["name"] == "memory_search")
        .and_then(|message| message["content"].as_str())
        .ok_or("the turn has no memory_search result")?;
    Ok((outcome.final_content, serde_json::from_str(content)?))
}
