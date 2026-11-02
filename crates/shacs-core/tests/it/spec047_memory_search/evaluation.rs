use super::support::*;
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use shacs_core::memory_search::{
    self, ItemSource, LazyEmbedder, MemoryIndex, MemoryItem, MemorySearch, MemorySearchPaths,
    VerifiedModelLoader, DEFAULT_MEMORY_MODEL_ID,
};
use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::path::Path;
use std::process::Command;
use std::sync::Arc;
use std::time::{Duration, Instant};
use std::{env, fs};

const HISTORY: &str = include_str!("fixture/history.jsonl");
const MEMORY: &str = include_str!("fixture/MEMORY.md");
const QUESTIONS: &str = include_str!("questions.json");
const TOP: usize = 3;
const KINDS: [&str; 5] = ["particle", "abbreviation", "paraphrase", "two_char", "date"];

#[derive(Debug, Deserialize)]
struct QuestionSet {
    timing_query_id: String,
    questions: Vec<Question>,
}

#[derive(Debug, Deserialize)]
struct Question {
    id: String,
    kind: String,
    query: String,
    answers: Vec<Answer>,
}

#[derive(Debug, Deserialize)]
struct Answer {
    file: String,
    #[serde(default)]
    cursor: Option<u64>,
    #[serde(default)]
    line: Option<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Place {
    History(u64),
    Memory(usize, usize),
}

impl Place {
    fn of(source: &ItemSource) -> Self {
        match source {
            ItemSource::History { cursor, .. } => Self::History(*cursor),
            ItemSource::Memory {
                first_line,
                last_line,
            } => Self::Memory(*first_line, *last_line),
        }
    }

    fn of_result(source: &Value) -> Self {
        let number = |key: &str| source[key].as_u64().unwrap_or(0);
        if source.get("cursor").is_some() {
            Self::History(number("cursor"))
        } else {
            Self::Memory(number("first_line") as usize, number("last_line") as usize)
        }
    }
}

impl Answer {
    fn is(&self, place: Place) -> bool {
        match (self.file.as_str(), place) {
            ("memory/history.jsonl", Place::History(cursor)) => self.cursor == Some(cursor),
            ("memory/MEMORY.md", Place::Memory(first, last)) => {
                self.line.is_some_and(|line| (first..=last).contains(&line))
            }
            _ => false,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
struct Tally {
    hits: usize,
    questions: usize,
}

impl Tally {
    fn rate(self) -> f64 {
        self.hits as f64 / self.questions.max(1) as f64
    }

    fn to_json(self) -> Value {
        json!({"hits": self.hits, "questions": self.questions, "rate": self.rate()})
    }
}

#[derive(Debug, Default, PartialEq)]
struct HitRates {
    overall: Tally,
    by_kind: BTreeMap<String, Tally>,
}

impl HitRates {
    fn of(outcomes: &[(&str, bool)]) -> Self {
        let mut rates = Self::default();
        for (kind, hit) in outcomes {
            for tally in [
                &mut rates.overall,
                rates.by_kind.entry((*kind).to_owned()).or_default(),
            ] {
                tally.questions += 1;
                tally.hits += usize::from(*hit);
            }
        }
        rates
    }

    fn kind(&self, kind: &str) -> f64 {
        self.by_kind.get(kind).copied().unwrap_or_default().rate()
    }

    fn to_json(&self) -> Value {
        let by_kind: serde_json::Map<String, Value> = self
            .by_kind
            .iter()
            .map(|(kind, tally)| (kind.clone(), tally.to_json()))
            .collect();
        json!({"overall": self.overall.to_json(), "by_kind": by_kind})
    }
}

/// Whether one of the first [`TOP`] places is an answer, and the keys of those places.
fn top_three(question: &Question, found: &[(String, Place)]) -> (Vec<String>, bool) {
    let top = &found[..found.len().min(TOP)];
    let hit = top
        .iter()
        .any(|(_, place)| question.answers.iter().any(|answer| answer.is(*place)));
    (top.iter().map(|(key, _)| key.clone()).collect(), hit)
}

/// The baseline: items holding the most distinct query terms in their search text, then in
/// order T; items holding none are not found.
fn grep_baseline<'a>(items: &'a [MemoryItem], query: &str) -> Vec<&'a MemoryItem> {
    let terms = memory_search::query_terms(query);
    let mut held: Vec<(&MemoryItem, usize)> = items
        .iter()
        .map(|item| {
            let text = memory_search::search_text(&item.text);
            let count = terms
                .iter()
                .filter(|term| text.contains(term.as_str()))
                .count();
            (item, count)
        })
        .filter(|(_, count)| *count > 0)
        .collect();
    held.sort_by(|first, second| {
        second.1.cmp(&first.1).then_with(|| {
            memory_search::tie_order(
                (&first.0.source, &first.0.key),
                (&second.0.source, &second.0.key),
            )
        })
    });
    held.into_iter().take(TOP).map(|(item, _)| item).collect()
}

struct Evaluation {
    rows: Vec<Value>,
    memory_search: HitRates,
    grep: HitRates,
    semantic_available: bool,
}

fn evaluate(
    search: &MemorySearch,
    workspace: &Path,
    questions: &QuestionSet,
) -> Result<Evaluation, Box<dyn Error>> {
    let snapshot = memory_search::read_memory_snapshot(workspace)?;
    let mut rows = Vec::new();
    let mut searched = Vec::new();
    let mut grepped = Vec::new();
    let mut semantic_available = true;
    for question in &questions.questions {
        let result: Value = serde_json::from_str(&search.search(&question.query, TOP))?;
        semantic_available &= result["semantic"]["status"] == "available";
        let found: Vec<(String, Place)> = result["results"]
            .as_array()
            .map(Vec::as_slice)
            .unwrap_or_default()
            .iter()
            .map(|item| {
                (
                    item["key"].as_str().unwrap_or_default().to_owned(),
                    Place::of_result(&item["source"]),
                )
            })
            .collect();
        let baseline: Vec<(String, Place)> = grep_baseline(&snapshot.items, &question.query)
            .into_iter()
            .map(|item| (item.key.clone(), Place::of(&item.source)))
            .collect();
        let (search_keys, search_hit) = top_three(question, &found);
        let (grep_keys, grep_hit) = top_three(question, &baseline);
        rows.push(json!({
            "id": question.id,
            "kind": question.kind,
            "query": question.query,
            "memory_search": {"top3": search_keys, "hit": search_hit},
            "grep": {"top3": grep_keys, "hit": grep_hit},
        }));
        searched.push((question.kind.as_str(), search_hit));
        grepped.push((question.kind.as_str(), grep_hit));
    }
    Ok(Evaluation {
        rows,
        memory_search: HitRates::of(&searched),
        grep: HitRates::of(&grepped),
        semantic_available,
    })
}

fn evaluation_fixture() -> Result<Fixture, Box<dyn Error>> {
    let fixture = Fixture::new()?;
    fixture.write_memory(MEMORY)?;
    fs::write(fixture.history_file(), HISTORY)?;
    Ok(fixture)
}

#[test]
fn the_grep_baseline_ranks_by_distinct_terms_then_order_t_and_skips_items_without_any() -> TestResult
{
    // Given
    let fixture = Fixture::new()?;
    fixture.write_memory("- 사과 바나나\n- 사과\n- 포도\n")?;
    fixture.write_history(&[
        (1, "2026-10-01 09:00", "사과"),
        (2, "2026-10-02 09:00", "사과 바나나"),
    ])?;
    let snapshot = memory_search::read_memory_snapshot(&fixture.workspace)?;

    // When
    let found = grep_baseline(&snapshot.items, "사과, 바나나!");

    // Then
    let places: Vec<Place> = found.iter().map(|item| Place::of(&item.source)).collect();
    assert_eq!(
        places,
        [Place::Memory(1, 1), Place::History(2), Place::Memory(2, 2)]
    );
    assert!(grep_baseline(&snapshot.items, "수박").is_empty());
    Ok(())
}

#[test]
fn a_question_is_a_hit_when_an_answer_is_in_the_top_three_and_rates_are_per_kind() -> TestResult {
    // Given
    let question = |answer: Answer| Question {
        id: "Q".to_owned(),
        kind: "date".to_owned(),
        query: String::new(),
        answers: vec![answer],
    };
    let history = |cursor| Answer {
        file: "memory/history.jsonl".to_owned(),
        cursor: Some(cursor),
        line: None,
    };
    let memory = |line| Answer {
        file: "memory/MEMORY.md".to_owned(),
        cursor: None,
        line: Some(line),
    };
    let found: Vec<(String, Place)> = [
        Place::History(5),
        Place::Memory(3, 4),
        Place::History(7),
        Place::History(9),
    ]
    .into_iter()
    .enumerate()
    .map(|(index, place)| (format!("k{index}"), place))
    .collect();

    // When
    let hits = [
        top_three(&question(history(7)), &found).1,
        top_three(&question(history(9)), &found).1,
        top_three(&question(memory(4)), &found).1,
        top_three(&question(memory(5)), &found).1,
        top_three(&question(memory(5)), &[]).1,
    ];
    let rates = HitRates::of(&[("particle", true), ("particle", false), ("date", true)]);

    // Then
    assert_eq!(hits, [true, false, true, false, false]);
    assert_eq!(
        top_three(&question(history(7)), &found).0,
        ["k0", "k1", "k2"]
    );
    assert_eq!(
        rates.overall,
        Tally {
            hits: 2,
            questions: 3
        }
    );
    assert!((rates.kind("particle") - 0.5).abs() < f64::EPSILON);
    assert!((rates.kind("date") - 1.0).abs() < f64::EPSILON);
    Ok(())
}

#[test]
fn the_evaluation_questions_cover_every_kind_and_point_at_fixture_items() -> TestResult {
    // Given
    let questions: QuestionSet = serde_json::from_str(QUESTIONS)?;
    let fixture = evaluation_fixture()?;
    let snapshot = memory_search::read_memory_snapshot(&fixture.workspace)?;
    let search = fixture.search(fake_loader())?;

    // When
    let evaluation = evaluate(&search, &fixture.workspace, &questions)?;

    // Then
    assert!(questions.questions.len() >= 30);
    let ids: BTreeSet<&str> = questions
        .questions
        .iter()
        .map(|question| question.id.as_str())
        .collect();
    assert_eq!(ids.len(), questions.questions.len());
    assert!(ids.contains(questions.timing_query_id.as_str()));
    let kinds: BTreeSet<&str> = questions
        .questions
        .iter()
        .map(|question| question.kind.as_str())
        .collect();
    assert_eq!(kinds, BTreeSet::from(KINDS));
    for question in &questions.questions {
        assert!(!question.answers.is_empty(), "{}", question.id);
        for answer in &question.answers {
            assert!(
                snapshot
                    .items
                    .iter()
                    .any(|item| answer.is(Place::of(&item.source))),
                "{}",
                question.id
            );
        }
    }
    assert!(evaluation.semantic_available);
    assert_eq!(evaluation.rows.len(), questions.questions.len());
    for rates in [&evaluation.memory_search, &evaluation.grep] {
        assert_eq!(rates.overall.questions, questions.questions.len());
        assert!(rates.overall.hits <= rates.overall.questions);
    }
    Ok(())
}

fn rss_kib() -> Result<u64, Box<dyn Error>> {
    let output = Command::new("ps")
        .args(["-o", "rss=", "-p", &std::process::id().to_string()])
        .output()?;
    Ok(String::from_utf8(output.stdout)?.trim().parse()?)
}

fn copy_tree(from: &Path, to: &Path) -> TestResult {
    fs::create_dir_all(to)?;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else {
            fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}

fn sha256(text: &str) -> String {
    format!("{:x}", Sha256::digest(text.as_bytes()))
}

fn millis(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1000.0
}

/// PRD 010 decision 12, with the default model installed under `SHACS_SPEC047_MODEL_DATA_DIR`;
/// the report goes to `SHACS_SPEC047_REPORT` before the acceptance check.
pub fn real_model() -> TestResult {
    let model_data_dir = env::var_os("SHACS_SPEC047_MODEL_DATA_DIR")
        .ok_or("SHACS_SPEC047_MODEL_DATA_DIR must name a data directory with the default model")?;
    let report_path =
        env::var_os("SHACS_SPEC047_REPORT").ok_or("SHACS_SPEC047_REPORT must name the report")?;
    let manifest = memory_search::model_manifest(DEFAULT_MEMORY_MODEL_ID)
        .ok_or("the default model has no manifest")?;
    let questions: QuestionSet = serde_json::from_str(QUESTIONS)?;
    let timing = questions
        .questions
        .iter()
        .find(|question| question.id == questions.timing_query_id)
        .ok_or("the timing question is missing")?;
    let fixture = evaluation_fixture()?;
    let paths = fixture.paths()?;
    copy_tree(
        &MemorySearchPaths::new(Path::new(&model_data_dir))?.model_dir(manifest),
        &paths.model_dir(manifest),
    )?;
    let embedder = Arc::new(LazyEmbedder::new(Arc::new(VerifiedModelLoader::new(
        paths.clone(),
        manifest,
    ))));

    let rss_before = rss_kib()?;
    let loading = Instant::now();
    embedder.get()?;
    let load = loading.elapsed();
    let rss_after = rss_kib()?;

    let pending = memory_search::read_memory_snapshot(&fixture.workspace)?
        .items
        .len();
    let search = MemorySearch::new(paths.clone(), fixture.workspace.clone(), embedder.clone());
    let started = Instant::now();
    let first: Value = serde_json::from_str(&search.search(&timing.query, TOP))?;
    let first_search = started.elapsed();
    let completed = MemoryIndex::reconcile(&paths, &fixture.workspace, &embedder.model())?
        .complete_vectors(embedder.get()?.as_ref())?;

    search.search(&timing.query, TOP);
    let mut times = Vec::new();
    for _ in 0..10 {
        let started = Instant::now();
        search.search(&timing.query, TOP);
        times.push(started.elapsed());
    }
    let mut sorted = times.clone();
    sorted.sort_unstable();
    let median = (sorted[4] + sorted[5]) / 2;

    let evaluation = evaluate(&search, &fixture.workspace, &questions)?;
    let report = json!({
        "model": {"id": manifest.id, "revision": manifest.revision},
        "fixture_sha256": {
            "history.jsonl": sha256(HISTORY),
            "MEMORY.md": sha256(MEMORY),
            "questions.json": sha256(QUESTIONS),
        },
        "platform": {
            "os": env::consts::OS,
            "arch": env::consts::ARCH,
            "profile": if cfg!(debug_assertions) { "debug" } else { "release" },
        },
        "memory_kib": {
            "before_load": rss_before,
            "after_load": rss_after,
            "increase": rss_after.saturating_sub(rss_before),
        },
        "load_ms": millis(load),
        "first_search": {
            "ms": millis(first_search),
            "vectors_pending_before": pending,
            "index_after": first["index"],
            "semantic": first["semantic"],
        },
        "index_completed": {"items": completed.items, "vectors_failed": completed.vectors_failed},
        "timing_query": {"id": timing.id, "query": timing.query},
        "search_ms": times.iter().copied().map(millis).collect::<Vec<_>>(),
        "search_median_ms": millis(median),
        "semantic_available_for_every_question": evaluation.semantic_available,
        "hit_rates": {
            "memory_search": evaluation.memory_search.to_json(),
            "grep": evaluation.grep.to_json(),
        },
        "questions": evaluation.rows,
    });
    fs::write(&report_path, serde_json::to_string_pretty(&report)?)?;

    assert!(evaluation.semantic_available);
    assert!(
        evaluation.memory_search.overall.rate() >= evaluation.grep.overall.rate(),
        "overall {:?} against grep {:?}",
        evaluation.memory_search.overall,
        evaluation.grep.overall
    );
    for kind in ["paraphrase", "abbreviation"] {
        assert!(
            evaluation.memory_search.kind(kind) > evaluation.grep.kind(kind),
            "{kind}: {} against grep {}",
            evaluation.memory_search.kind(kind),
            evaluation.grep.kind(kind)
        );
    }
    Ok(())
}
