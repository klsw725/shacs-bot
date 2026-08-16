use super::included_analysis;
use super::support::VideoFixture;
use shacs_core::runtime::{
    AnalyzerInvocation, AnalyzerMediaProvenance, CancellationToken, ContextBuildRequest,
    VideoAnalyzerSnapshotProjection, VideoComponentFailure, VideoContextAnalysis,
    VideoContextAnalyzer, VideoContextError, VideoContextRequest,
};
use shacs_projection::Spec031ExternalOwnerRef;
use std::error::Error;
use std::path::PathBuf;
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

#[derive(Debug)]
struct LateSuccessAnalyzer {
    started: Mutex<Option<mpsc::Sender<PathBuf>>>,
    release: Mutex<mpsc::Receiver<()>>,
    finished: mpsc::Sender<()>,
}

impl VideoContextAnalyzer for LateSuccessAnalyzer {
    fn analyze(
        &self,
        invocation: &AnalyzerInvocation,
        request: VideoContextRequest,
    ) -> Result<VideoContextAnalysis, VideoContextError> {
        std::fs::write(invocation.staging_directory().join("partial"), b"partial")
            .map_err(|error| VideoContextError::Failed(error.to_string()))?;
        if let Some(started) = self
            .started
            .lock()
            .map_err(|error| VideoContextError::Failed(error.to_string()))?
            .take()
        {
            let _ = started.send(invocation.staging_directory().to_path_buf());
        }
        self.release
            .lock()
            .map_err(|error| VideoContextError::Failed(error.to_string()))?
            .recv()
            .map_err(|error| VideoContextError::Failed(error.to_string()))?;
        self.finished
            .send(())
            .map_err(|error| VideoContextError::Failed(error.to_string()))?;
        let mut analysis = included_analysis(request.duration_seconds);
        analysis.scene_summary = Some("late success must be discarded".to_owned());
        Ok(analysis)
    }
}

#[derive(Debug)]
struct CapturingAnalyzer {
    captured: mpsc::Sender<(String, String, AnalyzerMediaProvenance, PathBuf)>,
}

impl VideoContextAnalyzer for CapturingAnalyzer {
    fn analyze(
        &self,
        invocation: &AnalyzerInvocation,
        request: VideoContextRequest,
    ) -> Result<VideoContextAnalysis, VideoContextError> {
        let owner = invocation
            .owner_ref()
            .map(|value| value.as_str().to_owned())
            .unwrap_or_default();
        let snapshot = invocation
            .snapshot_ref()
            .map(|value| value.snapshot_id.clone())
            .unwrap_or_default();
        self.captured
            .send((
                owner,
                snapshot,
                invocation.provenance(),
                invocation.staging_directory().to_path_buf(),
            ))
            .map_err(|error| VideoContextError::Failed(error.to_string()))?;
        let mut analysis = included_analysis(request.duration_seconds);
        analysis.subtitles = Some("s".repeat(request.policy.max_subtitle_chars * 2));
        analysis.component_failures = (0..64)
            .map(|index| VideoComponentFailure {
                component: format!("component-{index}"),
                reason: "r".repeat(400),
            })
            .collect();
        Ok(analysis)
    }
}

#[test]
fn cooperative_cancel_freezes_terminal_and_discards_late_success() -> Result<(), Box<dyn Error>> {
    let fixture = VideoFixture::new()?;
    let cancellation = CancellationToken::new();
    let invocation = AnalyzerInvocation::new(fixture.staging_root(), cancellation.clone());
    let (started_tx, started_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let (finished_tx, finished_rx) = mpsc::channel();
    let context = fixture.context(Arc::new(LateSuccessAnalyzer {
        started: Mutex::new(Some(started_tx)),
        release: Mutex::new(release_rx),
        finished: finished_tx,
    }));
    let media = fixture.media.clone();
    let worker = thread::spawn(move || {
        context.build_messages(ContextBuildRequest {
            media: &media,
            analyzer_invocation: Some(invocation),
            ..ContextBuildRequest::new("inspect")
        })
    });
    let staging = started_rx.recv_timeout(Duration::from_secs(1))?;

    cancellation.cancel();
    cancellation.cancel();
    release_tx.send(())?;
    let messages = worker.join().map_err(|_| "context worker panicked")?;
    finished_rx.recv_timeout(Duration::from_secs(1))?;

    let serialized = serde_json::to_string(&messages)?;
    assert!(serialized.contains("[attachment:cancelled]"));
    assert!(!serialized.contains("late success must be discarded"));
    assert!(!staging.exists());
    assert!(!fixture.staging_root().exists());
    let resumed = fixture
        .context(Arc::new(super::FixtureAnalyzer))
        .build_messages(ContextBuildRequest {
            media: &fixture.media,
            ..ContextBuildRequest::new("resume")
        });
    assert!(serde_json::to_string(&resumed)?.contains("[attachment:included_text]"));
    Ok(())
}

#[test]
fn deadline_freezes_timeout_and_discards_non_cooperative_late_success() -> Result<(), Box<dyn Error>>
{
    let fixture = VideoFixture::new()?;
    let invocation = AnalyzerInvocation::new(fixture.staging_root(), CancellationToken::new())
        .with_deadline(Instant::now() + Duration::from_millis(20));
    let (started_tx, started_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let (finished_tx, finished_rx) = mpsc::channel();
    let context = fixture.context(Arc::new(LateSuccessAnalyzer {
        started: Mutex::new(Some(started_tx)),
        release: Mutex::new(release_rx),
        finished: finished_tx,
    }));
    let media = fixture.media.clone();
    let worker = thread::spawn(move || {
        context.build_messages(ContextBuildRequest {
            media: &media,
            analyzer_invocation: Some(invocation),
            ..ContextBuildRequest::new("inspect")
        })
    });
    let staging = started_rx.recv_timeout(Duration::from_secs(1))?;
    let (deadline_latch_tx, deadline_latch_rx) = mpsc::channel::<()>();
    let _deadline_latch = deadline_latch_tx;
    let _ = deadline_latch_rx.recv_timeout(Duration::from_millis(30));
    release_tx.send(())?;
    let messages = worker.join().map_err(|_| "context worker panicked")?;
    finished_rx.recv_timeout(Duration::from_secs(1))?;

    let serialized = serde_json::to_string(&messages)?;
    assert!(serialized.contains("[attachment:timeout]"));
    assert!(!serialized.contains("late success must be discarded"));
    assert!(!staging.exists());
    assert!(!fixture.staging_root().exists());
    Ok(())
}

#[test]
fn invocation_preserves_owner_snapshot_provenance_and_output_bounds() -> Result<(), Box<dyn Error>>
{
    let fixture = VideoFixture::new()?;
    let owner_ref = Spec031ExternalOwnerRef::try_new("spec034://media/analyzer/fixture")?;
    let snapshot_ref = VideoAnalyzerSnapshotProjection {
        snapshot_id: "snapshot:current".to_owned(),
        provenance_digest: format!("sha256:{}", "a".repeat(64)),
    };
    let (captured_tx, captured_rx) = mpsc::channel();
    let context = fixture
        .context(Arc::new(CapturingAnalyzer {
            captured: captured_tx,
        }))
        .with_video_analyzer_owner_refs(owner_ref.clone(), snapshot_ref.clone());
    let messages = context.build_messages(ContextBuildRequest {
        media: &fixture.media,
        ..ContextBuildRequest::new("inspect")
    });
    let (owner, snapshot, provenance, staging) =
        captured_rx.recv_timeout(Duration::from_secs(1))?;

    assert_eq!(owner, owner_ref.as_str());
    assert_eq!(snapshot, snapshot_ref.snapshot_id);
    assert_eq!(provenance, AnalyzerMediaProvenance::Inbound);
    assert!(serde_json::to_string(&messages)?.contains("[attachment:truncated]"));
    let video_text = messages[1]["content"]
        .as_array()
        .and_then(|blocks| {
            blocks
                .iter()
                .filter_map(|block| block["text"].as_str())
                .find(|text| text.contains("[Attachment content warning]"))
        })
        .ok_or("missing bounded video evidence")?;
    assert!(video_text.contains("component-7"));
    assert!(!video_text.contains("component-8"));
    assert!(video_text.contains("component failures truncated"));
    assert!(!staging.exists());
    assert!(!fixture.staging_root().exists());
    Ok(())
}

#[test]
fn generated_provenance_remains_distinct_from_inbound() -> Result<(), Box<dyn Error>> {
    let fixture = VideoFixture::new()?;
    let (captured_tx, captured_rx) = mpsc::channel();
    let context = fixture
        .context(Arc::new(CapturingAnalyzer {
            captured: captured_tx,
        }))
        .with_video_media_provenance(AnalyzerMediaProvenance::Generated);

    let _messages = context.build_messages(ContextBuildRequest {
        media: &fixture.media,
        ..ContextBuildRequest::new("inspect")
    });
    let (_, _, provenance, staging) = captured_rx.recv_timeout(Duration::from_secs(1))?;

    assert_eq!(provenance, AnalyzerMediaProvenance::Generated);
    assert!(!staging.exists());
    assert!(!fixture.staging_root().exists());
    Ok(())
}
