use serde_json::{json, Value};
use shacs_cli::{parse_cli_args, run_command};
use shacs_core::memory_search::{self, MemorySearchPaths};
use std::error::Error;
use std::fs;
use std::path::PathBuf;

struct Memory {
    _root: tempfile::TempDir,
    data_dir: PathBuf,
    config: String,
    workspace: String,
}

/// Two `MEMORY.md` items and one history entry, with `config` as the configuration.
fn memory(config: &Value) -> Result<Memory, Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let data_dir = root.path().join("data");
    let workspace = root.path().join("workspace");
    fs::create_dir_all(&data_dir)?;
    fs::create_dir_all(workspace.join("memory"))?;
    fs::write(
        workspace.join("memory").join("MEMORY.md"),
        "- 치과 예약은 화요일 오후 3시\n- 점심 메뉴\n",
    )?;
    fs::write(
        workspace.join("memory").join("history.jsonl"),
        format!(
            "{}\n",
            json!({"cursor": 7, "timestamp": "2026-10-01 09:00", "content": "치과 예약을 다시 잡았다"})
        ),
    )?;
    let config_path = data_dir.join("config.json");
    fs::write(&config_path, config.to_string())?;
    Ok(Memory {
        config: config_path.to_str().ok_or("path")?.to_owned(),
        workspace: workspace.to_str().ok_or("path")?.to_owned(),
        data_dir,
        _root: root,
    })
}

fn memory_command(memory: &Memory, arguments: &[&str]) -> Result<String, String> {
    let mut all = vec!["memory"];
    all.extend_from_slice(arguments);
    all.extend(["--config", &memory.config, "-w", &memory.workspace]);
    parse_cli_args(all)
        .and_then(run_command)
        .map_err(|error| error.to_string())
}

#[test]
fn memory_search_shows_people_every_fact_of_the_result_and_programs_its_json(
) -> Result<(), Box<dyn Error>> {
    // Given
    let memory = memory(&json!({"agents": {"defaults": {"memorySearch": {"enable": false}}}}))?;

    // When
    let readable = memory_command(&memory, &["search", "치과 예약"])?;
    let json = memory_command(&memory, &["search", "치과 예약", "--json"])?;
    let one = memory_command(&memory, &["search", "치과 예약", "--limit", "1", "--json"])?;
    let empty = memory_command(&memory, &["search", "?!"])?;

    // Then
    let json: Value = serde_json::from_str(&json)?;
    let results = json["results"].as_array().ok_or("no results")?;
    assert_eq!(results.len(), 2);
    for item in results {
        let key = item["key"].as_str().ok_or("no key")?;
        let heading = format!(
            "{}. [{}] ",
            item["rank"],
            item["matched_by"].as_str().ok_or("no matched_by")?
        );
        let source = if item["source"]["cursor"].is_null() {
            format!(
                "{} lines {}-{}",
                item["source"]["file"].as_str().ok_or("no file")?,
                item["source"]["first_line"],
                item["source"]["last_line"]
            )
        } else {
            format!(
                "{} cursor {} ({}) item {}",
                item["source"]["file"].as_str().ok_or("no file")?,
                item["source"]["cursor"],
                item["source"]["timestamp"].as_str().ok_or("no timestamp")?,
                item["source"]["item"]
            )
        };
        let text = item["text"].as_str().ok_or("no text")?;
        assert!(
            readable.contains(&format!("{heading}{source} (key {key})\n   {text}")),
            "{readable}"
        );
    }
    for expected in [
        "1. [text] memory/MEMORY.md lines 1-1 (key m:",
        "2. [text] memory/history.jsonl cursor 7 (2026-10-01 09:00) item 0 (key h:7:0:",
        "semantic: unavailable: disabled (agents.defaults.memorySearch.enable is false)",
        "index: incomplete; vectors remaining: 3; vectors failed: 0; skipped history lines: 0",
    ] {
        assert!(readable.contains(expected), "{readable}");
    }
    assert!(!readable.contains("점심"), "{readable}");
    let one: Value = serde_json::from_str(&one)?;
    assert_eq!(one["results"].as_array().map(Vec::len), Some(1));
    assert_eq!(one["semantic"]["reason"], "disabled");
    for expected in [
        "no results",
        "semantic: unavailable: not_requested",
        "error: empty_query",
    ] {
        assert!(empty.contains(expected), "{empty}");
    }
    Ok(())
}

#[test]
fn memory_search_refuses_bad_arguments_and_settings_and_index_refuses_unknown_models(
) -> Result<(), Box<dyn Error>> {
    // Given
    let usable = memory(&json!({}))?;
    let small_limit = memory(&json!({"agents": {"defaults": {"maxToolResultChars": 999}}}))?;
    let unknown_model =
        memory(&json!({"agents": {"defaults": {"memorySearch": {"model": "someone/else"}}}}))?;

    // When / Then
    for arguments in [
        &["search"][..],
        &["search", "치과", "예약"],
        &["search", "치과", "--limit", "0"],
        &["search", "치과", "--limit", "21"],
        &["search", "치과", "--limit", "many"],
        &["search", "치과", "--limit"],
    ] {
        let refused = memory_command(&usable, arguments).err().unwrap_or_default();
        assert!(
            refused.starts_with("invalid CLI arguments"),
            "{arguments:?}: {refused}"
        );
    }
    let limit = memory_command(&small_limit, &["search", "치과"])
        .err()
        .unwrap_or_default();
    assert!(limit.contains("maxToolResultChars"), "{limit}");
    for action in [&["search", "치과"][..], &["index"]] {
        let refused = memory_command(&unknown_model, action)
            .err()
            .unwrap_or_default();
        for expected in [
            "someone/else",
            "dragonkue/multilingual-e5-small-ko-v2",
            "nlpai-lab/KURE-v1",
        ] {
            assert!(refused.contains(expected), "{action:?}: {refused}");
        }
    }
    Ok(())
}

#[test]
fn memory_index_with_semantic_search_off_updates_the_text_index_only() -> Result<(), Box<dyn Error>>
{
    // Given
    let memory = memory(&json!({"agents": {"defaults": {"memorySearch": {"enable": false}}}}))?;

    // When
    let indexed = memory_command(&memory, &["index"])?;

    // Then
    for expected in [
        "model: dragonkue/multilingual-e5-small-ko-v2@",
        "items: 3",
        "vectors remaining: 3",
        "semantic search is off",
    ] {
        assert!(indexed.contains(expected), "{indexed}");
    }
    assert!(!MemorySearchPaths::new(&memory.data_dir)?
        .models_dir()
        .exists());
    Ok(())
}

#[test]
fn memory_index_without_a_model_indexes_the_text_and_says_to_install_the_model(
) -> Result<(), Box<dyn Error>> {
    // Given
    let root = tempfile::tempdir()?;
    let data_dir = root.path().join("data");
    let workspace = root.path().join("workspace");
    fs::create_dir_all(&data_dir)?;
    fs::create_dir_all(workspace.join("memory"))?;
    let config = data_dir.join("config.json");
    fs::write(&config, b"{}")?;
    fs::write(
        workspace.join("memory").join("MEMORY.md"),
        "- 커피는 라떼\n- 출근은 9시\n",
    )?;

    // When
    let indexed = run_command(parse_cli_args([
        "memory",
        "index",
        "--config",
        config.to_str().ok_or("path")?,
        "-w",
        workspace.to_str().ok_or("path")?,
    ])?);

    // Then
    let message = indexed
        .err()
        .ok_or("indexing without a model must fail")?
        .to_string();
    for expected in [
        "items: 2",
        "vectors remaining: 2",
        "model file config.json is missing",
        "shacs-bot memory model install",
    ] {
        assert!(message.contains(expected), "{message}");
    }
    let paths = MemorySearchPaths::new(&data_dir)?;
    assert!(paths
        .index_file(&memory_search::workspace_digest(&workspace)?)
        .is_file());
    assert!(!paths.models_dir().exists());
    Ok(())
}

#[test]
fn memory_commands_refuse_unknown_models_and_arguments() -> Result<(), Box<dyn Error>> {
    // When
    let unknown = run_command(parse_cli_args([
        "memory",
        "model",
        "install",
        "--model",
        "someone/else",
    ])?);

    // Then
    let message = unknown
        .err()
        .ok_or("an unknown model must be refused")?
        .to_string();
    for known in ["dragonkue/multilingual-e5-small-ko-v2", "nlpai-lab/KURE-v1"] {
        assert!(message.contains(known), "{message}");
    }
    for arguments in [
        &["memory"][..],
        &["memory", "model"],
        &["memory", "model", "remove"],
        &["memory", "index", "--model", "nlpai-lab/KURE-v1"],
    ] {
        assert!(
            parse_cli_args(arguments.iter().copied()).is_err(),
            "{arguments:?}"
        );
    }
    Ok(())
}
