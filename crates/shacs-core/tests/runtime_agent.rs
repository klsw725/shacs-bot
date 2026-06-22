use serde_json::{json, Map, Value};
use shacs_core::runtime::{
    pick_consolidation_boundary, AgentHook, AgentHookContext, AgentRunSpec, AgentRunner,
    AudioContextAnalysis, AudioContextAnalyzer, AudioContextRequest, CompositeHook,
    ContainerNetworkMode, ContainerRuntimeKind, ContainmentSnapshotRef, ContextBuildRequest,
    ContextBuilder, DockerContainmentSnapshot, Dream, DreamProcessor, InboundMessage,
    MemoryGitBoundary, MemoryLineAge, MemoryStore, MessageBus, MessageBusError, OutboundMessage,
    PermissionMode, PermissionModeSnapshot, PermissionRuleInput, ProcExecSummary,
    ProviderArchiveConsolidator, ProviderMemoryConsolidator, Session, SessionHistoryOptions,
    SessionManager, SkillsLoader, StreamDeltaCoalescer, SubagentManager, TokenConsolidationConfig,
    ToolExecutionContext, ToolSearchConfig, ToolSearchMode, VideoContextAnalysis,
    VideoContextAnalyzer, VideoContextRequest, VideoMetadata,
};
use shacs_core::tools::{
    AskUserTool, JsonMap, SchemaFragment, StringSchema, Tool, ToolParameters, ToolRegistry,
    ToolResult,
};
use shacs_providers::{
    LlmResponse, ProviderClient, ProviderError, ProviderEvent, ProviderRequest, ToolCallRequest,
};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::error::Error;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

#[test]
fn agent_init_export_parity_aliases_compile() {
    let _dream: Option<Dream<'static>> = None;
    let _skills_loader: Option<SkillsLoader> = None;
    let _subagent_manager: Option<SubagentManager> = None;
}

#[test]
fn runtime_bus_preserves_message_shapes_and_sizes() -> Result<(), Box<dyn Error>> {
    let bus = MessageBus::new();
    let mut inbound = InboundMessage::new("telegram", "user-1", "chat-1", "hello");
    inbound.session_key_override = Some("thread-1".to_owned());
    inbound
        .metadata
        .insert("_wants_stream".to_owned(), json!(true));
    bus.publish_inbound(inbound.clone());
    bus.publish_outbound(OutboundMessage::new("telegram", "chat-1", "hi"));

    if bus.inbound_size() != 1 || bus.outbound_size() != 1 {
        return Err("message bus size accounting drifted".into());
    }
    let consumed = bus.consume_inbound().ok_or("missing inbound")?;
    if consumed.session_key() != "thread-1" || consumed.metadata["_wants_stream"] != true {
        return Err(format!("inbound shape drifted: {consumed:?}").into());
    }
    let outbound = bus.consume_outbound().ok_or("missing outbound")?;
    if outbound.channel != "telegram" || outbound.content != "hi" || bus.outbound_size() != 0 {
        return Err(format!("outbound shape drifted: {outbound:?}").into());
    }
    Ok(())
}

#[test]
fn bounded_bus_rejects_when_capacity_exceeded() -> Result<(), Box<dyn Error>> {
    let bus = MessageBus::bounded(2);
    bus.try_publish_inbound(InboundMessage::new("cli", "user", "direct", "first"))?;
    bus.try_publish_inbound(InboundMessage::new("cli", "user", "direct", "second"))?;

    let third = bus.try_publish_inbound(InboundMessage::new("cli", "user", "direct", "third"));
    if third != Err(MessageBusError::QueueFull { capacity: 2 }) {
        return Err(format!("bounded queue error drifted: {third:?}").into());
    }

    let first = bus.consume_inbound().ok_or("missing first inbound")?;
    let second = bus.consume_inbound().ok_or("missing second inbound")?;
    if first.content != "first" || second.content != "second" || bus.consume_inbound().is_some() {
        return Err(format!("bounded queue FIFO drifted: {first:?}, {second:?}").into());
    }
    Ok(())
}

#[test]
fn runtime_bus_supports_blocking_consume_and_serde_defaults() -> Result<(), Box<dyn Error>> {
    let inbound: InboundMessage = serde_json::from_value(json!({
        "channel": "cli",
        "sender_id": "user",
        "chat_id": "direct",
        "content": "hello"
    }))?;
    if inbound.session_key() != "cli:direct"
        || !inbound.media.is_empty()
        || !inbound.metadata.is_empty()
    {
        return Err(format!("inbound serde defaults drifted: {inbound:?}").into());
    }
    let outbound: OutboundMessage = serde_json::from_value(json!({
        "channel": "cli",
        "chat_id": "direct",
        "content": "hi"
    }))?;
    if outbound.reply_to.is_some() || !outbound.buttons.is_empty() {
        return Err(format!("outbound serde defaults drifted: {outbound:?}").into());
    }

    let bus = MessageBus::new();
    let publisher = bus.clone();
    thread::spawn(move || {
        thread::sleep(Duration::from_millis(20));
        publisher.publish_inbound(InboundMessage::new("cli", "user", "direct", "later"));
    });
    if bus.try_consume_inbound().is_some() {
        return Err("try_consume_inbound should remain non-blocking".into());
    }
    let consumed = bus.consume_inbound_blocking();
    if consumed.content != "later" {
        return Err(format!("blocking consume returned wrong message: {consumed:?}").into());
    }
    Ok(())
}

#[test]
fn runtime_bus_drain_matching_preserves_retained_fifo_order() -> Result<(), Box<dyn Error>> {
    let bus = MessageBus::new();
    for content in ["keep-a", "take-1", "keep-b", "take-2", "keep-c"] {
        bus.publish_inbound(InboundMessage::new("cli", "user", content, content));
    }

    let drained = bus.drain_inbound_matching(1, |message| message.content.starts_with("take"));
    if drained
        .iter()
        .map(|message| message.content.as_str())
        .collect::<Vec<_>>()
        != ["take-1"]
    {
        return Err(format!("drained messages drifted: {drained:?}").into());
    }

    let mut retained = Vec::new();
    while let Some(message) = bus.try_consume_inbound() {
        retained.push(message.content);
    }
    if retained != ["keep-a", "keep-b", "take-2", "keep-c"] {
        return Err(format!("retained FIFO order drifted: {retained:?}").into());
    }
    Ok(())
}

#[test]
fn runtime_session_saves_loads_and_filters_history() -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    let mut manager = SessionManager::new(workspace.path())?;
    let mut session = Session::new("telegram:chat/1");
    session.add_message("tool", "orphan", Map::new());
    session.add_message("user", "hello", Map::new());
    let mut assistant_extra = Map::new();
    assistant_extra.insert("reasoning_content".to_owned(), json!("why"));
    session.add_message("assistant", "hi", assistant_extra);
    let mut delivery_extra = Map::new();
    delivery_extra.insert("_channel_delivery".to_owned(), json!(true));
    session.add_message("assistant", "delivered", delivery_extra);
    session
        .metadata
        .insert("runtime_checkpoint".to_owned(), json!({"phase": "test"}));
    manager.save(&session)?;

    let loaded = manager.get_or_create("telegram:chat/1");
    let history = loaded.get_history(120, true);
    if history.len() != 3
        || history[0]["role"] != "user"
        || !history[0]["content"]
            .as_str()
            .unwrap_or_default()
            .contains("[Message Time:")
        || history[1]["reasoning_content"] != "why"
        || history[1]["content"]
            .as_str()
            .unwrap_or_default()
            .contains("[Message Time:")
        || !history[2]["content"]
            .as_str()
            .unwrap_or_default()
            .contains("[Message Time:")
    {
        return Err(format!("session history drifted: {history:?}").into());
    }
    let raw = manager
        .read_session_file("telegram:chat/1")
        .ok_or("missing raw session")?;
    if raw["metadata"]["runtime_checkpoint"]["phase"] != "test"
        || manager.list_sessions()?.len() != 1
    {
        return Err(format!("session raw/list view drifted: {raw:?}").into());
    }
    Ok(())
}

#[test]
fn stream_coalescer_batches_text_deltas_without_session_persistence() -> Result<(), Box<dyn Error>>
{
    let mut coalescer = StreamDeltaCoalescer::new();
    if coalescer
        .push(&ProviderEvent::TextDelta {
            text: "hel".to_owned(),
        })
        .is_some()
    {
        return Err("text delta should be progress-only until flush".into());
    }
    coalescer.push(&ProviderEvent::TextDelta {
        text: "lo".to_owned(),
    });
    let batch = coalescer.flush().ok_or("missing coalesced batch")?;
    if batch.text != "hello" || !batch.reasoning.is_empty() {
        return Err(format!("coalesced stream batch drifted: {batch:?}").into());
    }

    let session = Session::new("cli:stream");
    if !session.messages.is_empty() {
        return Err(
            format!("progress deltas should not persist session messages: {session:?}").into(),
        );
    }
    Ok(())
}

#[test]
fn runtime_session_options_repair_legacy_and_lifecycle_helpers() -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    let legacy = tempfile::tempdir()?;
    let mut legacy_manager = SessionManager::new(legacy.path())?;
    let mut legacy_session = Session::new("cli:unsafe/path name");
    legacy_session.add_message("user", "legacy", Map::new());
    legacy_manager.save(&legacy_session)?;

    let mut manager =
        SessionManager::with_legacy_sessions_dir(workspace.path(), legacy.path().join("sessions"))?;
    let loaded = manager.get_or_create("cli:unsafe/path name");
    if loaded.messages.len() != 1 || loaded.messages[0]["content"] != "legacy" {
        return Err(format!("legacy session was not migrated: {loaded:?}").into());
    }
    let safe_key = SessionManager::safe_key("a:b/c d");
    if !safe_key.starts_with("a_b_c d-") || safe_key.len() <= "a_b_c d-".len() {
        return Err("safe_key no longer matches nanobot unsafe-char parity".into());
    }

    let path = manager.session_path("cli:corrupt");
    std::fs::write(
        &path,
        "{\"_type\":\"metadata\",\"key\":\"cli:corrupt\"}\nnot-json\n{\"role\":\"user\",\"content\":\"ok\"}\n",
    )?;
    let repaired = manager
        .load("cli:corrupt")
        .ok_or("corrupt session did not salvage")?;
    if repaired.messages.len() != 1 || repaired.messages[0]["content"] != "ok" {
        return Err(format!("corrupt session salvage drifted: {repaired:?}").into());
    }

    let mut session = Session::new("cli:history");
    session.add_message(
        "user",
        "first image",
        Map::from_iter([("media".to_owned(), json!(["pic.png"]))]),
    );
    session.add_message("assistant", "old", Map::new());
    session.add_message("user", "newest user with many words", Map::new());
    let history = session.get_history_with_options(SessionHistoryOptions {
        max_messages: 10,
        max_tokens: 4,
        include_timestamps: false,
    });
    if history
        .first()
        .and_then(|m| m.get("role"))
        .and_then(Value::as_str)
        != Some("user")
    {
        return Err(format!("token trimming should preserve a user boundary: {history:?}").into());
    }
    let full_history = session.get_history_with_options(SessionHistoryOptions::default());
    let history_content = full_history[0]["content"].as_str().unwrap_or_default();
    if !history_content.contains("[attachment omitted from history]")
        || history_content.contains("pic.png")
    {
        return Err(format!("media breadcrumb missing: {full_history:?}").into());
    }

    let archived = session.enforce_file_cap_with_limit(2);
    if archived.is_empty() || session.messages.len() > 2 {
        return Err(format!("file cap did not archive/trim: {session:?}, {archived:?}").into());
    }
    manager.save_with_fsync(&session)?;
    manager.invalidate("cli:history");
    if !manager.delete_session("cli:history")? {
        return Err("delete_session did not remove saved file".into());
    }
    Ok(())
}

#[cfg(unix)]
#[test]
fn runtime_session_rejects_symlink_persistence_paths() -> Result<(), Box<dyn Error>> {
    use std::os::unix::fs::symlink;

    let workspace = tempfile::tempdir()?;
    let outside = tempfile::tempdir()?;
    symlink(outside.path(), workspace.path().join("sessions"))?;
    if SessionManager::new(workspace.path()).is_ok() {
        return Err("session manager should reject symlinked sessions directory".into());
    }

    let workspace = tempfile::tempdir()?;
    let legacy = tempfile::tempdir()?;
    std::fs::create_dir_all(legacy.path().join("sessions"))?;
    let outside_file = outside.path().join("outside.jsonl");
    std::fs::write(
        &outside_file,
        "{\"role\":\"user\",\"content\":\"secret\"}\n",
    )?;
    let legacy_link = legacy
        .path()
        .join("sessions")
        .join(format!("{}.jsonl", SessionManager::safe_key("cli:legacy")));
    symlink(&outside_file, &legacy_link)?;
    let mut manager =
        SessionManager::with_legacy_sessions_dir(workspace.path(), legacy.path().join("sessions"))?;
    let loaded = manager.get_or_create("cli:legacy");
    if !loaded.messages.is_empty() {
        return Err(format!("legacy symlink should not be migrated: {loaded:?}").into());
    }
    Ok(())
}

#[test]
fn runtime_session_drops_tool_results_without_matching_assistant_call() -> Result<(), Box<dyn Error>>
{
    let mut session = Session::new("cli:direct");
    session.add_message("user", "start", Map::new());
    let mut assistant_extra = Map::new();
    assistant_extra.insert(
        "tool_calls".to_owned(),
        json!([{"id": "valid", "type": "function", "function": {"name": "repeat", "arguments": "{}"}}]),
    );
    session.add_message("assistant", "", assistant_extra);

    let mut wrong_tool = Map::new();
    wrong_tool.insert("tool_call_id".to_owned(), json!("wrong"));
    wrong_tool.insert("name".to_owned(), json!("repeat"));
    session.add_message("tool", "wrong result", wrong_tool);

    let mut valid_tool = Map::new();
    valid_tool.insert("tool_call_id".to_owned(), json!("valid"));
    valid_tool.insert("name".to_owned(), json!("repeat"));
    session.add_message("tool", "valid result", valid_tool);

    session.add_message("user", "next", Map::new());
    let mut late_tool = Map::new();
    late_tool.insert("tool_call_id".to_owned(), json!("valid"));
    late_tool.insert("name".to_owned(), json!("repeat"));
    session.add_message("tool", "late orphan", late_tool);

    let history = session.get_history(120, false);
    let contents = history
        .iter()
        .filter_map(|message| message.get("content").and_then(Value::as_str))
        .collect::<Vec<_>>();
    if !contents.is_empty() {
        return Err(format!("orphan tool cleanup drifted: {history:?}").into());
    }
    Ok(())
}

#[test]
fn runtime_session_legal_start_handles_leading_orphan_without_panic() -> Result<(), Box<dyn Error>>
{
    let messages = vec![json!({"role": "tool", "tool_call_id": "missing", "content": "orphan"})];
    if shacs_core::runtime::find_legal_message_start(&messages) != 1 {
        return Err("legal message start should skip leading orphan tool result".into());
    }
    Ok(())
}

#[test]
fn runtime_context_builds_system_runtime_and_media_messages() -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    std::fs::write(workspace.path().join("AGENTS.md"), "Be useful")?;
    let image_path = workspace.path().join("pixel.png");
    std::fs::write(&image_path, b"\x89PNG\r\n\x1a\nrest")?;
    let context = ContextBuilder::new(workspace.path()).with_timezone("Asia/Seoul");
    let media = vec![image_path.to_string_lossy().to_string()];
    let messages = context.build_messages(ContextBuildRequest {
        history: vec![json!({"role": "user", "content": "previous"})],
        current_message: "current",
        media: &media,
        channel: Some("cli"),
        chat_id: Some("direct"),
        current_role: "user",
        session_summary: Some("summary"),
    });
    if messages[0]["role"] != "system"
        || !messages[0]["content"]
            .as_str()
            .unwrap_or_default()
            .contains("AGENTS.md")
        || messages.len() != 2
        || messages[1]["content"][0]["type"] != "text"
        || !messages[1]["content"][0]["text"]
            .as_str()
            .unwrap_or_default()
            .contains("previous")
        || !messages[1]["content"][2]["image_url"]["url"]
            .as_str()
            .unwrap_or_default()
            .starts_with("data:image/png;base64,")
        || !messages[1]["content"][1]["text"]
            .as_str()
            .unwrap_or_default()
            .contains("+09:00")
    {
        return Err(format!("context messages drifted: {messages:?}").into());
    }
    Ok(())
}

#[test]
fn runtime_context_routes_media_root_stored_attachments_and_keeps_workspace_images(
) -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    std::fs::write(workspace.path().join("AGENTS.md"), "Be useful")?;

    let media_root = tempfile::tempdir()?;
    let attachments = media_root.path().join("attachments/cli");
    std::fs::create_dir_all(&attachments)?;

    let stored_image = attachments.join("att-1-image.png");
    std::fs::write(&stored_image, b"\x89PNG\r\n\x1a\nrest")?;
    let stored_text = attachments.join("att-2-note.txt");
    std::fs::write(&stored_text, "stored text")?;
    let stored_binary = attachments.join("att-3-blob.bin");
    std::fs::write(&stored_binary, [0xff, 0x00, 0x01])?;
    let stored_audio = attachments.join("att-4-sound.mp3");
    std::fs::write(&stored_audio, b"ID3")?;

    let workspace_image = workspace.path().join("workspace.png");
    std::fs::write(&workspace_image, b"\x89PNG\r\n\x1a\nrest")?;
    let outside_root = tempfile::tempdir()?;
    let outside_image = outside_root.path().join("outside.png");
    std::fs::write(&outside_image, b"\x89PNG\r\n\x1a\nrest")?;

    let context =
        ContextBuilder::new(workspace.path()).with_media_roots([media_root.path().to_path_buf()]);
    let media = vec![
        stored_image.to_string_lossy().to_string(),
        stored_text.to_string_lossy().to_string(),
        stored_binary.to_string_lossy().to_string(),
        stored_audio.to_string_lossy().to_string(),
        workspace_image.to_string_lossy().to_string(),
        outside_image.to_string_lossy().to_string(),
    ];
    let messages = context.build_messages(ContextBuildRequest {
        history: vec![],
        current_message: "current",
        media: &media,
        channel: Some("cli"),
        chat_id: Some("direct"),
        current_role: "user",
        session_summary: None,
    });

    let blocks = messages[1]["content"]
        .as_array()
        .ok_or("missing routed content blocks")?;
    if blocks.len() != 8
        || !blocks[0]["text"]
            .as_str()
            .unwrap_or_default()
            .contains("[Runtime Context")
        || blocks[1]["type"] != "image_url"
        || !blocks[1]["image_url"]["url"]
            .as_str()
            .unwrap_or_default()
            .starts_with("data:image/png;base64,")
        || !blocks[2]["text"]
            .as_str()
            .unwrap_or_default()
            .contains("[attachment:included_text]")
        || blocks[3]["text"] != "stored text"
        || !blocks[4]["text"]
            .as_str()
            .unwrap_or_default()
            .contains("[attachment:unsupported]")
        || !blocks[5]["text"]
            .as_str()
            .unwrap_or_default()
            .contains("[attachment:unsupported]")
        || !blocks[5]["text"]
            .as_str()
            .unwrap_or_default()
            .contains("audio analyzer is not configured")
        || blocks[6]["type"] != "image_url"
        || !blocks[6]["_meta"]["path"]
            .as_str()
            .unwrap_or_default()
            .contains("workspace.png")
        || blocks[7]["text"] != "current"
    {
        return Err(format!("media routing drifted: {messages:?}").into());
    }
    Ok(())
}

#[derive(Debug)]
struct RuntimeAudioAnalyzer;

impl AudioContextAnalyzer for RuntimeAudioAnalyzer {
    fn analyze(
        &self,
        request: AudioContextRequest,
    ) -> Result<AudioContextAnalysis, shacs_core::runtime::AudioContextError> {
        if request.detected_mime != "audio/mpeg" {
            return Err(shacs_core::runtime::AudioContextError::Unsupported(
                "unsupported test mime".to_owned(),
            ));
        }
        Ok(AudioContextAnalysis {
            transcript: Some("runtime transcript".to_owned()),
            summary: None,
            language: Some("en".to_owned()),
            truncated: false,
        })
    }
}

#[derive(Debug)]
struct RuntimeVideoAnalyzer;

impl VideoContextAnalyzer for RuntimeVideoAnalyzer {
    fn analyze(
        &self,
        request: VideoContextRequest,
    ) -> Result<VideoContextAnalysis, shacs_core::runtime::VideoContextError> {
        if request.detected_mime != "video/mp4" {
            return Err(shacs_core::runtime::VideoContextError::Unsupported(
                "unsupported test video mime".to_owned(),
            ));
        }
        Ok(VideoContextAnalysis {
            metadata: Some(VideoMetadata {
                duration_seconds: request.duration_seconds,
                container: Some("mp4".to_owned()),
                video_codec: Some("h264".to_owned()),
                audio_codec: None,
                width: Some(640),
                height: Some(360),
                audio_track_available: false,
                subtitle_tracks: Vec::new(),
            }),
            subtitles: None,
            scene_summary: Some("runtime video scene".to_owned()),
            keyframe_summary: None,
            extracted_audio_path: None,
            extracted_audio_mime: None,
            extracted_audio_byte_length: None,
            extracted_audio_duration_seconds: None,
            component_failures: Vec::new(),
            truncated: false,
        })
    }
}

#[test]
fn runtime_context_routes_stored_audio_with_injected_analyzer() -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    let media_root = tempfile::tempdir()?;
    let attachments = media_root.path().join("attachments/cli");
    std::fs::create_dir_all(&attachments)?;
    let stored_audio = attachments.join("att-voice.mp3");
    std::fs::write(&stored_audio, b"ID3")?;
    let context = ContextBuilder::new(workspace.path())
        .with_media_roots([media_root.path().to_path_buf()])
        .with_audio_analyzer(Arc::new(RuntimeAudioAnalyzer));
    let media = vec![stored_audio.to_string_lossy().to_string()];
    let messages = context.build_messages(ContextBuildRequest {
        current_message: "current",
        media: &media,
        channel: Some("cli"),
        chat_id: Some("direct"),
        ..ContextBuildRequest::new("current")
    });
    let blocks = messages[1]["content"]
        .as_array()
        .ok_or("missing routed content blocks")?;
    if blocks.len() != 4
        || !blocks[1]["text"]
            .as_str()
            .unwrap_or_default()
            .contains("[attachment:included_text]")
        || !blocks[2]["text"]
            .as_str()
            .unwrap_or_default()
            .contains("[Attachment content warning]")
        || !blocks[2]["text"]
            .as_str()
            .unwrap_or_default()
            .contains("[Audio transcript]\nruntime transcript")
        || blocks[3]["text"] != "current"
    {
        return Err(format!("audio analyzer routing drifted: {messages:?}").into());
    }
    Ok(())
}

#[test]
fn runtime_context_routes_stored_video_with_injected_analyzer() -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    let media_root = tempfile::tempdir()?;
    let attachments = media_root.path().join("attachments/cli");
    std::fs::create_dir_all(&attachments)?;
    let stored_video = attachments.join("att-clip.mp4");
    std::fs::write(&stored_video, mp4_video_bytes_for_runtime(6))?;
    let context = ContextBuilder::new(workspace.path())
        .with_media_roots([media_root.path().to_path_buf()])
        .with_video_analyzer(Arc::new(RuntimeVideoAnalyzer));
    let media = vec![stored_video.to_string_lossy().to_string()];
    let messages = context.build_messages(ContextBuildRequest {
        current_message: "current",
        media: &media,
        channel: Some("cli"),
        chat_id: Some("direct"),
        ..ContextBuildRequest::new("current")
    });
    let blocks = messages[1]["content"]
        .as_array()
        .ok_or("missing routed content blocks")?;
    if blocks.len() != 4
        || !blocks[1]["text"]
            .as_str()
            .unwrap_or_default()
            .contains("[attachment:included_text]")
        || !blocks[2]["text"]
            .as_str()
            .unwrap_or_default()
            .contains("[Video metadata]")
        || !blocks[2]["text"]
            .as_str()
            .unwrap_or_default()
            .contains("[Video scene summary]\nruntime video scene")
        || blocks[3]["text"] != "current"
    {
        return Err(format!("video analyzer routing drifted: {messages:?}").into());
    }
    Ok(())
}

#[test]
fn runtime_context_routes_stored_video_missing_analyzer_as_unsupported(
) -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    let media_root = tempfile::tempdir()?;
    let attachments = media_root.path().join("attachments/cli");
    std::fs::create_dir_all(&attachments)?;
    let stored_video = attachments.join("att-clip.mp4");
    std::fs::write(&stored_video, mp4_video_bytes_for_runtime(6))?;
    let context =
        ContextBuilder::new(workspace.path()).with_media_roots([media_root.path().to_path_buf()]);
    let media = vec![stored_video.to_string_lossy().to_string()];
    let messages = context.build_messages(ContextBuildRequest {
        current_message: "current",
        media: &media,
        channel: Some("cli"),
        chat_id: Some("direct"),
        ..ContextBuildRequest::new("current")
    });
    let blocks = messages[1]["content"]
        .as_array()
        .ok_or("missing routed content blocks")?;
    if blocks.len() != 3
        || !blocks[1]["text"]
            .as_str()
            .unwrap_or_default()
            .contains("[attachment:unsupported]")
        || !blocks[1]["text"]
            .as_str()
            .unwrap_or_default()
            .contains("video analyzer is not configured")
        || blocks[1]["text"]
            .as_str()
            .unwrap_or_default()
            .contains("deferred")
        || blocks[2]["text"] != "current"
    {
        return Err(format!("missing video analyzer routing drifted: {messages:?}").into());
    }
    Ok(())
}

#[test]
fn runtime_context_gates_native_image_blocks_by_capability() -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    let media_root = tempfile::tempdir()?;
    let attachments = media_root.path().join("attachments/cli");
    std::fs::create_dir_all(&attachments)?;

    let stored_image = attachments.join("att-1-image.png");
    std::fs::write(&stored_image, b"\x89PNG\r\n\x1a\nrest")?;
    let workspace_image = workspace.path().join("workspace.png");
    std::fs::write(&workspace_image, b"\x89PNG\r\n\x1a\nrest")?;

    let context = ContextBuilder::new(workspace.path())
        .with_media_roots([media_root.path().to_path_buf()])
        .with_native_image_input_supported(false);
    let media = vec![
        stored_image.to_string_lossy().to_string(),
        workspace_image.to_string_lossy().to_string(),
    ];
    let messages = context.build_messages(ContextBuildRequest {
        current_message: "current",
        media: &media,
        channel: Some("cli"),
        chat_id: Some("direct"),
        ..ContextBuildRequest::new("current")
    });

    let blocks = messages[1]["content"]
        .as_array()
        .ok_or("missing routed content blocks")?;
    if blocks.iter().any(|block| block["type"] == "image_url")
        || !blocks.iter().any(|block| {
            block["text"]
                .as_str()
                .unwrap_or_default()
                .contains("att-1-image.png")
        })
        || !blocks.iter().any(|block| {
            block["text"]
                .as_str()
                .unwrap_or_default()
                .contains("workspace.png")
        })
        || !blocks.iter().any(|block| {
            block["text"]
                .as_str()
                .unwrap_or_default()
                .contains("native image input is not supported")
        })
    {
        return Err(format!("native image capability gate drifted: {messages:?}").into());
    }
    Ok(())
}

#[test]
fn runtime_context_merges_last_same_role_history_message() -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    std::fs::write(workspace.path().join("AGENTS.md"), "Be useful")?;

    let context = ContextBuilder::new(workspace.path()).with_timezone("Asia/Seoul");
    let history = vec![
        json!({"role": "user", "content": "prior user"}),
        json!({"role": "assistant", "content": "prior assistant"}),
    ];

    let first_messages = context.build_messages(ContextBuildRequest {
        history: history.clone(),
        current_message: "current request",
        channel: Some("cli"),
        chat_id: Some("direct"),
        current_role: "assistant",
        session_summary: Some("summary"),
        ..ContextBuildRequest::new("current request")
    });
    let second_messages = context.build_messages(ContextBuildRequest {
        history,
        current_message: "current request",
        channel: Some("cli"),
        chat_id: Some("direct"),
        current_role: "assistant",
        session_summary: Some("summary"),
        ..ContextBuildRequest::new("current request")
    });

    if first_messages != second_messages {
        return Err(format!(
            "context build should be deterministic: {first_messages:?} != {second_messages:?}"
        )
        .into());
    }
    if first_messages.is_empty() || first_messages[0]["role"] != "system" {
        return Err(format!("system message should be first: {first_messages:?}").into());
    }
    if first_messages[1]["role"].as_str() != Some("user")
        || first_messages[2]["role"].as_str() != Some("assistant")
    {
        return Err(format!("history order should be preserved: {first_messages:?}").into());
    }
    if first_messages.len() != 3 {
        return Err(format!(
            "same-role merge should keep history length plus system: {first_messages:?}"
        )
        .into());
    }
    if first_messages[1]["content"].as_str() != Some("prior user")
        || !first_messages[2]["content"]
            .as_str()
            .unwrap_or_default()
            .contains("prior assistant")
        || !first_messages[2]["content"]
            .as_str()
            .unwrap_or_default()
            .contains("[Runtime Context")
        || !first_messages[2]["content"]
            .as_str()
            .unwrap_or_default()
            .contains("current request")
        || !first_messages[2]["content"]
            .as_str()
            .unwrap_or_default()
            .contains("summary")
    {
        return Err(format!("same-role context merge drifted: {first_messages:?}").into());
    }

    Ok(())
}

#[test]
fn runtime_context_injects_memory_recent_history_skills_and_helpers() -> Result<(), Box<dyn Error>>
{
    let workspace = tempfile::tempdir()?;
    std::fs::create_dir_all(workspace.path().join("memory"))?;
    std::fs::write(workspace.path().join("memory/MEMORY.md"), "remember this")?;
    std::fs::write(workspace.path().join("memory/.dream_cursor"), "1")?;
    std::fs::write(
        workspace.path().join("memory/history.jsonl"),
        "{\"cursor\":1,\"timestamp\":\"old\",\"content\":\"skip\"}\n{\"cursor\":2,\"timestamp\":\"now\",\"content\":\"keep\"}\n{\"cursor\":\"bad\",\"timestamp\":\"bad\",\"content\":\"ignore\"}\n",
    )?;
    std::fs::create_dir_all(workspace.path().join("skills/always"))?;
    std::fs::write(
        workspace.path().join("skills/always/SKILL.md"),
        "---\nname: always\nalways: true\ndescription: Always on\n---\nAlways body",
    )?;
    std::fs::write(
        workspace.path().join("skills/indexed.md"),
        "---\nname: indexed\ndescription: Listed skill\nrequirements: use when asked\n---\nIndexed body",
    )?;
    std::fs::write(
        workspace.path().join("skills/disabled.md"),
        "---\nname: disabled\ndisabled: true\n---\nDisabled body",
    )?;
    let jpg_path = workspace.path().join("extension-only.jpg");
    std::fs::write(&jpg_path, b"not real magic but extension image fallback")?;

    let context = ContextBuilder::new(workspace.path())
        .with_disabled_skills(["manually-disabled".to_owned()]);
    let active_always = context.active_always_skill_names();
    if !active_always.contains(&"always".to_owned())
        || !active_always.contains(&"memory".to_owned())
        || !active_always.contains(&"my".to_owned())
        || active_always.contains(&"indexed".to_owned())
    {
        return Err(format!("active always skill inventory drifted: {:?}", active_always).into());
    }
    assert_eq!(
        context.skill_name_for_source_path("skills/always/SKILL.md"),
        Some("always".to_owned())
    );
    assert_eq!(
        context.skill_name_for_source_path(workspace.path().join("skills/indexed.md")),
        Some("indexed".to_owned())
    );
    assert_eq!(
        context.skill_name_for_source_path("skills/disabled.md"),
        None
    );
    assert_eq!(
        context.skill_name_for_source_path("builtin_skills/cron/SKILL.md"),
        Some("cron".to_owned())
    );
    assert_eq!(
        ContextBuilder::new(workspace.path())
            .with_disabled_skills(["cron".to_owned()])
            .skill_name_for_source_path("builtin_skills/cron/SKILL.md"),
        None
    );
    assert_eq!(context.skill_name_for_source_path("missing/SKILL.md"), None);
    let system = context.build_system_prompt(Some("cli"));
    if !system.contains("# Memory")
        || !system.contains("## Long-term Memory")
        || !system.contains("remember this")
        || !system.contains("# Recent History")
        || !system.contains("keep")
        || system.contains("skip")
        || system.contains("ignore")
        || !system.contains("# Active Skills")
        || !system.contains("Always body")
        || !system.contains("# Available Skills")
        || !system.contains("**indexed** — Listed skill")
        || system.contains("Disabled body")
    {
        return Err(format!("system prompt context injection drifted: {system}").into());
    }

    let media = vec![jpg_path.to_string_lossy().to_string()];
    let messages = context.build_messages(ContextBuildRequest {
        current_message: "look",
        media: &media,
        ..ContextBuildRequest::new("look")
    });
    if !messages[1]["content"][1]["image_url"]["url"]
        .as_str()
        .unwrap_or_default()
        .starts_with("data:image/jpeg;base64,")
    {
        return Err(format!("extension MIME fallback missing: {messages:?}").into());
    }

    let outside = tempfile::NamedTempFile::new()?;
    std::fs::write(outside.path(), b"\xff\xd8\xffoutside")?;
    let escaped_media = vec![outside.path().to_string_lossy().to_string()];
    let escaped = context.build_messages(ContextBuildRequest {
        current_message: "outside",
        media: &escaped_media,
        ..ContextBuildRequest::new("outside")
    });
    if escaped[1]["content"].is_array() {
        return Err(format!("escaped media should not create image blocks: {escaped:?}").into());
    }

    let mut helper_messages = Vec::new();
    shacs_core::runtime::add_assistant_message(&mut helper_messages, None, None, None, None);
    shacs_core::runtime::add_tool_result(&mut helper_messages, "call", "tool", json!("ok"));
    shacs_core::runtime::add_assistant_message(
        &mut helper_messages,
        None,
        None,
        None,
        Some(vec![json!({"type": "thinking", "text": "plan"})]),
    );
    if helper_messages[0]["content"] != ""
        || helper_messages[1]["role"] != "tool"
        || helper_messages[2]["reasoning_content"] != ""
        || helper_messages[2]["thinking_blocks"][0]["text"] != "plan"
    {
        return Err(format!("context helper API drifted: {helper_messages:?}").into());
    }
    Ok(())
}

#[test]
fn runtime_context_skips_template_memory_and_reports_skill_parity_metadata(
) -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    std::fs::create_dir_all(workspace.path().join("memory"))?;
    std::fs::write(
        workspace.path().join("memory/MEMORY.md"),
        shacs_templates::render_workspace_template(shacs_templates::WorkspaceTemplate::Memory),
    )?;
    std::fs::create_dir_all(workspace.path().join("skills/nested-always"))?;
    std::fs::write(
        workspace.path().join("skills/nested-always/SKILL.md"),
        "---\nname: nested-always\nmetadata:\n  nanobot:\n    always: true\ndescription: Nested always\n---\nNested always body",
    )?;
    std::fs::create_dir_all(workspace.path().join("skills/missing-req"))?;
    std::fs::write(
        workspace.path().join("skills/missing-req/SKILL.md"),
        "---\nname: missing-req\ndescription: Missing requirement\nrequires:\n  bins:\n    - shacs-definitely-missing-bin\n  env: [SHACS_DEFINITELY_MISSING_ENV]\n---\nMissing req body",
    )?;
    std::fs::create_dir_all(workspace.path().join("skills/nested-req"))?;
    std::fs::write(
        workspace.path().join("skills/nested-req/SKILL.md"),
        "---\nname: nested-req\ndescription: Nested requirement\nmetadata:\n  nanobot:\n    requires:\n      bins:\n        - shacs-definitely-missing-nested-bin\n      env:\n        - SHACS_DEFINITELY_MISSING_NESTED_ENV\n  openclaw:\n    requires:\n      bins: [shacs-definitely-missing-openclaw-bin]\n      env: [SHACS_DEFINITELY_MISSING_OPENCLAW_ENV]\n---\nNested req body",
    )?;
    std::fs::create_dir_all(workspace.path().join("skills/inline-req"))?;
    std::fs::write(
        workspace.path().join("skills/inline-req/SKILL.md"),
        "---\nname: inline-req\ndescription: Inline requirement\nmetadata: '{\"nanobot\":{\"requires\":{\"bins\":[\"shacs-definitely-missing-inline-bin\"],\"env\":[\"SHACS_DEFINITELY_MISSING_INLINE_ENV\"]}},\"openclaw\":{\"requires\":{\"bins\":[\"shacs-definitely-missing-inline-openclaw-bin\"],\"env\":[\"SHACS_DEFINITELY_MISSING_INLINE_OPENCLAW_ENV\"]}}}'\n---\nInline req body",
    )?;
    std::fs::create_dir_all(workspace.path().join("skills/dir-canonical"))?;
    std::fs::write(
        workspace.path().join("skills/dir-canonical/SKILL.md"),
        "---\nname: frontmatter-alias\ndescription: Directory canonical\n---\nDirectory canonical body",
    )?;
    std::fs::create_dir_all(workspace.path().join("skills/dup"))?;
    std::fs::write(
        workspace.path().join("skills/dup/SKILL.md"),
        "---\nname: dup\ndescription: Workspace duplicate\n---\nWorkspace duplicate body",
    )?;
    std::fs::create_dir_all(workspace.path().join(".nanobot/skills/dup"))?;
    std::fs::write(
        workspace.path().join(".nanobot/skills/dup/SKILL.md"),
        "---\nname: dup\ndescription: Lower duplicate\n---\nLower duplicate body",
    )?;
    std::fs::create_dir_all(workspace.path().join("builtin_skills/dup"))?;
    std::fs::write(
        workspace.path().join("builtin_skills/dup/SKILL.md"),
        "---\nname: dup\ndescription: Builtin duplicate\n---\nBuiltin duplicate body",
    )?;

    let dotted_workspace = workspace.path().join(".");
    let context = ContextBuilder::new(&dotted_workspace);
    let system = context.build_system_prompt(Some("cli"));
    let canonical = workspace.path().canonicalize()?;
    if !system.contains("# Active Skills")
        || !system.contains("Nested always body")
        || !system.contains("**missing-req** — Missing requirement")
        || !system.contains("unavailable: missing bins: shacs-definitely-missing-bin")
        || !system.contains("missing env: SHACS_DEFINITELY_MISSING_ENV")
        || !system.contains("**nested-req** — Nested requirement")
        || !system.contains("unavailable: missing bins: shacs-definitely-missing-nested-bin, shacs-definitely-missing-openclaw-bin")
        || !system.contains("missing env: SHACS_DEFINITELY_MISSING_NESTED_ENV, SHACS_DEFINITELY_MISSING_OPENCLAW_ENV")
        || !system.contains("**inline-req** — Inline requirement")
        || !system.contains("unavailable: missing bins: shacs-definitely-missing-inline-bin, shacs-definitely-missing-inline-openclaw-bin")
        || !system.contains("missing env: SHACS_DEFINITELY_MISSING_INLINE_ENV, SHACS_DEFINITELY_MISSING_INLINE_OPENCLAW_ENV")
        || !system.contains("**dir-canonical** — Directory canonical")
        || system.contains("**frontmatter-alias** — Directory canonical")
        || !system.contains("`")
        || !system.contains("**dup** — Workspace duplicate")
        || system.contains("Lower duplicate")
        || !system.contains(&canonical.display().to_string())
        || !system.contains(std::env::consts::OS)
        || !system.contains(std::env::consts::ARCH)
    {
        return Err(format!("context parity metadata drifted: {system}").into());
    }
    let dup = context
        .load_skill("dup")
        .ok_or("missing explicit dup skill")?;
    let canonical_skill = context
        .load_skill("dir-canonical")
        .ok_or("directory skill name should be canonical")?;
    let loaded_context = context.load_skills_for_context(&["nested-always", "missing-req"]);
    let summary = context.build_skills_summary(&BTreeSet::from(["missing-req".to_owned()]));
    let subagent_prompt = context.build_subagent_prompt();
    if !dup.contains("Workspace duplicate body")
        || dup.contains("Lower duplicate body")
        || dup.contains("Builtin duplicate body")
        || !canonical_skill.contains("Directory canonical body")
        || context.load_skill("frontmatter-alias").is_some()
        || !loaded_context.contains("### Skill: nested-always")
        || loaded_context.contains("metadata:")
        || !loaded_context.contains("Missing req body")
        || summary.contains("**missing-req**")
        || !summary.contains("**nested-req** — Nested requirement")
        || !summary.contains("unavailable: missing bins: shacs-definitely-missing-nested-bin")
        || !subagent_prompt.contains("# Subagent")
        || !subagent_prompt.contains(&canonical.display().to_string())
        || !subagent_prompt.contains("## Skills")
        || !subagent_prompt.contains("**nested-req**")
    {
        return Err(format!(
            "skill loader/subagent prompt parity drifted: dup={dup} loaded={loaded_context} summary={summary} prompt={subagent_prompt}"
        )
        .into());
    }
    std::fs::write(
        workspace.path().join("memory/MEMORY.md"),
        "My template memory notes are real user memory.",
    )?;
    let system_with_memory =
        ContextBuilder::new(&dotted_workspace).build_system_prompt(Some("cli"));
    if !system_with_memory.contains("# Memory")
        || !system_with_memory.contains("My template memory notes are real user memory.")
    {
        return Err(format!(
            "non-template memory placeholder detection drifted: {system_with_memory}"
        )
        .into());
    }
    Ok(())
}

#[test]
fn runtime_context_loads_extra_skill_roots_and_virtual_builtins() -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    let data_dir = tempfile::tempdir()?;
    std::fs::create_dir_all(data_dir.path().join("skills/user-skill"))?;
    std::fs::write(
        data_dir.path().join("skills/user-skill/SKILL.md"),
        "---\ndescription: User global skill\n---\nUser global body",
    )?;

    std::fs::create_dir_all(workspace.path().join("builtin_skills/hermes-agent"))?;
    std::fs::write(
        workspace
            .path()
            .join("builtin_skills/hermes-agent/SKILL.md"),
        "---\ndescription: Stale deferred builtin\n---\nStale Hermes body",
    )?;

    let context = ContextBuilder::new(workspace.path())
        .with_skill_roots([data_dir.path().join("skills")])
        .with_disabled_skills(["cron".to_owned()]);
    let system = context.build_system_prompt(Some("cli"));

    if !system.contains("**user-skill** — User global skill")
        || !system.contains("**skill-creator**")
        || !system.contains("**test-driven-development**")
        || system.contains("**hermes-agent**")
        || context.load_skill("hermes-agent").is_some()
        || system.contains("**cron**")
        || !context
            .load_skill("user-skill")
            .is_some_and(|skill| skill.contains("User global body"))
        || !context
            .load_skill("test-driven-development")
            .is_some_and(|skill| skill.contains("shacs-bot adaptation"))
    {
        return Err(format!("extra skill roots or virtual builtins drifted: {system}").into());
    }
    Ok(())
}

#[test]
fn runtime_memory_store_appends_sanitizes_cursors_and_feeds_context() -> Result<(), Box<dyn Error>>
{
    let workspace = tempfile::tempdir()?;
    let store = MemoryStore::new(workspace.path())?;
    store.write_memory("remember this")?;
    store.write_soul("soul facts")?;
    store.write_user("user facts")?;

    let first = store.append_history("<think>secret</think> visible", None)?;
    let second = store.append_history("abcdef", Some(3))?;
    store.set_last_dream_cursor(first)?;

    let entries = store.read_entries();
    if first != 1
        || second != 2
        || entries.len() != 2
        || entries[0].content != "visible"
        || !entries[1].content.contains("abc")
        || !entries[1].content.contains("truncated")
        || store.get_last_dream_cursor() != 1
        || store.read_memory() != "remember this"
        || store.read_soul() != "soul facts"
        || store.read_user() != "user facts"
    {
        return Err(format!("memory store append/read drifted: {entries:?}").into());
    }

    let system = ContextBuilder::new(workspace.path()).build_system_prompt(Some("cli"));
    if !system.contains("remember this")
        || system.contains("visible")
        || !system.contains("abc")
        || system.contains("secret")
    {
        return Err(format!("memory store context bridge drifted: {system}").into());
    }
    Ok(())
}

#[test]
fn runtime_memory_store_migrates_legacy_history_without_replaying_dream(
) -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    std::fs::create_dir_all(workspace.path().join("memory"))?;
    std::fs::write(
        workspace.path().join("memory/HISTORY.md"),
        "[2026-05-01 12:00] first\n\n[2026-05-01 12:01] second",
    )?;

    let store = MemoryStore::new(workspace.path())?;
    let entries = store.read_entries();
    if entries.len() != 2
        || entries[0].cursor != 1
        || entries[0].content != "first"
        || entries[1].content != "second"
        || store.get_last_dream_cursor() != 2
        || workspace.path().join("memory/HISTORY.md").exists()
        || !workspace.path().join("memory/HISTORY.md.bak").exists()
    {
        return Err(format!("legacy history migration drifted: {entries:?}").into());
    }
    Ok(())
}

#[test]
fn runtime_memory_consolidator_updates_memory_and_dream_cursor() -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    let store = MemoryStore::new(workspace.path())?;
    store.write_memory("old memory")?;
    store.append_history("first fact", None)?;
    store.append_history("second fact", None)?;

    let provider = MockProvider::new(vec![LlmResponse {
        content: Some("updated memory".to_owned()),
        ..LlmResponse::default()
    }]);
    let consolidator = ProviderMemoryConsolidator::new(&provider, "memory-model");
    let outcome = store.consolidate_pending(&consolidator)?;

    let requests = provider
        .requests
        .lock()
        .map_err(|error| error.to_string())?;
    let prompt = requests
        .first()
        .and_then(|request| request.messages.get(1))
        .and_then(|message| message.get("content"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    if outcome.processed_entries != 2
        || outcome.processed_cursor != 2
        || !outcome.memory_updated
        || store.read_memory() != "updated memory"
        || store.get_last_dream_cursor() != 2
        || requests.first().map(|request| request.model.as_str()) != Some("memory-model")
        || !requests
            .first()
            .is_some_and(|request| request.tools.is_empty())
        || !prompt.contains("old memory")
        || !prompt.contains("first fact")
        || !prompt.contains("second fact")
    {
        return Err(
            format!("memory consolidation drifted: outcome={outcome:?} prompt={prompt:?}").into(),
        );
    }
    Ok(())
}

#[test]
fn runtime_memory_consolidator_failure_keeps_memory_and_cursor() -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    let store = MemoryStore::new(workspace.path())?;
    store.write_memory("old memory")?;
    store.append_history("new fact", None)?;

    let provider = MockProvider::new(Vec::new());
    let consolidator = ProviderMemoryConsolidator::new(&provider, "memory-model");
    if store.consolidate_pending(&consolidator).is_ok()
        || store.read_memory() != "old memory"
        || store.get_last_dream_cursor() != 0
    {
        return Err("failed consolidation should not mutate memory or cursor".into());
    }
    Ok(())
}

#[test]
fn runtime_archive_consolidator_summarizes_or_raw_archives_on_provider_failure(
) -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    let store = MemoryStore::new(workspace.path())?;
    let messages = vec![json!({"role": "user", "content": "important detail", "timestamp": "now"})];
    let provider = MockProvider::new(vec![LlmResponse {
        content: Some("summary detail".to_owned()),
        ..LlmResponse::default()
    }]);
    let archive = ProviderArchiveConsolidator::new(&provider, "archive-model");
    let outcome = archive.archive(&store, &messages)?;
    let entries = store.read_entries();
    if outcome.summary.as_deref() != Some("summary detail")
        || outcome.raw_fallback
        || entries.len() != 1
        || entries[0].content != "summary detail"
    {
        return Err(
            format!("summary archive drifted: outcome={outcome:?} entries={entries:?}").into(),
        );
    }

    let fallback_workspace = tempfile::tempdir()?;
    let fallback_store = MemoryStore::new(fallback_workspace.path())?;
    let failing_provider = MockProvider::new(Vec::new());
    let failing_archive = ProviderArchiveConsolidator::new(&failing_provider, "archive-model");
    let fallback = failing_archive.archive(&fallback_store, &messages)?;
    let fallback_entries = fallback_store.read_entries();
    if !fallback.raw_fallback
        || fallback.summary.is_some()
        || fallback_entries.len() != 1
        || !fallback_entries[0].content.contains("[RAW] 1 messages")
        || !fallback_entries[0].content.contains("important detail")
    {
        return Err(format!(
            "raw archive fallback drifted: fallback={fallback:?} entries={fallback_entries:?}"
        )
        .into());
    }
    Ok(())
}

#[test]
fn runtime_archive_consolidator_respects_nothing_and_formats_tool_activity(
) -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    let store = MemoryStore::new(workspace.path())?;
    let messages = vec![
        json!({
            "role": "assistant",
            "content": "",
            "timestamp": "now",
            "tool_calls": [{"id": "call-1", "type": "function", "function": {"name": "read_file", "arguments": "{}"}}]
        }),
        json!({
            "role": "tool",
            "name": "read_file",
            "tool_call_id": "call-1",
            "content": "file contents",
            "timestamp": "now"
        }),
    ];
    let provider = MockProvider::new(vec![LlmResponse {
        content: Some("(nothing)".to_owned()),
        ..LlmResponse::default()
    }]);
    let archive = ProviderArchiveConsolidator::new(&provider, "archive-model");

    let outcome = archive.archive(&store, &messages)?;
    let requests = provider
        .requests
        .lock()
        .map_err(|error| error.to_string())?;
    let archive_prompt = requests
        .first()
        .and_then(|request| request.messages.get(1))
        .and_then(|message| message.get("content"))
        .and_then(Value::as_str)
        .unwrap_or_default();

    if outcome.summary.is_some()
        || outcome.raw_fallback
        || !store.read_entries().is_empty()
        || !archive_prompt.contains("tools_used: read_file")
        || !archive_prompt.contains("tool_result: read_file (call-1)")
    {
        return Err(format!(
            "archive nothing/tool formatting drifted: outcome={outcome:?} prompt={archive_prompt:?} entries={:?}",
            store.read_entries()
        )
        .into());
    }
    Ok(())
}

#[test]
fn runtime_memory_store_keeps_raw_legacy_blocks_together() -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    std::fs::create_dir_all(workspace.path().join("memory"))?;
    std::fs::write(
        workspace.path().join("memory/HISTORY.md"),
        "[RAW] 2 messages\n[2026-05-01 12:00] USER: inside raw\nassistant: still raw\n\n[2026-05-01 12:01] next",
    )?;

    let store = MemoryStore::new(workspace.path())?;
    let entries = store.read_entries();
    if entries.len() != 2
        || !entries[0].content.contains("inside raw")
        || entries[0].content.contains("[2026-05-01 12:01] next")
        || entries[1].content != "next"
    {
        return Err(format!("raw legacy block should remain unsplit: {entries:?}").into());
    }
    Ok(())
}

#[test]
fn runtime_memory_store_keeps_timestamped_raw_legacy_blocks_together() -> Result<(), Box<dyn Error>>
{
    let workspace = tempfile::tempdir()?;
    std::fs::create_dir_all(workspace.path().join("memory"))?;
    std::fs::write(
        workspace.path().join("memory/HISTORY.md"),
        "[2026-05-01 12:00] [RAW] 2 messages\n[2026-05-01 12:00] USER: inside raw\nassistant: still raw\n\n[2026-05-01 12:01] next",
    )?;

    let store = MemoryStore::new(workspace.path())?;
    let entries = store.read_entries();
    if entries.len() != 2
        || !entries[0].content.contains("inside raw")
        || entries[0].content.contains("[2026-05-01 12:01] next")
        || entries[1].content != "next"
    {
        return Err(
            format!("timestamped raw legacy block should remain unsplit: {entries:?}").into(),
        );
    }
    Ok(())
}

#[test]
fn runtime_token_consolidation_archives_on_user_boundary_and_preserves_agent_configuration(
) -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    let store = MemoryStore::new(workspace.path())?;
    let mut manager = SessionManager::new(workspace.path())?;
    let mut session = Session::new("cli:token");
    session.add_message("user", "old question ".repeat(80), Map::new());
    session.add_message("assistant", "old answer ".repeat(80), Map::new());
    session.add_message("user", "new question", Map::new());
    session.add_message("assistant", "new answer", Map::new());
    session
        .metadata
        .insert("agent_configuration".to_owned(), json!({"model": "kept"}));
    session
        .metadata
        .insert("runtime_checkpoint".to_owned(), json!({"phase": "kept"}));
    manager.save(&session)?;

    let boundary = pick_consolidation_boundary(&session, 10).ok_or("missing boundary")?;
    if boundary.0 != 2 || boundary.1 == 0 {
        return Err(format!("token consolidation boundary drifted: {boundary:?}").into());
    }

    let provider = MockProvider::new(vec![LlmResponse {
        content: Some("old turn summary".to_owned()),
        ..LlmResponse::default()
    }]);
    let archive = ProviderArchiveConsolidator::new(&provider, "archive-model");
    let config = TokenConsolidationConfig {
        context_window_tokens: 120,
        max_completion_tokens: 1,
        safety_buffer: 1,
        consolidation_ratio: 0.2,
        max_rounds: 5,
    };
    let outcome = archive.maybe_consolidate_session_by_tokens(
        &store,
        &mut manager,
        &mut session,
        &config,
        &[],
        None,
    )?;
    let loaded = manager.get_or_create("cli:token");
    if outcome.rounds != 1
        || outcome.archived_messages != 2
        || outcome.last_consolidated != 2
        || !outcome.summary_stored
        || loaded.last_consolidated != 2
        || loaded.metadata["agent_configuration"]["model"] != "kept"
        || loaded.metadata["runtime_checkpoint"]["phase"] != "kept"
        || loaded.metadata["_last_summary"]["text"] != "old turn summary"
        || store
            .read_entries()
            .first()
            .map(|entry| entry.content.as_str())
            != Some("old turn summary")
    {
        return Err(format!(
            "token consolidation drifted: outcome={outcome:?} loaded={loaded:?} entries={:?}",
            store.read_entries()
        )
        .into());
    }
    Ok(())
}

#[test]
fn runtime_dream_processor_runs_phase_prompts_advances_cursor_and_uses_git_boundary(
) -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    let store = MemoryStore::new(workspace.path())?;
    store.write_memory("stale fact\nfresh fact\n")?;
    store.write_soul("soul")?;
    store.write_user("user")?;
    store.append_history("new durable fact", None)?;
    std::fs::create_dir_all(workspace.path().join("skills/existing"))?;
    std::fs::write(
        workspace.path().join("skills/existing/SKILL.md"),
        "---\ndescription: Existing skill\n---\nBody",
    )?;
    let provider = MockProvider::new(vec![
        LlmResponse {
            content: Some("analyze stale fact".to_owned()),
            ..LlmResponse::default()
        },
        LlmResponse {
            content: Some("dream done".to_owned()),
            ..LlmResponse::default()
        },
    ]);
    let git = StaticGitBoundary {
        ages: vec![
            MemoryLineAge { age_days: 30 },
            MemoryLineAge { age_days: 1 },
        ],
    };
    let outcome = DreamProcessor::new(store.clone(), &provider, "dream-model")
        .with_git_boundary(&git)
        .run()?;
    let requests = provider
        .requests
        .lock()
        .map_err(|error| error.to_string())?;
    let phase1_prompt = requests
        .first()
        .and_then(|request| request.messages.get(1))
        .and_then(|message| message.get("content"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    let phase2_prompt = requests
        .get(1)
        .and_then(|request| request.messages.get(1))
        .and_then(|message| message.get("content"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    if !outcome.worked
        || !outcome.phase1_completed
        || !outcome.phase2_completed
        || !outcome.cursor_advanced
        || outcome.processed_cursor != 1
        || store.get_last_dream_cursor() != 1
        || requests.len() != 2
        || !requests[1]
            .tools
            .iter()
            .any(|tool| tool["function"]["name"] == "read_file")
        || !requests[1]
            .tools
            .iter()
            .any(|tool| tool["function"]["name"] == "edit_file")
        || !requests[1]
            .tools
            .iter()
            .any(|tool| tool["function"]["name"] == "write_file")
        || !phase1_prompt.contains("stale fact  ← 30d")
        || !phase2_prompt.contains("Existing Skills")
        || !phase2_prompt.contains("existing — Existing skill")
    {
        return Err(format!(
            "dream processor drifted: outcome={outcome:?} phase1={phase1_prompt:?} phase2={phase2_prompt:?}"
        )
        .into());
    }
    Ok(())
}

#[cfg(unix)]
#[test]
fn runtime_context_rejects_media_symlink_to_outside_workspace() -> Result<(), Box<dyn Error>> {
    use std::os::unix::fs::symlink;

    let workspace = tempfile::tempdir()?;
    let outside = tempfile::NamedTempFile::new()?;
    std::fs::write(outside.path(), b"\xff\xd8\xffsecret")?;
    let link = workspace.path().join("linked.jpg");
    symlink(outside.path(), &link)?;
    let media = vec![link.to_string_lossy().to_string()];
    let messages = ContextBuilder::new(workspace.path()).build_messages(ContextBuildRequest {
        current_message: "look",
        media: &media,
        ..ContextBuildRequest::new("look")
    });
    if messages[1]["content"].is_array() {
        return Err(format!("symlinked media should be skipped: {messages:?}").into());
    }
    Ok(())
}

#[test]
fn runtime_runner_executes_tool_loop_and_accumulates_usage() -> Result<(), Box<dyn Error>> {
    let mut registry = ToolRegistry::new();
    registry.register(NamedRepeatTool("web_search"));
    let client = MockProvider::new(vec![
        LlmResponse {
            tool_calls: vec![ToolCallRequest::new(
                "call_1",
                "web_search",
                Map::from_iter([("text".to_owned(), json!("ha"))]),
            )],
            finish_reason: "tool_calls".to_owned(),
            reasoning_content: Some("because".to_owned()),
            thinking_blocks: Some(vec![json!({"type": "thinking", "text": "plan"})]),
            usage: BTreeMap::from([("prompt_tokens".to_owned(), 2)]),
            ..LlmResponse::default()
        },
        LlmResponse {
            content: Some("done".to_owned()),
            usage: BTreeMap::from([("completion_tokens".to_owned(), 3)]),
            ..LlmResponse::default()
        },
    ]);
    let runner = AgentRunner::new();
    let checkpoints = Arc::new(Mutex::new(Vec::<Value>::new()));
    let checkpoint_capture = checkpoints.clone();
    let mut spec = AgentRunSpec::new(
        vec![json!({"role": "user", "content": "go"})],
        &registry,
        &client,
        "test-model",
    );
    spec.max_iterations = 4;
    spec.tool_search = ToolSearchConfig {
        enabled: ToolSearchMode::On,
        threshold_pct: 0,
        search_default_limit: 1,
        max_search_limit: 1,
    };
    spec.context_window_tokens = Some(8_192);
    spec.tool_context = safe_mcp_tool_context();
    assert_eq!(
        spec.tool_search_runtime_input().context_window_tokens,
        Some(8_192)
    );
    spec.checkpoint_callback = Some(Arc::new(move |checkpoint| {
        if let Ok(mut checkpoints) = checkpoints.lock() {
            checkpoints.push(checkpoint.clone());
        }
    }));
    let result = runner.run(spec)?;

    if result.stop_reason != "completed"
        || result.final_content.as_deref() != Some("done")
        || result.tools_used != ["web_search"]
        || result.usage.get("prompt_tokens") != Some(&2)
        || result.usage.get("completion_tokens") != Some(&3)
        || result.messages[1]["tool_calls"][0]["function"]["arguments"] != "{\"text\":\"ha\"}"
        || result.messages[1]["reasoning_content"] != "because"
        || result.messages[1]["thinking_blocks"][0]["text"] != "plan"
        || result.messages[2]["role"] != "tool"
        || result.messages[2]["content"] != "haha"
    {
        return Err(format!("runner tool loop drifted: {result:?}").into());
    }
    let requests = client.requests.lock().map_err(|error| error.to_string())?;
    if requests.len() != 2
        || requests[1].messages.len() != 3
        || requests[0].tools != registry.definitions()
    {
        return Err(format!("runner request sequence drifted: {requests:?}").into());
    }
    let checkpoints = checkpoint_capture
        .lock()
        .map_err(|error| error.to_string())?;
    if checkpoints.len() != 3
        || checkpoints[0]["phase"] != "awaiting_tools"
        || checkpoints[0]["pending_tool_calls"][0]["id"] != "call_1"
        || checkpoints[1]["phase"] != "tools_completed"
        || checkpoints[1]["completed_tool_results"][0]["tool_call_id"] != "call_1"
        || checkpoints[2]["phase"] != "final_response"
        || checkpoints[2]["assistant_message"]["content"] != "done"
    {
        return Err(format!("runner checkpoint callbacks drifted: {checkpoints:?}").into());
    }
    Ok(())
}

#[test]
fn runtime_runner_tool_search_off_uses_registry_definitions() -> Result<(), Box<dyn Error>> {
    let mut registry = ToolRegistry::new();
    registry.register(RepeatTool);
    registry.register(McpEchoTool);
    let client = MockProvider::new(vec![LlmResponse {
        content: Some("done".to_owned()),
        ..LlmResponse::default()
    }]);
    let mut spec = AgentRunSpec::new(
        vec![json!({"role": "user", "content": "go"})],
        &registry,
        &client,
        "test-model",
    );
    spec.tool_search = ToolSearchConfig {
        enabled: ToolSearchMode::Off,
        threshold_pct: 0,
        search_default_limit: 2,
        max_search_limit: 4,
    };

    let result = AgentRunner::new().run(spec)?;
    let requests = client.requests.lock().map_err(|error| error.to_string())?;
    if result.final_content.as_deref() != Some("done")
        || requests.len() != 1
        || requests[0].tools != registry.definitions()
    {
        return Err(format!(
            "Tool Search off should preserve provider tools: result={result:?} requests={requests:?}"
        )
        .into());
    }
    Ok(())
}

#[test]
fn runtime_runner_tool_search_activation_hides_mcp_and_adds_bridge_tools(
) -> Result<(), Box<dyn Error>> {
    let mut registry = ToolRegistry::new();
    registry.register(RepeatTool);
    registry.register(McpEchoTool);
    let client = MockProvider::new(vec![LlmResponse {
        content: Some("done".to_owned()),
        ..LlmResponse::default()
    }]);
    let mut spec = AgentRunSpec::new(
        vec![json!({"role": "user", "content": "go"})],
        &registry,
        &client,
        "test-model",
    );
    spec.tool_search = activated_tool_search_config();
    spec.tool_context = safe_mcp_tool_context();

    let result = AgentRunner::new().run(spec)?;
    let requests = client.requests.lock().map_err(|error| error.to_string())?;
    let names = provider_tool_names(requests.first().ok_or("missing provider request")?)?;
    if result.final_content.as_deref() != Some("done")
        || names != ["repeat", "tool_search", "tool_describe", "tool_call"]
        || names.iter().any(|name| name.starts_with("mcp_"))
    {
        return Err(format!(
            "activated Tool Search provider surface drifted: names={names:?} requests={requests:?}"
        )
        .into());
    }
    Ok(())
}

#[test]
fn runtime_runner_tool_search_activation_diagnostics_are_observable() -> Result<(), Box<dyn Error>>
{
    let mut registry = ToolRegistry::new();
    registry.register(RepeatTool);
    registry.register(McpEchoTool);
    let client = MockProvider::new(vec![LlmResponse {
        content: Some("done".to_owned()),
        ..LlmResponse::default()
    }]);
    let mut spec = AgentRunSpec::new(
        vec![json!({"role": "user", "content": "go"})],
        &registry,
        &client,
        "test-model",
    );
    spec.tool_search = activated_tool_search_config();
    spec.tool_context = safe_mcp_tool_context();

    let result = AgentRunner::new().run(spec)?;
    let event = result
        .tool_events
        .iter()
        .find(|event| event.name == "tool_search_activation")
        .ok_or("missing Tool Search activation event")?;
    let activation = event
        .result
        .as_ref()
        .and_then(|result| result.get("activation"))
        .ok_or("missing activation summary")?;

    if activation["mode"] != "on"
        || activation["activated"] != true
        || activation["reason"] != "forced_on"
        || activation["visible_count"] != 4
        || activation["deferred_count"] != 1
        || activation["deferred_source_counts"]["mcp_tool"] != 1
        || !event.detail.contains("reason=forced_on")
        || !activation["scope_digest"]
            .as_str()
            .unwrap_or_default()
            .starts_with("sha256:")
    {
        return Err(format!("activation diagnostics drifted: {activation}").into());
    }
    Ok(())
}

#[test]
fn runtime_runner_bridge_events_use_redacted_bounded_evidence() -> Result<(), Box<dyn Error>> {
    let mut registry = ToolRegistry::new();
    for name in [
        "mcp_alpha_lookup",
        "mcp_beta_lookup",
        "mcp_gamma_lookup",
        "mcp_delta_lookup",
        "mcp_epsilon_lookup",
        "mcp_zeta_lookup",
    ] {
        registry.register(NamedMcpTool(name));
    }
    registry.register(McpEchoTool);
    let client = MockProvider::new(vec![
        LlmResponse {
            tool_calls: vec![ToolCallRequest::new(
                "bridge-search",
                "tool_search",
                Map::from_iter([
                    ("query".to_owned(), json!("lookup token sk-secret")),
                    ("limit".to_owned(), json!(10)),
                ]),
            )],
            finish_reason: "tool_calls".to_owned(),
            ..LlmResponse::default()
        },
        LlmResponse {
            tool_calls: vec![ToolCallRequest::new(
                "bridge-describe",
                "tool_describe",
                Map::from_iter([("name".to_owned(), json!("mcp_echo_lookup"))]),
            )],
            finish_reason: "tool_calls".to_owned(),
            ..LlmResponse::default()
        },
        LlmResponse {
            tool_calls: vec![ToolCallRequest::new(
                "bridge-call",
                "tool_call",
                Map::from_iter([
                    ("name".to_owned(), json!("mcp_echo_lookup")),
                    ("arguments".to_owned(), json!({"query": "secret token"})),
                ]),
            )],
            finish_reason: "tool_calls".to_owned(),
            ..LlmResponse::default()
        },
        LlmResponse {
            content: Some("done".to_owned()),
            ..LlmResponse::default()
        },
    ]);
    let mut spec = AgentRunSpec::new(
        vec![json!({"role": "user", "content": "go"})],
        &registry,
        &client,
        "test-model",
    );
    spec.tool_search = activated_tool_search_config();
    spec.max_iterations = 6;
    spec.tool_context = safe_mcp_tool_context();

    let result = AgentRunner::new().run(spec)?;
    let search_event = result
        .tool_events
        .iter()
        .find(|event| event.call_id.as_deref() == Some("bridge-search"))
        .ok_or("missing search event")?;
    let search_evidence = &search_event
        .result
        .as_ref()
        .ok_or("missing search result")?["query_evidence"];
    if search_evidence["redacted_query"] != "[redacted]"
        || search_evidence["matched_names"].as_array().map(Vec::len) != Some(4)
        || !search_evidence["scope_digest"]
            .as_str()
            .unwrap_or_default()
            .starts_with("sha256:")
    {
        return Err(format!("search evidence drifted: {search_evidence}").into());
    }

    let describe_event = result
        .tool_events
        .iter()
        .find(|event| event.call_id.as_deref() == Some("bridge-describe"))
        .ok_or("missing describe event")?;
    let describe_json = serde_json::to_string(describe_event)?;
    if describe_json.contains("schema")
        || describe_json.contains("properties")
        || describe_event
            .result
            .as_ref()
            .ok_or("missing describe result")?["describe_evidence"]["requested_name"]
            != "mcp_echo_lookup"
        || describe_event
            .result
            .as_ref()
            .ok_or("missing describe result")?["describe_evidence"]["found"]
            != true
    {
        return Err(format!("describe evidence exposed schema: {describe_json}").into());
    }

    let call_event = result
        .tool_events
        .iter()
        .find(|event| event.call_id.as_deref() == Some("bridge-call"))
        .ok_or("missing call event")?;
    let call_json = serde_json::to_string(call_event)?;
    let mapping = &call_event.result.as_ref().ok_or("missing call result")?["mapping_evidence"];
    if mapping["bridge_call_id"] != "bridge-call"
        || mapping["bridge_name"] != "tool_call"
        || mapping["underlying_name"] != "mcp_echo_lookup"
        || !mapping["scope_digest"]
            .as_str()
            .unwrap_or_default()
            .starts_with("sha256:")
        || call_json.contains("secret token")
        || call_json.contains("arguments")
    {
        return Err(format!("call evidence drifted or leaked arguments: {call_json}").into());
    }
    Ok(())
}

#[test]
fn runtime_runner_bridge_search_describe_call_roundtrip_completes_turn(
) -> Result<(), Box<dyn Error>> {
    let mut registry = ToolRegistry::new();
    registry.register(RepeatTool);
    registry.register(McpEchoTool);
    let client = MockProvider::new(vec![
        LlmResponse {
            tool_calls: vec![ToolCallRequest::new(
                "bridge-search",
                "tool_search",
                Map::from_iter([
                    ("query".to_owned(), json!("echo")),
                    ("limit".to_owned(), json!(1)),
                ]),
            )],
            finish_reason: "tool_calls".to_owned(),
            ..LlmResponse::default()
        },
        LlmResponse {
            tool_calls: vec![ToolCallRequest::new(
                "bridge-describe",
                "tool_describe",
                Map::from_iter([("name".to_owned(), json!("mcp_echo_lookup"))]),
            )],
            finish_reason: "tool_calls".to_owned(),
            ..LlmResponse::default()
        },
        LlmResponse {
            tool_calls: vec![ToolCallRequest::new(
                "bridge-call",
                "tool_call",
                Map::from_iter([
                    ("name".to_owned(), json!("mcp_echo_lookup")),
                    ("arguments".to_owned(), json!({"query": "hello"})),
                ]),
            )],
            finish_reason: "tool_calls".to_owned(),
            ..LlmResponse::default()
        },
        LlmResponse {
            content: Some("done".to_owned()),
            ..LlmResponse::default()
        },
    ]);
    let mut spec = AgentRunSpec::new(
        vec![json!({"role": "user", "content": "go"})],
        &registry,
        &client,
        "test-model",
    );
    spec.tool_search = activated_tool_search_config();
    spec.max_iterations = 6;
    spec.tool_context = safe_mcp_tool_context();

    let result = AgentRunner::new().run(spec)?;
    if result.stop_reason != "completed"
        || result.final_content.as_deref() != Some("done")
        || result.tools_used != ["tool_search", "tool_describe", "mcp_echo_lookup"]
        || result.messages[1]["tool_calls"][0]["function"]["name"] != "tool_search"
        || result.messages[2]["tool_call_id"] != "bridge-search"
        || !result.messages[2]["content"]
            .as_str()
            .unwrap_or_default()
            .contains("mcp_echo_lookup")
        || result.messages[3]["tool_calls"][0]["function"]["name"] != "tool_describe"
        || result.messages[4]["tool_call_id"] != "bridge-describe"
        || !result.messages[4]["content"]
            .as_str()
            .unwrap_or_default()
            .contains("mcp_echo_lookup")
        || result.messages[5]["tool_calls"][0]["function"]["name"] != "tool_call"
        || result.messages[6]["tool_call_id"] != "bridge-call"
        || result.messages[6]["name"] != "tool_call"
        || result.messages[6]["content"] != "mcp:hello"
    {
        return Err(format!("bridge roundtrip drifted: {result:?}").into());
    }
    Ok(())
}

#[test]
fn runtime_runner_direct_visible_tool_still_executes_with_tool_search_active(
) -> Result<(), Box<dyn Error>> {
    let mut registry = ToolRegistry::new();
    registry.register(NamedRepeatTool("web_search"));
    registry.register(McpEchoTool);
    let client = MockProvider::new(vec![
        LlmResponse {
            tool_calls: vec![ToolCallRequest::new(
                "direct-repeat",
                "web_search",
                Map::from_iter([("text".to_owned(), json!("ha"))]),
            )],
            finish_reason: "tool_calls".to_owned(),
            ..LlmResponse::default()
        },
        LlmResponse {
            content: Some("done".to_owned()),
            ..LlmResponse::default()
        },
    ]);
    let mut spec = AgentRunSpec::new(
        vec![json!({"role": "user", "content": "go"})],
        &registry,
        &client,
        "test-model",
    );
    spec.tool_search = activated_tool_search_config();
    spec.tool_context = safe_mcp_tool_context();

    let result = AgentRunner::new().run(spec)?;
    let requests = client.requests.lock().map_err(|error| error.to_string())?;
    let names = provider_tool_names(requests.first().ok_or("missing provider request")?)?;
    if result.final_content.as_deref() != Some("done")
        || result.tools_used != ["web_search"]
        || result.messages[2]["tool_call_id"] != "direct-repeat"
        || result.messages[2]["name"] != "web_search"
        || result.messages[2]["content"] != "haha"
        || names != ["web_search", "tool_search", "tool_describe", "tool_call"]
    {
        return Err(
            format!("direct visible tool path drifted: result={result:?} names={names:?}").into(),
        );
    }
    Ok(())
}

#[test]
fn runtime_runner_rebuilds_catalog_and_rejects_stale_bridge_call() -> Result<(), Box<dyn Error>> {
    let fresh_name = Arc::new(AtomicBool::new(false));
    let tool_calls = Arc::new(AtomicUsize::new(0));
    let mut registry = ToolRegistry::new();
    registry.register(SwitchingMcpTool {
        fresh_name: fresh_name.clone(),
        calls: tool_calls.clone(),
    });
    let fresh_name_after_first_request = fresh_name.clone();
    let client = MutatingProvider::new(
        vec![
            LlmResponse {
                tool_calls: vec![ToolCallRequest::new(
                    "search-stale-catalog",
                    "tool_search",
                    Map::from_iter([("query".to_owned(), json!("lookup"))]),
                )],
                finish_reason: "tool_calls".to_owned(),
                ..LlmResponse::default()
            },
            LlmResponse {
                tool_calls: vec![ToolCallRequest::new(
                    "stale-bridge-call",
                    "tool_call",
                    Map::from_iter([
                        ("name".to_owned(), json!("mcp_stale_lookup")),
                        ("arguments".to_owned(), json!({"query": "old"})),
                    ]),
                )],
                finish_reason: "tool_calls".to_owned(),
                ..LlmResponse::default()
            },
            LlmResponse {
                content: Some("done".to_owned()),
                ..LlmResponse::default()
            },
        ],
        Arc::new(move |request_count| {
            if request_count == 1 {
                fresh_name_after_first_request.store(true, Ordering::SeqCst);
            }
        }),
    );
    let mut spec = AgentRunSpec::new(
        vec![json!({"role": "user", "content": "go"})],
        &registry,
        &client,
        "test-model",
    );
    spec.tool_search = activated_tool_search_config();
    spec.max_iterations = 5;

    let result = AgentRunner::new().run(spec)?;
    let stale_result = result
        .messages
        .iter()
        .find(|message| message["tool_call_id"] == "stale-bridge-call")
        .ok_or("missing stale bridge tool result")?;
    if result.final_content.as_deref() != Some("done")
        || tool_calls.load(Ordering::SeqCst) != 0
        || result.tools_used != ["tool_search", "tool_call"]
        || !stale_result["content"]
            .as_str()
            .unwrap_or_default()
            .contains("not deferred in the current catalog")
    {
        return Err(format!(
            "stale bridge call should fail closed without executing: result={result:?} stale_result={stale_result:?} calls={}",
            tool_calls.load(Ordering::SeqCst)
        )
        .into());
    }
    Ok(())
}

#[test]
fn runtime_runner_bridge_schemas_remain_provider_agnostic_canonical_tools(
) -> Result<(), Box<dyn Error>> {
    let mut registry = ToolRegistry::new();
    registry.register(McpEchoTool);
    let client = MockProvider::new(vec![LlmResponse {
        content: Some("done".to_owned()),
        ..LlmResponse::default()
    }]);
    let mut spec = AgentRunSpec::new(
        vec![json!({"role": "user", "content": "go"})],
        &registry,
        &client,
        "test-model",
    );
    spec.tool_search = activated_tool_search_config();

    let result = AgentRunner::new().run(spec)?;
    let requests = client.requests.lock().map_err(|error| error.to_string())?;
    let request = requests.first().ok_or("missing provider request")?;
    let names = provider_tool_names(request)?;
    let forbidden_keys = ["defer", "provider", "provider_native", "tool_search_beta"];
    let canonical = request.tools.iter().all(|tool| {
        tool.get("type") == Some(&json!("function"))
            && tool.get("function").is_some_and(Value::is_object)
            && forbidden_keys.iter().all(|key| tool.get(*key).is_none())
    });
    if result.final_content.as_deref() != Some("done")
        || names != ["tool_search", "tool_describe", "tool_call"]
        || !canonical
    {
        return Err(format!(
            "bridge schemas should remain canonical provider-agnostic tools: names={names:?} tools={:?}",
            request.tools
        )
        .into());
    }
    Ok(())
}

#[test]
fn runtime_runner_honors_concurrent_tools_flag() -> Result<(), Box<dyn Error>> {
    let active = Arc::new(AtomicUsize::new(0));
    let max_active = Arc::new(AtomicUsize::new(0));
    let mut registry = ToolRegistry::new();
    registry.register(SafeDelayTool::new(
        "read_file",
        active.clone(),
        max_active.clone(),
    ));
    registry.register(SafeDelayTool::new("list_dir", active, max_active.clone()));
    let client = MockProvider::new(vec![
        LlmResponse {
            tool_calls: vec![
                ToolCallRequest::new("safe-a", "read_file", Map::new()),
                ToolCallRequest::new("safe-b", "list_dir", Map::new()),
            ],
            finish_reason: "tool_calls".to_owned(),
            ..LlmResponse::default()
        },
        LlmResponse {
            content: Some("done".to_owned()),
            ..LlmResponse::default()
        },
    ]);
    let mut spec = AgentRunSpec::new(
        vec![json!({"role": "user", "content": "go"})],
        &registry,
        &client,
        "test-model",
    );
    spec.concurrent_tools = true;
    spec.tool_context = safe_mcp_tool_context();

    let result = AgentRunner::new().run(spec)?;
    if result.stop_reason != "completed"
        || result.final_content.as_deref() != Some("done")
        || result.messages[2]["content"] != "read_file"
        || result.messages[3]["content"] != "list_dir"
        || max_active.load(Ordering::SeqCst) != 2
    {
        return Err(format!("runner did not batch safe tools: {result:?}").into());
    }
    Ok(())
}

#[test]
fn runtime_runner_drains_mid_turn_injections_between_iterations() -> Result<(), Box<dyn Error>> {
    let mut registry = ToolRegistry::new();
    registry.register(RepeatTool);
    let client = MockProvider::new(vec![
        LlmResponse {
            tool_calls: vec![ToolCallRequest::new(
                "repeat",
                "repeat",
                Map::from_iter([
                    ("text".to_owned(), json!("ha")),
                    ("times".to_owned(), json!(1)),
                ]),
            )],
            finish_reason: "tool_calls".to_owned(),
            ..LlmResponse::default()
        },
        LlmResponse {
            content: Some("answered follow-up".to_owned()),
            ..LlmResponse::default()
        },
    ]);
    let drain_count = Arc::new(AtomicUsize::new(0));
    let drain_count_clone = drain_count.clone();
    let hook_events = Arc::new(Mutex::new(Vec::<String>::new()));
    let mut spec = AgentRunSpec::new(
        vec![json!({"role": "user", "content": "start"})],
        &registry,
        &client,
        "test-model",
    );
    spec.mid_turn_injection_callback = Some(Arc::new(move || {
        let count = drain_count_clone.fetch_add(1, Ordering::SeqCst);
        if count == 1 {
            vec![json!({"role": "user", "content": "follow-up"})]
        } else {
            Vec::new()
        }
    }));
    spec.agent_hook = Some(Arc::new(RecordingHook::new(
        "inject",
        hook_events.clone(),
        false,
        "",
    )));

    let result = AgentRunner::new().run(spec)?;
    let requests = client.requests.lock().map_err(|error| error.to_string())?;
    let hook_events = hook_events.lock().map_err(|error| error.to_string())?;
    if !result.had_injections
        || result.final_content.as_deref() != Some("answered follow-up")
        || !requests
            .get(1)
            .into_iter()
            .flat_map(|request| &request.messages)
            .any(|message| message["role"] == "user" && message["content"] == "follow-up")
        || !hook_events
            .iter()
            .any(|event| event == "inject:before:1:follow-up")
    {
        return Err(format!(
            "runner did not drain mid-turn injection before hook/model: result={result:?} requests={requests:?} hook_events={hook_events:?}"
        )
        .into());
    }
    Ok(())
}

#[test]
fn runtime_runner_caps_mid_turn_injection_cycles() -> Result<(), Box<dyn Error>> {
    let registry = ToolRegistry::new();
    let client = MockProvider::new(vec![
        LlmResponse {
            content: Some("candidate-0".to_owned()),
            ..LlmResponse::default()
        },
        LlmResponse {
            content: Some("candidate-1".to_owned()),
            ..LlmResponse::default()
        },
        LlmResponse {
            content: Some("candidate-2".to_owned()),
            ..LlmResponse::default()
        },
    ]);
    let injection_count = Arc::new(AtomicUsize::new(0));
    let injection_count_clone = injection_count.clone();
    let mut spec = AgentRunSpec::new(
        vec![json!({"role": "user", "content": "start"})],
        &registry,
        &client,
        "test-model",
    );
    spec.mid_turn_injection_callback = Some(Arc::new(move || {
        let count = injection_count_clone.fetch_add(1, Ordering::SeqCst);
        vec![json!({"role": "user", "content": format!("follow-up-{count}")})]
    }));

    let result = AgentRunner::new().run(spec)?;
    let requests = client.requests.lock().map_err(|error| error.to_string())?;
    let injected_messages = result
        .messages
        .iter()
        .filter(|message| {
            message["role"] == "user"
                && message["content"]
                    .as_str()
                    .is_some_and(|content| content.starts_with("follow-up-"))
        })
        .count();
    if !result.had_injections
        || result.final_content.as_deref() != Some("candidate-2")
        || injection_count.load(Ordering::SeqCst) != 5
        || injected_messages != 5
        || requests.len() != 3
    {
        return Err(format!(
            "runner did not cap mid-turn injections: result={result:?} requests={requests:?}"
        )
        .into());
    }
    Ok(())
}

#[test]
fn runtime_runner_streams_provider_events_when_callback_is_configured() -> Result<(), Box<dyn Error>>
{
    let registry = ToolRegistry::new();
    let client = StreamMockProvider::new(
        vec![LlmResponse {
            content: Some("streamed final".to_owned()),
            ..LlmResponse::default()
        }],
        vec![
            ProviderEvent::TextDelta {
                text: "streamed".to_owned(),
            },
            ProviderEvent::Finish {
                usage: json!({"completion_tokens": 2}),
                reason: "stop".to_owned(),
            },
        ],
    );
    let captured = Arc::new(Mutex::new(Vec::<ProviderEvent>::new()));
    let captured_clone = captured.clone();
    let hook_events = Arc::new(Mutex::new(Vec::<String>::new()));
    let mut spec = AgentRunSpec::new(
        vec![json!({"role": "user", "content": "go"})],
        &registry,
        &client,
        "test-model",
    );
    spec.provider_event_callback = Some(Arc::new(move |event| {
        if let Ok(mut captured) = captured_clone.lock() {
            captured.push(event.clone());
        }
    }));
    spec.agent_hook = Some(Arc::new(RecordingHook::new(
        "provider-only",
        hook_events.clone(),
        false,
        "",
    )));

    let result = AgentRunner::new().run(spec)?;
    let captured = captured.lock().map_err(|error| error.to_string())?;
    let hook_events = hook_events.lock().map_err(|error| error.to_string())?;
    if result.final_content.as_deref() != Some("streamed final")
        || captured.len() != 2
        || captured[0]
            != (ProviderEvent::TextDelta {
                text: "streamed".to_owned(),
            })
        || hook_events.iter().any(|event| event.contains(":stream"))
    {
        return Err(format!(
            "provider stream callback drifted: result={result:?} events={captured:?} hook_events={hook_events:?}"
        )
        .into());
    }
    Ok(())
}

#[test]
fn runtime_runner_agent_hook_stream_lifecycle_finalize_and_composite_order(
) -> Result<(), Box<dyn Error>> {
    let mut registry = ToolRegistry::new();
    registry.register(RepeatTool);
    let client = StreamMockProvider::new(
        vec![
            LlmResponse {
                tool_calls: vec![ToolCallRequest::new(
                    "repeat",
                    "repeat",
                    Map::from_iter([("text".to_owned(), json!("ha"))]),
                )],
                finish_reason: "tool_calls".to_owned(),
                ..LlmResponse::default()
            },
            LlmResponse {
                content: Some("done".to_owned()),
                ..LlmResponse::default()
            },
        ],
        vec![ProviderEvent::TextDelta {
            text: "delta".to_owned(),
        }],
    );
    let events = Arc::new(Mutex::new(Vec::<String>::new()));
    let mut composite = CompositeHook::new(vec![
        Arc::new(RecordingHook::new("a", events.clone(), true, "A")),
        Arc::new(PanickingHook),
        Arc::new(RecordingHook::new("b", events.clone(), false, "B")),
    ]);
    composite.push(Arc::new(RecordingHook::new(
        "c",
        events.clone(),
        false,
        "C",
    )));
    let mut spec = AgentRunSpec::new(
        vec![json!({"role": "user", "content": "go"})],
        &registry,
        &client,
        "test-model",
    );
    spec.agent_hook = Some(Arc::new(composite));

    let result = AgentRunner::new().run(spec)?;
    let events = events.lock().map_err(|error| error.to_string())?;
    if result.final_content.as_deref() != Some("doneABC")
        || !events.iter().any(|event| event == "a:stream:delta")
        || !events.iter().any(|event| event == "a:stream_end:true")
        || !events.iter().any(|event| event == "a:stream_end:false")
        || !events.iter().any(|event| event == "a:before_tools:1")
        || !events.iter().any(|event| event == "b:before_tools:1")
        || !events.iter().any(|event| event == "c:before_tools:1")
        || !events.windows(3).any(|window| {
            window
                == [
                    "a:finalize".to_owned(),
                    "b:finalize".to_owned(),
                    "c:finalize".to_owned(),
                ]
        })
        || client
            .inner
            .requests
            .lock()
            .map_err(|error| error.to_string())?
            .len()
            != 2
    {
        return Err(format!(
            "agent hook lifecycle/finalize drifted: result={result:?} events={events:?}"
        )
        .into());
    }
    Ok(())
}

#[test]
fn runtime_runner_retry_wait_callback_observes_provider_retry() -> Result<(), Box<dyn Error>> {
    let registry = ToolRegistry::new();
    let client = MockProvider::new(vec![
        LlmResponse {
            content: Some("retry me".to_owned()),
            finish_reason: "error".to_owned(),
            retry_after: Some(0.01),
            error_should_retry: Some(true),
            ..LlmResponse::default()
        },
        LlmResponse {
            content: Some("ok".to_owned()),
            ..LlmResponse::default()
        },
    ]);
    let waits = Arc::new(Mutex::new(Vec::<String>::new()));
    let waits_capture = waits.clone();
    let mut spec = AgentRunSpec::new(
        vec![json!({"role": "user", "content": "retry"})],
        &registry,
        &client,
        "test-model",
    );
    spec.retry_wait_callback = Some(Arc::new(move |delay, message| {
        if let Ok(mut waits) = waits_capture.lock() {
            waits.push(format!("{delay:.2}:{message}"));
        }
    }));

    let result = AgentRunner::new().run(spec)?;
    let waits = waits.lock().map_err(|error| error.to_string())?;
    if result.final_content.as_deref() != Some("ok")
        || waits.len() != 1
        || !waits[0].contains("0.01")
        || !waits[0].contains("retry")
    {
        return Err(
            format!("retry wait callback drifted: result={result:?} waits={waits:?}").into(),
        );
    }
    Ok(())
}

#[test]
fn runtime_runner_isolates_callback_panics() -> Result<(), Box<dyn Error>> {
    let mut registry = ToolRegistry::new();
    registry.register(RepeatTool);
    let client = StreamMockProvider::new(
        vec![
            LlmResponse {
                finish_reason: "tool_calls".to_owned(),
                tool_calls: vec![ToolCallRequest::new(
                    "repeat",
                    "repeat",
                    Map::from_iter([
                        ("text".to_owned(), json!("ha")),
                        ("times".to_owned(), json!(1)),
                    ]),
                )],
                ..LlmResponse::default()
            },
            LlmResponse {
                content: Some("done after panics".to_owned()),
                ..LlmResponse::default()
            },
        ],
        vec![ProviderEvent::TextDelta {
            text: "streamed".to_owned(),
        }],
    );
    let mut spec = AgentRunSpec::new(
        vec![json!({"role": "user", "content": "go"})],
        &registry,
        &client,
        "test-model",
    );
    spec.mid_turn_injection_callback = Some(Arc::new(|| panic!("mid-turn callback panic")));
    spec.provider_event_callback = Some(Arc::new(|_| panic!("provider callback panic")));
    spec.tool_event_callback = Some(Arc::new(|_| panic!("tool callback panic")));
    spec.checkpoint_callback = Some(Arc::new(|_| panic!("checkpoint callback panic")));

    let result = AgentRunner::new().run(spec)?;
    if result.final_content.as_deref() != Some("done after panics") || result.had_injections {
        return Err(format!("callback panic isolation drifted: {result:?}").into());
    }
    Ok(())
}

#[test]
fn runtime_runner_checkpoint_uses_normalized_tool_results() -> Result<(), Box<dyn Error>> {
    let mut registry = ToolRegistry::new();
    registry.register(LargeTool);
    let workspace = tempfile::tempdir()?;
    let client = MockProvider::new(vec![
        LlmResponse {
            tool_calls: vec![ToolCallRequest::new(
                "large-checkpoint",
                "mcp_large_tool",
                Map::new(),
            )],
            finish_reason: "tool_calls".to_owned(),
            ..LlmResponse::default()
        },
        LlmResponse {
            content: Some("done".to_owned()),
            ..LlmResponse::default()
        },
    ]);
    let checkpoints = Arc::new(Mutex::new(Vec::<Value>::new()));
    let checkpoint_capture = checkpoints.clone();
    let mut spec = AgentRunSpec::new(
        vec![json!({"role": "user", "content": "go"})],
        &registry,
        &client,
        "test-model",
    );
    spec.workspace = Some(workspace.path().to_path_buf());
    spec.session_key = Some("cli:checkpoint-large".to_owned());
    spec.max_tool_result_chars = 32;
    spec.tool_context = safe_mcp_tool_context();
    spec.checkpoint_callback = Some(Arc::new(move |checkpoint| {
        if let Ok(mut checkpoints) = checkpoints.lock() {
            checkpoints.push(checkpoint.clone());
        }
    }));

    let result = AgentRunner::new().run(spec)?;
    let checkpoints = checkpoint_capture
        .lock()
        .map_err(|error| error.to_string())?;
    let tools_completed = checkpoints
        .iter()
        .find(|checkpoint| checkpoint["phase"] == "tools_completed")
        .ok_or("missing tools_completed checkpoint")?;
    let checkpoint_content = tools_completed["completed_tool_results"][0]["content"]
        .as_str()
        .ok_or("checkpoint tool content should be string")?;
    if result.messages[2]["content"] != checkpoint_content
        || !checkpoint_content.contains("tool output persisted")
    {
        return Err(format!("checkpoint used raw tool output: {checkpoints:?}").into());
    }
    Ok(())
}

#[test]
fn runtime_runner_stops_on_ask_user_without_later_tools() -> Result<(), Box<dyn Error>> {
    let calls = Arc::new(Mutex::new(0usize));
    let mut registry = ToolRegistry::new();
    registry.register(AskUserTool::new());
    registry.register(CountingTool {
        calls: calls.clone(),
    });
    let client = MockProvider::new(vec![LlmResponse {
        tool_calls: vec![
            ToolCallRequest::new(
                "ask",
                "ask_user",
                Map::from_iter([
                    ("question".to_owned(), json!("Continue?")),
                    ("options".to_owned(), json!(["Yes", "No"])),
                ]),
            ),
            ToolCallRequest::new("count", "count", Map::new()),
        ],
        finish_reason: "tool_calls".to_owned(),
        ..LlmResponse::default()
    }]);
    let mut spec = AgentRunSpec::new(
        vec![json!({"role": "user", "content": "go"})],
        &registry,
        &client,
        "test",
    );
    spec.max_iterations = 2;
    let result = AgentRunner::new().run(spec)?;
    if result.stop_reason != "ask_user"
        || result.final_content.as_deref() != Some("Continue?")
        || *calls.lock().map_err(|error| error.to_string())? != 0
        || result.messages.len() != 2
        || result.messages[1]["tool_calls"].as_array().map(Vec::len) != Some(1)
    {
        return Err(format!("runner ask_user behavior drifted: {result:?}").into());
    }
    Ok(())
}

#[test]
fn runtime_runner_prioritizes_fatal_tool_error_before_ask_user() -> Result<(), Box<dyn Error>> {
    let mut registry = ToolRegistry::new();
    registry.register(ErrorTool);
    registry.register(AskUserTool::new());
    let client = MockProvider::new(vec![LlmResponse {
        tool_calls: vec![
            ToolCallRequest::new("err", "mcp_error_tool", Map::new()),
            ToolCallRequest::new(
                "ask",
                "ask_user",
                Map::from_iter([("question".to_owned(), json!("Continue?"))]),
            ),
        ],
        finish_reason: "tool_calls".to_owned(),
        ..LlmResponse::default()
    }]);
    let mut spec = AgentRunSpec::new(
        vec![json!({"role": "user", "content": "go"})],
        &registry,
        &client,
        "test",
    );
    spec.fail_on_tool_error = true;
    spec.tool_context = safe_mcp_tool_context();

    let result = AgentRunner::new().run(spec)?;
    if result.stop_reason != "tool_error"
        || result.interrupt.is_some()
        || result.error.as_deref() != Some("Error: simulated failure\n\n[Analyze the error above and try a different approach.]")
        || result.messages[3]["role"] != "tool"
        || result.messages[3]["tool_call_id"] != "ask"
        || !result.messages[3]["content"]
            .as_str()
            .unwrap_or_default()
            .contains("fatal tool error stopped")
    {
        return Err(format!("fatal error should win before ask_user: {result:?}").into());
    }
    Ok(())
}

#[test]
fn runtime_runner_includes_throttled_tool_results_in_checkpoint_and_fatal(
) -> Result<(), Box<dyn Error>> {
    let mut registry = ToolRegistry::new();
    registry.register(WebSearchStub);
    registry.register(AskUserTool::new());
    let repeat_call = |id: &str| {
        ToolCallRequest::new(
            id,
            "web_search",
            Map::from_iter([("query".to_owned(), json!("same query"))]),
        )
    };
    let client = MockProvider::new(vec![
        LlmResponse {
            tool_calls: vec![repeat_call("search-1")],
            finish_reason: "tool_calls".to_owned(),
            ..LlmResponse::default()
        },
        LlmResponse {
            tool_calls: vec![repeat_call("search-2")],
            finish_reason: "tool_calls".to_owned(),
            ..LlmResponse::default()
        },
        LlmResponse {
            tool_calls: vec![
                repeat_call("search-3"),
                ToolCallRequest::new(
                    "ask-after-throttle",
                    "ask_user",
                    Map::from_iter([("question".to_owned(), json!("Continue?"))]),
                ),
            ],
            finish_reason: "tool_calls".to_owned(),
            ..LlmResponse::default()
        },
    ]);
    let checkpoints = Arc::new(Mutex::new(Vec::<Value>::new()));
    let checkpoint_capture = checkpoints.clone();
    let mut spec = AgentRunSpec::new(
        vec![json!({"role": "user", "content": "go"})],
        &registry,
        &client,
        "test",
    );
    spec.fail_on_tool_error = true;
    spec.checkpoint_callback = Some(Arc::new(move |checkpoint| {
        if let Ok(mut checkpoints) = checkpoints.lock() {
            checkpoints.push(checkpoint.clone());
        }
    }));

    let result = AgentRunner::new().run(spec)?;
    let checkpoints = checkpoint_capture
        .lock()
        .map_err(|error| error.to_string())?;
    let final_checkpoint = checkpoints.last().ok_or("missing final checkpoint")?;
    if result.stop_reason != "tool_error"
        || result.interrupt.is_some()
        || final_checkpoint["phase"] != "tools_completed"
        || final_checkpoint["completed_tool_results"][0]["tool_call_id"] != "search-3"
        || !final_checkpoint["completed_tool_results"][0]["content"]
            .as_str()
            .unwrap_or_default()
            .contains("repeated external lookup blocked")
        || final_checkpoint["completed_tool_results"][1]["tool_call_id"] != "ask-after-throttle"
        || !final_checkpoint["completed_tool_results"][1]["content"]
            .as_str()
            .unwrap_or_default()
            .contains("fatal tool error stopped")
    {
        return Err(format!(
            "throttled tool result was not fatal/checkpointed: result={result:?} checkpoints={checkpoints:?}"
        )
        .into());
    }
    Ok(())
}

#[test]
fn runtime_runner_handles_error_empty_length_and_tool_result_governance(
) -> Result<(), Box<dyn Error>> {
    let mut registry = ToolRegistry::new();
    registry.register(ErrorTool);
    registry.register(LargeTool);
    registry.register(EmptyTool);

    let error_client = MockProvider::new(vec![LlmResponse {
        tool_calls: vec![ToolCallRequest::new("err", "mcp_error_tool", Map::new())],
        finish_reason: "tool_calls".to_owned(),
        ..LlmResponse::default()
    }]);
    let mut error_spec = AgentRunSpec::new(
        vec![json!({"role": "user", "content": "go"})],
        &registry,
        &error_client,
        "test",
    );
    error_spec.fail_on_tool_error = true;
    error_spec.tool_context = safe_mcp_tool_context();
    let error_result = AgentRunner::new().run(error_spec)?;
    if error_result.stop_reason != "tool_error"
        || !error_result
            .tool_events
            .iter()
            .any(|event| event.name == "mcp_error_tool")
    {
        return Err(format!("fail_on_tool_error drifted: {error_result:?}").into());
    }

    let workspace = tempfile::tempdir()?;
    let large_client = MockProvider::new(vec![
        LlmResponse {
            tool_calls: vec![ToolCallRequest::new("large", "mcp_large_tool", Map::new())],
            finish_reason: "tool_calls".to_owned(),
            ..LlmResponse::default()
        },
        LlmResponse {
            content: Some("done".to_owned()),
            ..LlmResponse::default()
        },
    ]);
    let mut large_spec = AgentRunSpec::new(
        vec![json!({"role": "user", "content": "go"})],
        &registry,
        &large_client,
        "test",
    );
    large_spec.workspace = Some(workspace.path().to_path_buf());
    large_spec.session_key = Some("cli:direct".to_owned());
    large_spec.max_tool_result_chars = 32;
    large_spec.tool_context = safe_mcp_tool_context();
    let large_result = AgentRunner::new().run(large_spec)?;
    let tool_results_dir = workspace.path().join(".nanobot/tool-results");
    let persisted = std::fs::read_dir(&tool_results_dir)?
        .filter_map(Result::ok)
        .flat_map(|entry| std::fs::read_dir(entry.path()).into_iter().flatten())
        .filter_map(Result::ok)
        .any(|entry| entry.file_name().to_string_lossy() == "large.txt");
    if !large_result.messages[2]["content"]
        .as_str()
        .unwrap_or_default()
        .contains("tool output persisted")
        || !persisted
    {
        return Err(format!("large tool result persistence drifted: {large_result:?}").into());
    }

    let empty_tool_client = MockProvider::new(vec![
        LlmResponse {
            tool_calls: vec![ToolCallRequest::new("empty", "mcp_empty_tool", Map::new())],
            finish_reason: "tool_calls".to_owned(),
            ..LlmResponse::default()
        },
        LlmResponse {
            content: Some("done".to_owned()),
            ..LlmResponse::default()
        },
    ]);
    let mut empty_tool_spec = AgentRunSpec::new(
        vec![json!({"role": "user", "content": "go"})],
        &registry,
        &empty_tool_client,
        "test",
    );
    empty_tool_spec.tool_context = safe_mcp_tool_context();
    let empty_tool_result = AgentRunner::new().run(empty_tool_spec)?;
    if empty_tool_result.messages[2]["content"] != "(mcp_empty_tool completed with no output)" {
        return Err(format!("empty tool marker drifted: {empty_tool_result:?}").into());
    }

    let empty_final_client = MockProvider::new(vec![
        LlmResponse {
            content: Some("   ".to_owned()),
            ..LlmResponse::default()
        },
        LlmResponse {
            content: Some("\n".to_owned()),
            ..LlmResponse::default()
        },
        LlmResponse {
            content: Some("finalized".to_owned()),
            ..LlmResponse::default()
        },
    ]);
    let empty_final_events = Arc::new(Mutex::new(Vec::<String>::new()));
    let mut empty_final_spec = AgentRunSpec::new(
        vec![json!({"role": "user", "content": "go"})],
        &registry,
        &empty_final_client,
        "test",
    );
    empty_final_spec.agent_hook = Some(Arc::new(RecordingHook::new(
        "empty-final",
        empty_final_events.clone(),
        true,
        "",
    )));
    let empty_final = AgentRunner::new().run(empty_final_spec)?;
    if empty_final.final_content.as_deref() != Some("finalized") {
        return Err(format!("empty final retry drifted: {empty_final:?}").into());
    }
    let empty_final_stream_ends = empty_final_events
        .lock()
        .map_err(|error| error.to_string())?
        .iter()
        .filter(|event| event.as_str() == "empty-final:stream_end:false")
        .count();
    if empty_final_stream_ends != 3 {
        return Err(format!(
            "empty final stream lifecycle drifted: stream_ends={empty_final_stream_ends} result={empty_final:?}"
        )
        .into());
    }
    let empty_requests = empty_final_client
        .requests
        .lock()
        .map_err(|error| error.to_string())?;
    if !empty_requests[2].tools.is_empty()
        || empty_requests[2]
            .messages
            .last()
            .and_then(|m| m.get("content"))
            .and_then(Value::as_str)
            != Some("Please provide your response to the user based on the conversation above.")
    {
        return Err(format!("finalization retry request drifted: {empty_requests:?}").into());
    }
    drop(empty_requests);

    let length_client = MockProvider::new(vec![
        LlmResponse {
            content: Some("part one".to_owned()),
            finish_reason: "length".to_owned(),
            ..LlmResponse::default()
        },
        LlmResponse {
            content: Some("part two".to_owned()),
            ..LlmResponse::default()
        },
    ]);
    let length_result = AgentRunner::new().run(AgentRunSpec::new(
        vec![json!({"role": "user", "content": "go"})],
        &registry,
        &length_client,
        "test",
    ))?;
    if length_result.final_content.as_deref() != Some("part two")
        || !length_result.messages.iter().any(|m| {
            m.get("content")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .contains("Output limit reached")
        })
    {
        return Err(format!("length recovery drifted: {length_result:?}").into());
    }

    let model_error_client = MockProvider::new(vec![LlmResponse {
        finish_reason: "error".to_owned(),
        content: None,
        ..LlmResponse::default()
    }]);
    let model_error = AgentRunner::new().run(AgentRunSpec::new(
        vec![json!({"role": "user", "content": "go"})],
        &registry,
        &model_error_client,
        "test",
    ))?;
    if model_error.stop_reason != "error"
        || model_error.messages[1]["content"] != "[Assistant reply unavailable due to model error.]"
    {
        return Err(format!("model error placeholder drifted: {model_error:?}").into());
    }
    Ok(())
}

#[cfg(unix)]
#[test]
fn runtime_runner_rejects_symlinked_tool_result_directory() -> Result<(), Box<dyn Error>> {
    use std::os::unix::fs::symlink;

    let mut registry = ToolRegistry::new();
    registry.register(LargeTool);
    let workspace = tempfile::tempdir()?;
    let outside = tempfile::tempdir()?;
    symlink(outside.path(), workspace.path().join(".nanobot"))?;
    let client = MockProvider::new(vec![
        LlmResponse {
            tool_calls: vec![ToolCallRequest::new("large", "mcp_large_tool", Map::new())],
            finish_reason: "tool_calls".to_owned(),
            ..LlmResponse::default()
        },
        LlmResponse {
            content: Some("done".to_owned()),
            ..LlmResponse::default()
        },
    ]);
    let mut spec = AgentRunSpec::new(
        vec![json!({"role": "user", "content": "go"})],
        &registry,
        &client,
        "test",
    );
    spec.workspace = Some(workspace.path().to_path_buf());
    spec.session_key = Some("cli:direct".to_owned());
    spec.max_tool_result_chars = 32;
    spec.tool_context = safe_mcp_tool_context();
    let result = AgentRunner::new().run(spec)?;
    if result.messages[2]["content"]
        .as_str()
        .unwrap_or_default()
        .contains("tool output persisted")
        || outside.path().join("tool-results").exists()
    {
        return Err(
            format!("symlinked tool-result directory should not be used: {result:?}").into(),
        );
    }
    Ok(())
}

struct RepeatTool;

struct NamedRepeatTool(&'static str);

impl Tool for RepeatTool {
    fn name(&self) -> &str {
        "repeat"
    }

    fn description(&self) -> &str {
        "Repeat text."
    }

    fn parameters(&self) -> Value {
        ToolParameters::new()
            .property("text", StringSchema::new("Text"))
            .required(["text"])
            .to_json_schema()
    }

    fn execute(&self, params: JsonMap) -> ToolResult {
        let text = params
            .get("text")
            .and_then(Value::as_str)
            .unwrap_or_default();
        ToolResult::Text(text.repeat(2))
    }
}

impl Tool for NamedRepeatTool {
    fn name(&self) -> &str {
        self.0
    }

    fn description(&self) -> &str {
        "Repeat text."
    }

    fn parameters(&self) -> Value {
        ToolParameters::new()
            .property("text", StringSchema::new("Text"))
            .required(["text"])
            .to_json_schema()
    }

    fn execute(&self, params: JsonMap) -> ToolResult {
        let text = params
            .get("text")
            .and_then(Value::as_str)
            .unwrap_or_default();
        ToolResult::Text(text.repeat(2))
    }
}

struct SafeDelayTool {
    name: &'static str,
    active: Arc<AtomicUsize>,
    max_active: Arc<AtomicUsize>,
}

impl SafeDelayTool {
    fn new(name: &'static str, active: Arc<AtomicUsize>, max_active: Arc<AtomicUsize>) -> Self {
        Self {
            name,
            active,
            max_active,
        }
    }
}

impl Tool for SafeDelayTool {
    fn name(&self) -> &str {
        self.name
    }

    fn description(&self) -> &str {
        "Concurrency-safe delay."
    }

    fn parameters(&self) -> Value {
        ToolParameters::new().to_json_schema()
    }

    fn read_only(&self) -> bool {
        true
    }

    fn execute(&self, _params: JsonMap) -> ToolResult {
        let active = self.active.fetch_add(1, Ordering::SeqCst) + 1;
        record_max(&self.max_active, active);
        thread::sleep(Duration::from_millis(25));
        self.active.fetch_sub(1, Ordering::SeqCst);
        self.name.into()
    }
}

fn record_max(target: &AtomicUsize, value: usize) {
    let mut observed = target.load(Ordering::SeqCst);
    while observed < value {
        match target.compare_exchange(observed, value, Ordering::SeqCst, Ordering::SeqCst) {
            Ok(_) => return,
            Err(next) => observed = next,
        }
    }
}

struct CountingTool {
    calls: Arc<Mutex<usize>>,
}

struct ErrorTool;

impl Tool for ErrorTool {
    fn name(&self) -> &str {
        "mcp_error_tool"
    }

    fn description(&self) -> &str {
        "Return an error."
    }

    fn parameters(&self) -> Value {
        ToolParameters::new().to_json_schema()
    }

    fn execute(&self, _params: JsonMap) -> ToolResult {
        ToolResult::Text("Error: simulated failure".to_owned())
    }
}

struct LargeTool;

impl Tool for LargeTool {
    fn name(&self) -> &str {
        "mcp_large_tool"
    }

    fn description(&self) -> &str {
        "Return large output."
    }

    fn parameters(&self) -> Value {
        ToolParameters::new().to_json_schema()
    }

    fn execute(&self, _params: JsonMap) -> ToolResult {
        ToolResult::Text("x".repeat(200))
    }
}

struct EmptyTool;

struct McpEchoTool;

struct NamedMcpTool(&'static str);

impl Tool for NamedMcpTool {
    fn name(&self) -> &str {
        self.0
    }

    fn description(&self) -> &str {
        "Lookup test MCP tool."
    }

    fn parameters(&self) -> Value {
        ToolParameters::new()
            .property("query", StringSchema::new("Query"))
            .to_json_schema()
    }

    fn execute(&self, _params: JsonMap) -> ToolResult {
        ToolResult::Text(self.0.to_owned())
    }
}

struct SwitchingMcpTool {
    fresh_name: Arc<AtomicBool>,
    calls: Arc<AtomicUsize>,
}

impl Tool for EmptyTool {
    fn name(&self) -> &str {
        "mcp_empty_tool"
    }

    fn description(&self) -> &str {
        "Return empty output."
    }

    fn parameters(&self) -> Value {
        ToolParameters::new().to_json_schema()
    }

    fn execute(&self, _params: JsonMap) -> ToolResult {
        ToolResult::Text(String::new())
    }
}

impl Tool for McpEchoTool {
    fn name(&self) -> &str {
        "mcp_echo_lookup"
    }

    fn description(&self) -> &str {
        "Echo lookup for deferred Tool Search tests."
    }

    fn parameters(&self) -> Value {
        ToolParameters::new()
            .property("query", StringSchema::new("Query"))
            .required(["query"])
            .to_json_schema()
    }

    fn execute(&self, params: JsonMap) -> ToolResult {
        let query = params
            .get("query")
            .and_then(Value::as_str)
            .unwrap_or_default();
        ToolResult::Text(format!("mcp:{query}"))
    }
}

impl Tool for SwitchingMcpTool {
    fn name(&self) -> &str {
        if self.fresh_name.load(Ordering::SeqCst) {
            "mcp_fresh_lookup"
        } else {
            "mcp_stale_lookup"
        }
    }

    fn description(&self) -> &str {
        "Switching lookup for per-iteration catalog scope tests."
    }

    fn parameters(&self) -> Value {
        ToolParameters::new()
            .property("query", StringSchema::new("Query"))
            .required(["query"])
            .to_json_schema()
    }

    fn execute(&self, params: JsonMap) -> ToolResult {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let query = params
            .get("query")
            .and_then(Value::as_str)
            .unwrap_or_default();
        ToolResult::Text(format!("switch:{query}"))
    }
}

struct WebSearchStub;

struct StaticGitBoundary {
    ages: Vec<MemoryLineAge>,
}

struct RecordingHook {
    label: &'static str,
    events: Arc<Mutex<Vec<String>>>,
    wants_streaming: bool,
    suffix: &'static str,
}

impl RecordingHook {
    fn new(
        label: &'static str,
        events: Arc<Mutex<Vec<String>>>,
        wants_streaming: bool,
        suffix: &'static str,
    ) -> Self {
        Self {
            label,
            events,
            wants_streaming,
            suffix,
        }
    }

    fn record(&self, event: impl Into<String>) {
        if let Ok(mut events) = self.events.lock() {
            events.push(format!("{}:{}", self.label, event.into()));
        }
    }
}

impl AgentHook for RecordingHook {
    fn wants_streaming(&self) -> bool {
        self.wants_streaming
    }

    fn before_iteration(&self, context: &AgentHookContext) {
        let latest_user = context
            .messages
            .iter()
            .rev()
            .find(|message| message.get("role") == Some(&json!("user")))
            .and_then(|message| message.get("content"))
            .and_then(Value::as_str)
            .unwrap_or_default();
        self.record(format!("before:{}:{latest_user}", context.iteration));
    }

    fn on_stream(&self, _context: &AgentHookContext, text: &str) {
        self.record(format!("stream:{text}"));
    }

    fn on_stream_end(&self, _context: &AgentHookContext, resuming: bool) {
        self.record(format!("stream_end:{resuming}"));
    }

    fn before_execute_tools(
        &self,
        _context: &AgentHookContext,
        calls: &[shacs_core::runtime::RuntimeToolCall],
    ) {
        self.record(format!("before_tools:{}", calls.len()));
    }

    fn after_iteration(&self, context: &AgentHookContext) {
        self.record(format!("after:{}", context.iteration));
    }

    fn finalize_content(&self, _context: &AgentHookContext, content: String) -> String {
        self.record("finalize");
        format!("{content}{}", self.suffix)
    }
}

struct PanickingHook;

impl AgentHook for PanickingHook {
    fn before_iteration(&self, _context: &AgentHookContext) {
        panic!("before panic");
    }

    fn on_stream(&self, _context: &AgentHookContext, _text: &str) {
        panic!("stream panic");
    }

    fn on_stream_end(&self, _context: &AgentHookContext, _resuming: bool) {
        panic!("stream end panic");
    }

    fn before_execute_tools(
        &self,
        _context: &AgentHookContext,
        _calls: &[shacs_core::runtime::RuntimeToolCall],
    ) {
        panic!("tools panic");
    }

    fn after_iteration(&self, _context: &AgentHookContext) {
        panic!("after panic");
    }

    fn finalize_content(&self, _context: &AgentHookContext, _content: String) -> String {
        panic!("finalize panic");
    }
}

impl MemoryGitBoundary for StaticGitBoundary {
    fn line_ages(&self, relative_path: &str) -> Result<Option<Vec<MemoryLineAge>>, String> {
        if relative_path == "memory/MEMORY.md" {
            Ok(Some(self.ages.clone()))
        } else {
            Ok(None)
        }
    }
}

impl Tool for WebSearchStub {
    fn name(&self) -> &str {
        "web_search"
    }

    fn description(&self) -> &str {
        "Search stub."
    }

    fn parameters(&self) -> Value {
        ToolParameters::new()
            .property("query", StringSchema::new("Query"))
            .required(["query"])
            .to_json_schema()
    }

    fn execute(&self, _params: JsonMap) -> ToolResult {
        ToolResult::Text("search result".to_owned())
    }
}

impl Tool for CountingTool {
    fn name(&self) -> &str {
        "count"
    }

    fn description(&self) -> &str {
        "Count executions."
    }

    fn parameters(&self) -> Value {
        ToolParameters::new().to_json_schema()
    }

    fn execute(&self, _params: JsonMap) -> ToolResult {
        *self.calls.lock().expect("counter poisoned") += 1;
        ToolResult::Text("counted".to_owned())
    }
}

fn activated_tool_search_config() -> ToolSearchConfig {
    ToolSearchConfig {
        enabled: ToolSearchMode::On,
        threshold_pct: 0,
        search_default_limit: 2,
        max_search_limit: 4,
    }
}

fn safe_mcp_tool_context() -> ToolExecutionContext {
    ToolExecutionContext {
        containment_snapshot: Some(ContainmentSnapshotRef {
            contained: Some(true),
            digest: Some("test-contained".to_owned()),
            summary: Some("non-privileged test containment".to_owned()),
        }),
        permission_mode_snapshot: PermissionModeSnapshot {
            mode: PermissionMode::BypassPermissions,
            source: Some("runtime_agent_test".to_owned()),
            scope_ref: None,
        },
        permission_rule_input: PermissionRuleInput {
            containment: DockerContainmentSnapshot {
                contained: Some(true),
                runtime: ContainerRuntimeKind::Docker,
                root_user: Some(false),
                privileged: Some(false),
                host_mounts_summary: Vec::new(),
                network_mode: ContainerNetworkMode::None,
                digest: Some("test-contained".to_owned()),
                summary: Some("non-privileged test containment".to_owned()),
            },
            protected_targets: Vec::new(),
            proc_exec_summary: Some(ProcExecSummary {
                command_family: "mcp-test".to_owned(),
                target_refs: Vec::new(),
                destructive: false,
                network: false,
                secret_exposure: false,
                summary_available: true,
            }),
        },
        ..ToolExecutionContext::default()
    }
}

fn provider_tool_names(request: &ProviderRequest) -> Result<Vec<String>, Box<dyn Error>> {
    request
        .tools
        .iter()
        .map(|tool| {
            provider_tool_name(tool)
                .map(str::to_owned)
                .ok_or_else(|| "missing provider tool name".to_owned())
        })
        .collect::<Result<Vec<_>, _>>()
        .map_err(Into::into)
}

fn provider_tool_name(tool: &Value) -> Option<&str> {
    tool.get("function")
        .and_then(Value::as_object)
        .and_then(|function| function.get("name"))
        .and_then(Value::as_str)
        .or_else(|| tool.get("name").and_then(Value::as_str))
}

fn mp4_video_bytes_for_runtime(duration_seconds: u64) -> Vec<u8> {
    let mut bytes = mp4_box_for_runtime(b"ftyp", b"isom\0\0\0\0mp42");
    let mut mvhd = Vec::new();
    mvhd.extend_from_slice(&[0, 0, 0, 0]);
    mvhd.extend_from_slice(&0u32.to_be_bytes());
    mvhd.extend_from_slice(&0u32.to_be_bytes());
    mvhd.extend_from_slice(&1u32.to_be_bytes());
    mvhd.extend_from_slice(&(duration_seconds as u32).to_be_bytes());
    let mvhd = mp4_box_for_runtime(b"mvhd", &mvhd);
    bytes.extend_from_slice(&mp4_box_for_runtime(b"moov", &mvhd));
    bytes
}

fn mp4_box_for_runtime(name: &[u8; 4], payload: &[u8]) -> Vec<u8> {
    let size = (8 + payload.len()) as u32;
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&size.to_be_bytes());
    bytes.extend_from_slice(name);
    bytes.extend_from_slice(payload);
    bytes
}

struct MockProvider {
    responses: Mutex<VecDeque<LlmResponse>>,
    requests: Mutex<Vec<ProviderRequest>>,
}

impl MockProvider {
    fn new(responses: Vec<LlmResponse>) -> Self {
        Self {
            responses: Mutex::new(responses.into()),
            requests: Mutex::new(Vec::new()),
        }
    }
}

struct StreamMockProvider {
    inner: MockProvider,
    events: Vec<ProviderEvent>,
}

struct MutatingProvider {
    responses: Mutex<VecDeque<LlmResponse>>,
    requests: Mutex<Vec<ProviderRequest>>,
    after_request: Arc<dyn Fn(usize) + Send + Sync>,
}

impl StreamMockProvider {
    fn new(responses: Vec<LlmResponse>, events: Vec<ProviderEvent>) -> Self {
        Self {
            inner: MockProvider::new(responses),
            events,
        }
    }
}

impl MutatingProvider {
    fn new(responses: Vec<LlmResponse>, after_request: Arc<dyn Fn(usize) + Send + Sync>) -> Self {
        Self {
            responses: Mutex::new(responses.into()),
            requests: Mutex::new(Vec::new()),
            after_request,
        }
    }
}

impl ProviderClient for StreamMockProvider {
    fn chat(&self, request: ProviderRequest) -> Result<LlmResponse, ProviderError> {
        self.inner.chat(request)
    }

    fn chat_stream(
        &self,
        request: ProviderRequest,
        on_event: &mut dyn FnMut(ProviderEvent),
    ) -> Result<LlmResponse, ProviderError> {
        for event in &self.events {
            on_event(event.clone());
        }
        self.inner.chat(request)
    }
}

impl ProviderClient for MockProvider {
    fn chat(&self, request: ProviderRequest) -> Result<LlmResponse, ProviderError> {
        self.requests
            .lock()
            .map_err(|error| provider_error(error.to_string()))?
            .push(request);
        self.responses
            .lock()
            .map_err(|error| provider_error(error.to_string()))?
            .pop_front()
            .ok_or_else(|| provider_error("no mock response"))
    }

    fn chat_stream(
        &self,
        request: ProviderRequest,
        _on_event: &mut dyn FnMut(ProviderEvent),
    ) -> Result<LlmResponse, ProviderError> {
        self.chat(request)
    }
}

impl ProviderClient for MutatingProvider {
    fn chat(&self, request: ProviderRequest) -> Result<LlmResponse, ProviderError> {
        let request_count = {
            let mut requests = self
                .requests
                .lock()
                .map_err(|error| provider_error(error.to_string()))?;
            requests.push(request);
            requests.len()
        };
        (self.after_request)(request_count);
        self.responses
            .lock()
            .map_err(|error| provider_error(error.to_string()))?
            .pop_front()
            .ok_or_else(|| provider_error("no mock response"))
    }

    fn chat_stream(
        &self,
        request: ProviderRequest,
        _on_event: &mut dyn FnMut(ProviderEvent),
    ) -> Result<LlmResponse, ProviderError> {
        self.chat(request)
    }
}

fn provider_error(message: impl Into<String>) -> ProviderError {
    ProviderError::Api {
        status: None,
        message: message.into(),
        retryable: false,
        headers: BTreeMap::new(),
        body: None,
    }
}
