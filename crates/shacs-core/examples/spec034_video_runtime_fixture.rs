use serde_json::json;
use shacs_core::controlled_child::{
    run_generic_argv, ControlledChildCommand, ControlledChildOutcome,
};
use shacs_core::runtime::{
    AnalyzerInvocation, ContextBuildRequest, ContextBuilder, VideoContextAnalysis,
    VideoContextAnalyzer, VideoContextError, VideoContextRequest, VideoMetadata,
};
use std::error::Error;
use std::path::Path;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

#[derive(Debug)]
struct ControlledTimeoutAnalyzer;

impl VideoContextAnalyzer for ControlledTimeoutAnalyzer {
    fn analyze(
        &self,
        invocation: &AnalyzerInvocation,
        _request: VideoContextRequest,
    ) -> Result<VideoContextAnalysis, VideoContextError> {
        let script = "trap '' TERM; /bin/sh -c 'trap \"\" TERM; echo $$ > ../../driver-descendant.pid; exec sleep 30' & wait";
        let mut command = ControlledChildCommand::new(
            ["/bin/sh", "-c", script],
            invocation.staging_directory(),
            Duration::from_millis(100),
        );
        command.termination_grace = Duration::from_millis(50);
        command.output_limit = 256;
        invocation.apply_to_controlled_child(&mut command);
        let receipt = run_generic_argv(&command, &invocation.controlled_child_abort())
            .map_err(|error| VideoContextError::Failed(error.to_string()))?;
        match receipt.outcome {
            ControlledChildOutcome::TimedOut => Err(VideoContextError::TimedOut),
            ControlledChildOutcome::Aborted => Err(VideoContextError::Cancelled),
            ControlledChildOutcome::Succeeded { .. }
            | ControlledChildOutcome::Failed { .. }
            | ControlledChildOutcome::InvalidCwd => Err(VideoContextError::Failed(
                "unexpected fixture outcome".to_owned(),
            )),
        }
    }
}

#[derive(Debug)]
struct SuccessAnalyzer;

impl VideoContextAnalyzer for SuccessAnalyzer {
    fn analyze(
        &self,
        _invocation: &AnalyzerInvocation,
        request: VideoContextRequest,
    ) -> Result<VideoContextAnalysis, VideoContextError> {
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
            scene_summary: Some("fixture resumed".to_owned()),
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

fn main() -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    let media_root = tempfile::tempdir()?;
    let attachments = media_root.path().join("attachments/cli");
    std::fs::create_dir_all(&attachments)?;
    let video = attachments.join("clip.mp4");
    std::fs::write(&video, mp4_video_bytes(6))?;
    let media = vec![video.to_string_lossy().to_string()];
    let staging_root = workspace.path().join("video-analyzer-staging");
    let timed_out = ContextBuilder::new(workspace.path())
        .with_media_roots([media_root.path().to_path_buf()])
        .with_video_analyzer(Arc::new(ControlledTimeoutAnalyzer))
        .with_video_analyzer_staging_root(staging_root.clone())
        .build_messages(ContextBuildRequest {
            media: &media,
            ..ContextBuildRequest::new("inspect")
        });
    let timeout_observed = serde_json::to_string(&timed_out)?.contains("[attachment:timeout]");
    let pid = std::fs::read_to_string(workspace.path().join("driver-descendant.pid"))?
        .trim()
        .parse::<i32>()?;
    wait_for_process_exit(pid)?;
    wait_for_absence(&staging_root)?;

    let resumed = ContextBuilder::new(workspace.path())
        .with_media_roots([media_root.path().to_path_buf()])
        .with_video_analyzer(Arc::new(SuccessAnalyzer))
        .with_video_analyzer_staging_root(staging_root.clone())
        .build_messages(ContextBuildRequest {
            media: &media,
            ..ContextBuildRequest::new("resume")
        });
    let resume_observed = serde_json::to_string(&resumed)?.contains("fixture resumed");
    wait_for_absence(&staging_root)?;
    if !timeout_observed || !resume_observed {
        return Err("video runtime fixture did not observe timeout and resume".into());
    }
    println!(
        "{}",
        serde_json::to_string(&json!({
            "timeout": "observed",
            "resume": "included",
            "descendant_running": false,
            "staging_remaining": false
        }))?
    );
    Ok(())
}

fn wait_for_absence(path: &Path) -> Result<(), Box<dyn Error>> {
    let deadline = Instant::now() + Duration::from_secs(2);
    while path.exists() && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(10));
    }
    if path.exists() {
        return Err(format!("staging remained: {}", path.display()).into());
    }
    Ok(())
}

fn wait_for_process_exit(pid: i32) -> Result<(), Box<dyn Error>> {
    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline {
        let running = std::process::Command::new("/bin/kill")
            .args(["-0", &pid.to_string()])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()?
            .success();
        if !running {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(10));
    }
    Err(format!("fixture descendant remained: pid={pid}").into())
}

fn mp4_video_bytes(duration_seconds: u64) -> Vec<u8> {
    let mut mvhd_payload = vec![0u8; 20];
    mvhd_payload[12..16].copy_from_slice(&1u32.to_be_bytes());
    mvhd_payload[16..20].copy_from_slice(
        &u32::try_from(duration_seconds)
            .unwrap_or(u32::MAX)
            .to_be_bytes(),
    );
    let mvhd = mp4_box(*b"mvhd", &mvhd_payload);
    let moov = mp4_box(*b"moov", &mvhd);
    let mut bytes = mp4_box(*b"ftyp", b"isom\0\0\0\0");
    bytes.extend(moov);
    bytes
}

fn mp4_box(box_type: [u8; 4], payload: &[u8]) -> Vec<u8> {
    let size = u32::try_from(payload.len() + 8).unwrap_or(u32::MAX);
    let mut bytes = Vec::with_capacity(payload.len() + 8);
    bytes.extend(size.to_be_bytes());
    bytes.extend(box_type);
    bytes.extend(payload);
    bytes
}
