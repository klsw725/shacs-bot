use super::included_analysis;
use super::support::VideoFixture;
use shacs_core::runtime::{
    AnalyzerInvocation, CancellationToken, ContextBuildRequest, VideoContextAnalysis,
    VideoContextAnalyzer, VideoContextError, VideoContextRequest,
};
use std::error::Error;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::Duration;

#[derive(Debug)]
struct BlockingFirstAnalyzer {
    calls: Arc<AtomicUsize>,
    first_started: Mutex<Option<mpsc::Sender<PathBuf>>>,
    first_release: Mutex<mpsc::Receiver<()>>,
}

impl VideoContextAnalyzer for BlockingFirstAnalyzer {
    fn analyze(
        &self,
        invocation: &AnalyzerInvocation,
        request: VideoContextRequest,
    ) -> Result<VideoContextAnalysis, VideoContextError> {
        let call = self.calls.fetch_add(1, Ordering::SeqCst);
        if call == 0 {
            if let Some(started) = self
                .first_started
                .lock()
                .map_err(|error| VideoContextError::Failed(error.to_string()))?
                .take()
            {
                started
                    .send(invocation.staging_directory().to_path_buf())
                    .map_err(|error| VideoContextError::Failed(error.to_string()))?;
            }
            self.first_release
                .lock()
                .map_err(|error| VideoContextError::Failed(error.to_string()))?
                .recv()
                .map_err(|error| VideoContextError::Failed(error.to_string()))?;
        }
        Ok(included_analysis(request.duration_seconds))
    }
}

#[derive(Debug)]
struct PanicOnceAnalyzer {
    calls: Arc<AtomicUsize>,
}

impl VideoContextAnalyzer for PanicOnceAnalyzer {
    fn analyze(
        &self,
        _invocation: &AnalyzerInvocation,
        request: VideoContextRequest,
    ) -> Result<VideoContextAnalysis, VideoContextError> {
        if self.calls.fetch_add(1, Ordering::SeqCst) == 0 {
            panic!("analyzer panic fixture");
        }
        Ok(included_analysis(request.duration_seconds))
    }
}

#[derive(Debug)]
struct CountingAnalyzer {
    calls: Arc<AtomicUsize>,
}

impl VideoContextAnalyzer for CountingAnalyzer {
    fn analyze(
        &self,
        _invocation: &AnalyzerInvocation,
        request: VideoContextRequest,
    ) -> Result<VideoContextAnalysis, VideoContextError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(included_analysis(request.duration_seconds))
    }
}

#[test]
fn context_clones_reject_a_second_worker_while_the_shared_analyzer_is_busy(
) -> Result<(), Box<dyn Error>> {
    let fixture = VideoFixture::new()?;
    let calls = Arc::new(AtomicUsize::new(0));
    let (started_tx, started_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let context = fixture.context(Arc::new(BlockingFirstAnalyzer {
        calls: Arc::clone(&calls),
        first_started: Mutex::new(Some(started_tx)),
        first_release: Mutex::new(release_rx),
    }));
    let concurrent_context = context.clone();
    let first_media = fixture.media.clone();
    let first = thread::spawn(move || {
        context.build_messages(ContextBuildRequest {
            media: &first_media,
            ..ContextBuildRequest::new("first")
        })
    });
    let first_staging = started_rx.recv_timeout(Duration::from_secs(1))?;

    let second = concurrent_context.build_messages(ContextBuildRequest {
        media: &fixture.media,
        ..ContextBuildRequest::new("second")
    });
    release_tx.send(())?;
    let first_messages = first.join().map_err(|_| "first context worker panicked")?;

    let second_serialized = serde_json::to_string(&second)?;
    assert!(second_serialized.contains("[attachment:extraction_failed]"));
    assert!(second_serialized.contains("video analyzer is busy"));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert!(serde_json::to_string(&first_messages)?.contains("[attachment:included_text]"));
    assert!(!first_staging.exists());
    assert!(!fixture.staging_root().exists());
    Ok(())
}

#[test]
fn pre_cancelled_invocation_does_not_call_the_analyzer() -> Result<(), Box<dyn Error>> {
    let fixture = VideoFixture::new()?;
    let calls = Arc::new(AtomicUsize::new(0));
    let cancellation = CancellationToken::new();
    cancellation.cancel();
    let messages = fixture
        .context(Arc::new(CountingAnalyzer {
            calls: Arc::clone(&calls),
        }))
        .build_messages(ContextBuildRequest {
            media: &fixture.media,
            analyzer_invocation: Some(AnalyzerInvocation::new(
                fixture.staging_root(),
                cancellation,
            )),
            ..ContextBuildRequest::new("cancelled")
        });

    assert!(serde_json::to_string(&messages)?.contains("[attachment:cancelled]"));
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert!(!fixture.staging_root().exists());
    Ok(())
}

#[test]
fn analyzer_panic_maps_to_failure_and_releases_the_shared_gate() -> Result<(), Box<dyn Error>> {
    let fixture = VideoFixture::new()?;
    let calls = Arc::new(AtomicUsize::new(0));
    let context = fixture.context(Arc::new(PanicOnceAnalyzer {
        calls: Arc::clone(&calls),
    }));

    let failed = context.clone().build_messages(ContextBuildRequest {
        media: &fixture.media,
        ..ContextBuildRequest::new("panic")
    });
    let resumed = context.build_messages(ContextBuildRequest {
        media: &fixture.media,
        ..ContextBuildRequest::new("resume")
    });

    assert!(serde_json::to_string(&failed)?.contains("[attachment:extraction_failed]"));
    assert!(serde_json::to_string(&resumed)?.contains("[attachment:included_text]"));
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    assert!(!fixture.staging_root().exists());
    Ok(())
}
