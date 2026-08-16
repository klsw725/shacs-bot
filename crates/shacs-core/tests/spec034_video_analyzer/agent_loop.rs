use super::included_analysis;
use super::support::VideoFixture;
use shacs_core::runtime::{
    ActiveLoopTask, AgentLoop, AgentLoopConfig, AnalyzerInvocation, AutomationExecutionControl,
    CancellationToken, InboundMessage, LoopTaskRegisterResult, LoopTaskRegistry, MessageBus,
    SessionManager, VideoContextAnalysis, VideoContextAnalyzer, VideoContextError,
    VideoContextRequest,
};
use shacs_core::tools::ToolRegistry;
use shacs_providers::{LlmResponse, ProviderClient, ProviderError, ProviderEvent, ProviderRequest};
use std::error::Error;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{mpsc, Arc};
use std::time::Duration;

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

#[derive(Debug)]
struct DeadlineAnalyzer {
    captured: mpsc::Sender<bool>,
}

impl VideoContextAnalyzer for DeadlineAnalyzer {
    fn analyze(
        &self,
        invocation: &AnalyzerInvocation,
        request: VideoContextRequest,
    ) -> Result<VideoContextAnalysis, VideoContextError> {
        self.captured
            .send(invocation.remaining_duration().is_some())
            .map_err(|error| VideoContextError::Failed(error.to_string()))?;
        Ok(included_analysis(request.duration_seconds))
    }
}

struct OkProvider;

impl ProviderClient for OkProvider {
    fn chat(&self, _request: ProviderRequest) -> Result<LlmResponse, ProviderError> {
        Ok(LlmResponse {
            content: Some("ok".to_owned()),
            ..LlmResponse::default()
        })
    }

    fn chat_stream(
        &self,
        request: ProviderRequest,
        _on_event: &mut dyn FnMut(ProviderEvent),
    ) -> Result<LlmResponse, ProviderError> {
        self.chat(request)
    }
}

#[test]
fn agent_loop_observes_pre_start_cancel_before_video_context_build() -> Result<(), Box<dyn Error>> {
    let fixture = VideoFixture::new()?;
    let calls = Arc::new(AtomicUsize::new(0));
    let context = fixture.context(Arc::new(CountingAnalyzer {
        calls: Arc::clone(&calls),
    }));
    let tasks = LoopTaskRegistry::new();
    let cancellation = CancellationToken::new();
    cancellation.cancel();
    assert_eq!(
        tasks.register(ActiveLoopTask::new(
            "cli:video-pre-cancel",
            "video-task",
            cancellation,
        )),
        LoopTaskRegisterResult::Registered
    );
    let registry = ToolRegistry::new();
    let provider = OkProvider;
    let mut runtime = AgentLoop::new(
        MessageBus::new(),
        SessionManager::new(fixture.workspace())?,
        context,
        &registry,
        &provider,
        AgentLoopConfig::new(fixture.workspace(), "test-model"),
    )
    .with_loop_task_registry(tasks);
    let mut message = InboundMessage::new("cli", "user", "direct", "inspect");
    message.media = fixture.media.clone();
    message.session_key_override = Some("cli:video-pre-cancel".to_owned());

    let result = runtime.process_message(message)?;

    assert_eq!(result.stop_reason, "cancelled");
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert!(!fixture.staging_root().exists());
    Ok(())
}

#[test]
fn agent_loop_passes_turn_deadline_before_video_context_build() -> Result<(), Box<dyn Error>> {
    let fixture = VideoFixture::new()?;
    let (captured_tx, captured_rx) = mpsc::channel();
    let context = fixture.context(Arc::new(DeadlineAnalyzer {
        captured: captured_tx,
    }));
    let registry = ToolRegistry::new();
    let provider = OkProvider;
    let mut config = AgentLoopConfig::new(fixture.workspace(), "test-model");
    config.execution_control = Some(AutomationExecutionControl::with_timeout(
        "video-context-deadline",
        Duration::from_secs(5),
    ));
    let mut runtime = AgentLoop::new(
        MessageBus::new(),
        SessionManager::new(fixture.workspace())?,
        context,
        &registry,
        &provider,
        config,
    );
    let mut message = InboundMessage::new("cli", "user", "direct", "inspect");
    message.media = fixture.media.clone();

    let result = runtime.process_message(message)?;

    assert_eq!(result.final_content.as_deref(), Some("ok"));
    assert!(captured_rx.recv_timeout(Duration::from_secs(1))?);
    assert!(!fixture.staging_root().exists());
    Ok(())
}
