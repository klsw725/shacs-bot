use super::support::*;
use serde_json::{json, Value};
use shacs_core::memory_search::{self, ItemSource, MAX_QUERY_TERMS, RANKED_LIST_LIMIT};
use shacs_core::runtime::{build_subagent_tool_registry, SubagentExecutionConfig};
use shacs_core::tools::{MemorySearchTool, Tool, ToolRegistry, ToolResult};
use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use unicode_normalization::UnicodeNormalization;

#[test]
fn query_terms_are_nfc_lowercase_words_split_at_spaces_punctuation_symbols_and_controls(
) -> TestResult {
    // Given
    let decomposed: String = "회의록 Café".nfd().collect();
    let query = format!("ACME_{decomposed}, 회의록!! \"인용\" x—y\t+₩★ snake_case 회의록");
    let many: Vec<String> = (0..40).map(|number| format!("w{number}")).collect();

    // When
    let terms = memory_search::query_terms(&query);
    let capped = memory_search::query_terms(&format!("w0 W0 {}", many.join(" ")));

    // Then
    assert_eq!(
        terms,
        ["acme", "회의록", "café", "인용", "x", "y", "snake", "case"]
    );
    assert_eq!(capped.len(), MAX_QUERY_TERMS);
    assert_eq!(capped.first().map(String::as_str), Some("w0"));
    assert_eq!(capped.last().map(String::as_str), Some("w31"));
    assert!(memory_search::query_terms(" ?! ★\n\u{7}").is_empty());
    Ok(())
}

#[test]
fn an_nfd_item_is_found_by_an_nfc_query_and_returned_as_written() -> TestResult {
    // Given
    let fixture = Fixture::new()?;
    let decomposed: String = "- 회의록 정리 Café".nfd().collect();
    fixture.write_memory(&format!("{decomposed}\n- 다른 항목\n"))?;
    let search = fixture.search(fake_loader())?.with_semantic(false);

    // When
    let found = run(&search, "회의록 café", 5)?;

    // Then
    assert_eq!(texts(&found), [decomposed]);
    Ok(())
}

#[test]
fn mixed_case_items_are_found_by_queries_in_any_case() -> TestResult {
    // Given
    let fixture = Fixture::new()?;
    fixture.write_memory("- Meeting with ACME Corp\n- 다른 항목\n")?;
    let search = fixture.search(fake_loader())?.with_semantic(false);

    // When / Then
    for query in ["acme", "ACME", "Acme corp"] {
        assert_eq!(
            texts(&run(&search, query, 5)?),
            ["- Meeting with ACME Corp"],
            "{query}"
        );
    }
    Ok(())
}

#[test]
fn a_query_without_terms_is_refused_after_reconciling_without_loading_the_model() -> TestResult {
    // Given
    let fixture = Fixture::new()?;
    fixture.write_memory("- 하나\n- 둘\n")?;
    fixture.write_history(&[(1, "2026-10-01 09:00", "기록")])?;
    fs::OpenOptions::new()
        .append(true)
        .open(fixture.history_file())?
        .write_all(b"not json\n")?;
    let loads = Arc::new(AtomicUsize::new(0));
    let search = fixture.search(CountingLoader {
        loads: loads.clone(),
    })?;

    // When
    let refused = run(&search, " ?! ★ ", 5)?;

    // Then
    assert_eq!(refused["results"], json!([]));
    assert_eq!(refused["error"]["code"], "empty_query");
    assert_eq!(refused["semantic"]["status"], "unavailable");
    assert_eq!(refused["semantic"]["reason"], "not_requested");
    assert_eq!(
        refused["index"],
        json!({
            "status": "incomplete",
            "vectors_remaining": 3,
            "vectors_failed": 0,
            "skipped_history_lines": 1
        })
    );
    assert_eq!(loads.load(Ordering::SeqCst), 0);
    Ok(())
}

#[test]
fn terms_shorter_than_three_characters_are_compared_directly() -> TestResult {
    // Given
    let fixture = Fixture::new()?;
    fixture.write_memory("- 치과 예약\n- 치킨 주문\n- 과일 가게\n")?;
    let search = fixture.search(fake_loader())?.with_semantic(false);

    // When
    let two = run(&search, "치과", 5)?;
    let one = run(&search, "과", 5)?;

    // Then
    assert_eq!(texts(&two), ["- 치과 예약"]);
    assert_eq!(texts(&one), ["- 치과 예약", "- 과일 가게"]);
    Ok(())
}

#[test]
fn long_terms_are_matched_as_quoted_fts_phrases_even_when_they_are_fts_keywords() -> TestResult {
    // Given
    let fixture = Fixture::new()?;
    fixture
        .write_memory("- 회의록 공유\n- near the station\n- bread and butter\n- nothing here\n")?;
    let search = fixture.search(fake_loader())?.with_semantic(false);

    // When
    let found = run(&search, "회의록 OR NEAR \"AND\" -", 10)?;

    // Then
    let mut found_texts = texts(&found);
    found_texts.sort_unstable();
    assert_eq!(
        found_texts,
        ["- bread and butter", "- near the station", "- 회의록 공유"]
    );
    assert_eq!(found["index"]["status"], "incomplete");
    Ok(())
}

#[test]
fn text_ranking_counts_distinct_terms_then_bm25_then_order_t() -> TestResult {
    // Given: lines 2 and 3 are as long, line 3 has its term twice, line 5 repeats line 2, and
    // line 6 holds two terms but only one long term once, so its bm25 is worse than line 3's.
    let fixture = Fixture::new()?;
    let mut memory = String::from(
        "- 우유 한 잔\n- 바나나 키위키\n- 바나나 바나나\n- 바나나와 딸기잼\n- 바나나 키위키\n\
         - 바나나 우유\n",
    );
    for filler in 0..6 {
        memory.push_str(&format!("- 기타 메모 {filler}\n"));
    }
    fixture.write_memory(&memory)?;
    let search = fixture.search(fake_loader())?.with_semantic(false);

    // When
    let ranked = run(&search, "바나나 딸기잼 우유", 10)?;

    // Then
    assert_eq!(first_lines(&ranked), [4, 6, 3, 2, 5, 1]);
    assert!(matched_by(&ranked).iter().all(|matched| matched == "text"));
    Ok(())
}

#[test]
fn items_whose_scores_are_equal_follow_order_t() -> TestResult {
    // Given: the same text twice in MEMORY.md and three times in the history.
    let fixture = Fixture::new()?;
    fixture.write_memory("# 메모\n- 같은 문장\n\n- 같은 문장\n")?;
    fixture.write_history(&[
        (1, "2026-10-01 09:00", "- 같은 문장"),
        (3, "2026-10-03 09:00", "- 같은 문장\n- 같은 문장"),
    ])?;
    let search = fixture.search(fake_loader())?;
    let order_t = |result: &Value| -> Vec<(u64, u64, u64)> {
        result["results"]
            .as_array()
            .map(|items| {
                items
                    .iter()
                    .map(|item| {
                        let source = &item["source"];
                        (
                            source["first_line"].as_u64().unwrap_or(0),
                            source["cursor"].as_u64().unwrap_or(0),
                            source["item"].as_u64().unwrap_or(0),
                        )
                    })
                    .collect()
            })
            .unwrap_or_default()
    };
    let expected = [(2, 0, 0), (4, 0, 0), (0, 3, 0), (0, 3, 1), (0, 1, 0)];

    // When
    let by_meaning = run(&search, "zzz", 5)?;
    let by_both = run(&search, "같은", 5)?;
    let by_text = run(
        &fixture.search(fake_loader())?.with_semantic(false),
        "같은",
        5,
    )?;

    // Then
    assert_eq!(by_meaning["semantic"]["status"], "available");
    for (result, matched) in [
        (&by_meaning, "semantic"),
        (&by_both, "both"),
        (&by_text, "text"),
    ] {
        assert_eq!(order_t(result), expected, "{matched}");
        assert!(
            matched_by(result).iter().all(|found| found == matched),
            "{matched}"
        );
    }
    Ok(())
}

#[test]
fn order_t_puts_memory_lines_first_then_newer_history_then_position_then_key() {
    // Given
    let memory = |line| ItemSource::Memory {
        first_line: line,
        last_line: line,
    };
    let history = |cursor, ordinal| ItemSource::History {
        cursor,
        timestamp: String::new(),
        ordinal,
    };
    let mut items = [
        (history(5, 1), "h:b"),
        (memory(9), "m:a"),
        (history(5, 0), "h:z"),
        (history(7, 0), "h:x"),
        (memory(2), "m:z"),
        (history(5, 0), "h:y"),
    ];

    // When
    items.sort_by(|first, second| {
        memory_search::tie_order((&first.0, first.1), (&second.0, second.1))
    });

    // Then
    let keys: Vec<&str> = items.iter().map(|(_, key)| *key).collect();
    assert_eq!(keys, ["m:z", "m:a", "h:x", "h:y", "h:z", "h:b"]);
}

#[test]
fn semantic_ranking_compares_directions_not_lengths() -> TestResult {
    // Given: the longer vector points further from the query's direction.
    let fixture = Fixture::new()?;
    fixture.write_memory("- 짧은 벡터\n- 긴 벡터\n")?;
    let passages = BTreeMap::from([
        ("- 짧은 벡터".to_owned(), vec![1.0, 0.0]),
        ("- 긴 벡터".to_owned(), vec![3.0, 1.0]),
    ]);

    // When
    let found = run(&fixture.search(TableEmbedder { passages })?, "zzz", 2)?;

    // Then
    assert_eq!(first_lines(&found), [1, 2]);
    assert_eq!(matched_by(&found), ["semantic", "semantic"]);
    Ok(())
}

/// Sixty `MEMORY.md` items holding "사과", in text ranks 1 to 60, of which only the one at
/// `line` has a vector.
fn text_list_with_one_vector(line: usize) -> Result<Value, Box<dyn std::error::Error>> {
    let fixture = Fixture::new()?;
    let lines: Vec<String> = (1..=60)
        .map(|number| format!("- 사과 {number:02}"))
        .collect();
    fixture.write_memory(&lines.join("\n"))?;
    let passages = BTreeMap::from([(lines[line - 1].clone(), at_rank(1))]);
    run(&fixture.search(TableEmbedder { passages })?, "사과", 3)
}

#[test]
fn only_the_first_fifty_text_matches_take_part_in_merging() -> TestResult {
    // When
    let ranked_fiftieth = text_list_with_one_vector(RANKED_LIST_LIMIT)?;
    let ranked_fifty_first = text_list_with_one_vector(RANKED_LIST_LIMIT + 1)?;

    // Then
    assert_eq!(first_lines(&ranked_fiftieth), [50, 1, 2]);
    assert_eq!(matched_by(&ranked_fiftieth), ["both", "text", "text"]);
    assert_eq!(first_lines(&ranked_fifty_first), [1, 51, 2]);
    assert_eq!(
        matched_by(&ranked_fifty_first),
        ["text", "semantic", "text"]
    );
    Ok(())
}

/// Two text matches, of which the one at line 2 has the vector of semantic rank `rank`, and
/// sixty other items holding the other semantic ranks from 1 to 61.
fn semantic_list_with_second_match_at(rank: usize) -> Result<Value, Box<dyn std::error::Error>> {
    let fixture = Fixture::new()?;
    let mut lines = vec!["- 사과 하나".to_owned(), "- 사과 둘".to_owned()];
    let mut passages = BTreeMap::from([(lines[1].clone(), at_rank(rank))]);
    let other_ranks = (1..=61).filter(|other| *other != rank);
    for (number, other_rank) in other_ranks.enumerate() {
        let line = format!("- 배 {number:02}");
        passages.insert(line.clone(), at_rank(other_rank));
        lines.push(line);
    }
    fixture.write_memory(&lines.join("\n"))?;
    run(&fixture.search(TableEmbedder { passages })?, "사과", 3)
}

#[test]
fn only_the_first_fifty_semantic_matches_take_part_in_merging() -> TestResult {
    // When
    let ranked_fiftieth = semantic_list_with_second_match_at(RANKED_LIST_LIMIT)?;
    let ranked_fifty_first = semantic_list_with_second_match_at(RANKED_LIST_LIMIT + 1)?;

    // Then
    assert_eq!(first_lines(&ranked_fiftieth), [2, 1, 3]);
    assert_eq!(matched_by(&ranked_fiftieth), ["both", "text", "semantic"]);
    assert_eq!(first_lines(&ranked_fifty_first), [1, 3, 2]);
    assert_eq!(
        matched_by(&ranked_fifty_first),
        ["text", "semantic", "text"]
    );
    Ok(())
}

#[test]
fn merging_adds_one_over_sixty_plus_rank_from_each_list_with_equal_weight() -> TestResult {
    // Given: text ranks 1 to 5 at lines 1 to 5; semantic ranks 5, 2, 1, 3 and 4 at lines 2, 5,
    // 6, 7 and 8.
    let fixture = Fixture::new()?;
    let lines = [
        "- 포도 1",
        "- 포도 2",
        "- 포도 3",
        "- 포도 4",
        "- 포도 5",
        "- 키위 a",
        "- 키위 b",
        "- 키위 c",
    ];
    fixture.write_memory(&lines.join("\n"))?;
    let passages = [(1, 5), (4, 2), (5, 1), (6, 3), (7, 4)]
        .into_iter()
        .map(|(index, rank)| (lines[index].to_owned(), at_rank(rank)))
        .collect();
    let search = fixture.search(TableEmbedder { passages })?;

    // When
    let merged = run(&search, "포도", 8)?;

    // Then
    assert_eq!(first_lines(&merged), [2, 5, 1, 6, 3, 7, 4, 8]);
    assert_eq!(
        matched_by(&merged),
        ["both", "both", "text", "semantic", "text", "semantic", "text", "semantic"]
    );
    Ok(())
}

#[test]
fn merged_scores_that_are_equal_as_fractions_follow_order_t() -> TestResult {
    // Given: 1/63 + 1/84 = 1/72 + 1/72, for text rank 3 with semantic rank 24 (line 3) and text
    // rank 12 with semantic rank 12 (line 12); the other items are in one list only.
    let fixture = Fixture::new()?;
    let mut lines: Vec<String> = (1..=12)
        .map(|number| format!("- 포도 {number:02}"))
        .collect();
    let mut passages = BTreeMap::from([
        (lines[2].clone(), at_rank(24)),
        (lines[11].clone(), at_rank(12)),
    ]);
    for (number, rank) in (1..24).filter(|rank| *rank != 12).enumerate() {
        let line = format!("- 키위 {number:02}");
        passages.insert(line.clone(), at_rank(rank));
        lines.push(line);
    }
    fixture.write_memory(&lines.join("\n"))?;

    // When
    let merged = run(&fixture.search(TableEmbedder { passages })?, "포도", 2)?;

    // Then
    assert_eq!(first_lines(&merged), [3, 12]);
    assert_eq!(matched_by(&merged), ["both", "both"]);
    Ok(())
}

#[test]
fn a_search_returns_at_most_its_limit_and_never_more_than_twenty_items() -> TestResult {
    // Given
    let fixture = Fixture::new()?;
    let lines: Vec<String> = (0..30)
        .map(|number| format!("- 감자 {number:02}"))
        .collect();
    fixture.write_memory(&lines.join("\n"))?;
    let search = Arc::new(fixture.search(fake_loader())?.with_semantic(false));
    let tool = MemorySearchTool::new(search.clone());
    let mut registry = ToolRegistry::new();
    registry.register(MemorySearchTool::new(search.clone()));

    // When
    let counts: Vec<usize> = [0, 1, 3, 20, 21, 50]
        .into_iter()
        .map(|limit| run(&search, "감자", limit).map(|result| texts(&result).len()))
        .collect::<Result<_, _>>()?;
    let by_default = registry.execute("memory_search", json!({"query": "감자"}));
    let refused: Vec<String> = [json!(0), json!(21), json!("many")]
        .into_iter()
        .map(|limit| {
            registry
                .execute("memory_search", json!({"query": "감자", "limit": limit}))
                .into_text()
        })
        .collect();

    // Then
    assert_eq!(counts, [1, 1, 3, 20, 20, 20]);
    let ToolResult::Text(by_default) = by_default else {
        return Err("memory_search must answer with text".into());
    };
    assert_eq!(texts(&serde_json::from_str(&by_default)?).len(), 5);
    for refusal in refused {
        assert!(
            refusal.starts_with("Error: Invalid parameters"),
            "{refusal}"
        );
    }
    assert_eq!(tool.name(), "memory_search");
    assert!(tool.read_only());
    assert!(!tool.concurrency_safe());
    Ok(())
}

#[test]
fn subagents_get_no_memory_search() -> TestResult {
    // Given
    let fixture = Fixture::new()?;
    let mut config = SubagentExecutionConfig::new(&fixture.workspace, "test-model");
    config.allow_side_effect_tools = true;

    // When
    let tools = build_subagent_tool_registry(&config);

    // Then
    assert!(tools.has("read_file"));
    assert!(!tools.has("memory_search"));
    Ok(())
}
