use super::support::*;
use serde_json::{json, Value};
use shacs_config::{AgentDefaults, Config, ConfigError};
use shacs_core::memory_search::{
    MemorySearch, MemorySearchSettings, DEFAULT_MEMORY_MODEL_ID, MIN_RESULT_CHARS,
};
use std::error::Error;
use std::fs;

fn chars(json: &str) -> usize {
    json.chars().count()
}

/// Three items of `length` characters made of `unit`, all holding the query "가".
fn long_items(unit: &str, length: usize) -> Result<Fixture, Box<dyn Error>> {
    let fixture = Fixture::new()?;
    let text: String = unit.chars().cycle().take(length).collect();
    fixture.write_memory(&format!("- 1{text}\n- 2{text}\n- 3{text}\n"))?;
    Ok(fixture)
}

fn limited(fixture: &Fixture, max_chars: usize) -> Result<MemorySearch, Box<dyn Error>> {
    Ok(fixture
        .search(fake_loader())?
        .with_semantic(false)
        .with_max_result_chars(max_chars))
}

#[test]
fn a_limit_of_zero_keeps_every_text_whole() -> TestResult {
    // Given
    let fixture = long_items("가나다", 30_000)?;

    // When
    let found: Value = serde_json::from_str(&limited(&fixture, 0)?.search("가", 5))?;

    // Then
    assert!(texts(&found)
        .iter()
        .all(|text| text.chars().count() == 30_003));
    assert!(found["results"].as_array().is_some_and(|items| items
        .iter()
        .all(|item| item.get("text_truncated").is_none())));
    Ok(())
}

#[test]
fn configuration_accepts_no_limit_or_at_least_a_thousand_characters_and_only_pinned_models(
) -> TestResult {
    // Given
    let with_limit = |max_tool_result_chars| AgentDefaults {
        max_tool_result_chars,
        ..AgentDefaults::default()
    };
    let mut unknown = AgentDefaults::default();
    unknown.memory_search.model = "someone/else".to_owned();
    let config: Config = serde_json::from_value(json!({
        "agents": {"defaults": {"memorySearch": {"enable": false}}}
    }))?;

    // When
    let accepted: Vec<usize> = [0, MIN_RESULT_CHARS, 16_000]
        .into_iter()
        .map(|limit| MemorySearchSettings::from_config(&with_limit(limit)))
        .map(|settings| settings.map(|settings| settings.max_result_chars))
        .collect::<Result<_, _>>()?;
    let refused = [1, MIN_RESULT_CHARS - 1]
        .map(|limit| MemorySearchSettings::from_config(&with_limit(limit)));
    let unknown = MemorySearchSettings::from_config(&unknown);

    // Then
    assert_eq!(accepted, [0, MIN_RESULT_CHARS, 16_000]);
    for refusal in refused {
        assert!(
            matches!(&refusal, Err(ConfigError::Invalid(reason)) if reason.contains("maxToolResultChars")),
            "{refusal:?}"
        );
    }
    let Err(ConfigError::Invalid(reason)) = unknown else {
        return Err("an unknown model must be refused".into());
    };
    for known in ["someone/else", DEFAULT_MEMORY_MODEL_ID, "nlpai-lab/KURE-v1"] {
        assert!(reason.contains(known), "{reason}");
    }
    let defaults = MemorySearchSettings::from_config(&AgentDefaults::default())?;
    assert_eq!(defaults.manifest.id, DEFAULT_MEMORY_MODEL_ID);
    assert!(defaults.semantic);
    assert_eq!(
        AgentDefaults::default().memory_search.model,
        DEFAULT_MEMORY_MODEL_ID
    );
    assert!(!config.agents.defaults.memory_search.enable);
    assert_eq!(
        config.agents.defaults.memory_search.model,
        DEFAULT_MEMORY_MODEL_ID
    );
    Ok(())
}

#[test]
fn a_result_as_long_as_the_limit_is_kept_and_one_character_more_cuts_the_last_text() -> TestResult {
    // Given
    let fixture = long_items("가나다", 700)?;
    let full = limited(&fixture, 0)?.search("가", 5);
    let length = chars(&full);
    let whole: Value = serde_json::from_str(&full)?;

    // When
    let at_limit = limited(&fixture, length)?.search("가", 5);
    let over_by_one = limited(&fixture, length - 1)?.search("가", 5);

    // Then
    assert!(length > MIN_RESULT_CHARS);
    assert_eq!(at_limit, full);
    assert_eq!(chars(&over_by_one), length - 1);
    let cut: Value = serde_json::from_str(&over_by_one)?;
    assert_eq!(keys(&cut), keys(&whole));
    let original = texts(&whole);
    let kept = texts(&cut);
    assert_eq!(kept[..2], original[..2]);
    assert!(original[2].starts_with(&kept[2]) && kept[2].len() < original[2].len());
    assert_eq!(
        cut["results"][2]["text_truncated"],
        json!({"original_chars": original[2].chars().count()})
    );
    for item in 0..3 {
        assert_eq!(
            cut["results"][item]["source"],
            whole["results"][item]["source"]
        );
    }
    assert!(cut["results"][1].get("text_truncated").is_none());
    Ok(())
}

#[test]
fn a_limit_of_a_thousand_empties_the_last_texts_first_and_cuts_the_next() -> TestResult {
    // Given
    let fixture = long_items("가나다", 700)?;

    // When
    let cut = limited(&fixture, MIN_RESULT_CHARS)?.search("가", 5);

    // Then
    assert_eq!(chars(&cut), MIN_RESULT_CHARS);
    let cut: Value = serde_json::from_str(&cut)?;
    let kept = texts(&cut);
    assert_eq!(kept.len(), 3);
    assert!(kept[0].starts_with("- 1가나다") && kept[0].chars().count() < 703);
    assert_eq!(kept[1..], ["", ""]);
    for item in 0..3 {
        assert_eq!(
            cut["results"][item]["text_truncated"],
            json!({"original_chars": 703})
        );
    }
    Ok(())
}

#[test]
fn json_escapes_count_toward_the_limit() -> TestResult {
    // Given: quotes, backslashes and a control character take 2 to 6 characters in JSON.
    let fixture = long_items("\"\\\u{1}가", 900)?;
    let full = limited(&fixture, 0)?.search("가", 5);

    // When
    let cut = limited(&fixture, chars(&full) - 100)?.search("가", 5);

    // Then
    let limit = chars(&full) - 100;
    assert!(
        chars(&cut) <= limit && chars(&cut) > limit - 6,
        "{}",
        chars(&cut)
    );
    let cut: Value = serde_json::from_str(&cut)?;
    let whole: Value = serde_json::from_str(&full)?;
    let (original, kept) = (texts(&whole), texts(&cut));
    assert!(original[2].starts_with(&kept[2]));
    assert!(kept[2].chars().count() < original[2].chars().count());
    Ok(())
}

#[test]
fn a_result_too_long_even_without_texts_becomes_the_fixed_result() -> TestResult {
    // Given
    let fixture = Fixture::new()?;
    let lines: Vec<String> = (0..20)
        .map(|number| format!("- 감자 {number:02}"))
        .collect();
    fixture.write_memory(&lines.join("\n"))?;
    let broken = Fixture::new()?;
    broken.write_memory(&lines.join("\n"))?;
    run(&broken.search(fake_loader())?, "감자", 20)?;
    fs::write(broken.index_file()?, b"not a SQLite database at all")?;
    let error = json!({
        "code": "result_limit_exceeded",
        "detail": "result exceeds maxToolResultChars"
    });

    // When
    let indexed = limited(&fixture, MIN_RESULT_CHARS)?.search("감자", 20);
    let unindexed = limited(&broken, MIN_RESULT_CHARS)?.search("감자", 20);

    // Then
    assert!(chars(&indexed) <= MIN_RESULT_CHARS && chars(&unindexed) <= MIN_RESULT_CHARS);
    assert_eq!(
        serde_json::from_str::<Value>(&indexed)?,
        json!({
            "results": [],
            "semantic": {"status": "unavailable", "reason": "disabled"},
            "index": {
                "status": "incomplete",
                "vectors_remaining": 20,
                "vectors_failed": 0,
                "skipped_history_lines": 0
            },
            "error": error
        })
    );
    assert_eq!(
        serde_json::from_str::<Value>(&unindexed)?,
        json!({
            "results": [],
            "semantic": {"status": "unavailable", "reason": "index_unavailable"},
            "index": {"status": "index_unavailable", "reason": "omitted", "skipped_history_lines": 0},
            "error": error
        })
    );
    Ok(())
}
