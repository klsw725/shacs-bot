#[derive(Debug)]
struct RuntimeVideoAnalyzer;

impl VideoContextAnalyzer for RuntimeVideoAnalyzer {
    fn analyze(
        &self,
        _invocation: &shacs_core::runtime::AnalyzerInvocation,
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

