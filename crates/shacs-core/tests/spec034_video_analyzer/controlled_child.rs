use super::included_analysis;
use super::support::{wait_for_absence, wait_for_presence, wait_for_process_exit, VideoFixture};
use shacs_core::controlled_child::{
    run_generic_argv, ControlledChildCommand, ControlledChildOutcome,
};
use shacs_core::runtime::{
    AnalyzerInvocation, CancellationToken, ContextBuildRequest, VideoContextAnalysis,
    VideoContextAnalyzer, VideoContextError, VideoContextRequest,
};
use std::error::Error;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

#[derive(Debug)]
struct ControlledAnalyzer {
    script: String,
    timeout: Duration,
    output_limit: usize,
}

impl VideoContextAnalyzer for ControlledAnalyzer {
    fn analyze(
        &self,
        invocation: &AnalyzerInvocation,
        request: VideoContextRequest,
    ) -> Result<VideoContextAnalysis, VideoContextError> {
        let mut command = ControlledChildCommand::new(
            ["/bin/sh", "-c", self.script.as_str()],
            invocation.staging_directory(),
            self.timeout,
        );
        command.output_limit = self.output_limit;
        command.termination_grace = Duration::from_millis(50);
        invocation.apply_to_controlled_child(&mut command);
        let receipt = run_generic_argv(&command, &invocation.controlled_child_abort())
            .map_err(|error| VideoContextError::Failed(error.to_string()))?;
        match receipt.outcome {
            ControlledChildOutcome::Succeeded { .. } if receipt.stdout.truncated => Err(
                VideoContextError::Failed("analyzer output exceeded limit".to_owned()),
            ),
            ControlledChildOutcome::Succeeded { .. } => {
                serde_json::from_slice::<serde_json::Value>(&receipt.stdout.captured).map_err(
                    |_| VideoContextError::Failed("analyzer output malformed".to_owned()),
                )?;
                Ok(included_analysis(request.duration_seconds))
            }
            ControlledChildOutcome::TimedOut => Err(VideoContextError::TimedOut),
            ControlledChildOutcome::Aborted => Err(VideoContextError::Cancelled),
            ControlledChildOutcome::Failed { .. } | ControlledChildOutcome::InvalidCwd => Err(
                VideoContextError::Failed("controlled analyzer failed".to_owned()),
            ),
        }
    }
}

#[test]
#[cfg(unix)]
fn controlled_child_timeout_cleans_descendant_and_staging() -> Result<(), Box<dyn Error>> {
    let fixture = VideoFixture::new()?;
    let script = "trap '' TERM; /bin/sh -c 'trap \"\" TERM; echo $$ > ../../descendant.pid; exec sleep 30' & wait";
    let context = fixture.context(Arc::new(ControlledAnalyzer {
        script: script.to_owned(),
        timeout: Duration::from_millis(100),
        output_limit: 1_024,
    }));

    let messages = context.build_messages(ContextBuildRequest {
        media: &fixture.media,
        ..ContextBuildRequest::new("inspect")
    });

    assert!(serde_json::to_string(&messages)?.contains("[attachment:timeout]"));
    let pid = std::fs::read_to_string(fixture.workspace().join("descendant.pid"))?
        .trim()
        .parse::<i32>()?;
    wait_for_process_exit(pid)?;
    wait_for_absence(&fixture.staging_root())?;
    Ok(())
}

#[test]
#[cfg(unix)]
fn controlled_child_abort_is_cooperative_and_does_not_publish_success() -> Result<(), Box<dyn Error>>
{
    let fixture = VideoFixture::new()?;
    let cancellation = CancellationToken::new();
    let invocation = AnalyzerInvocation::new(fixture.staging_root(), cancellation.clone());
    let started = fixture.workspace().join("abort-started");
    let pid_path = fixture.workspace().join("abort.pid");
    let context = fixture.context(Arc::new(ControlledAnalyzer {
        script: "echo $$ > ../../abort.pid; touch ../../abort-started; sleep 30; printf '{\"scene\":\"late-controlled-child-success-9f31\"}'".to_owned(),
        timeout: Duration::from_secs(30),
        output_limit: 1_024,
    }));
    let media = fixture.media.clone();
    let worker = thread::spawn(move || {
        context.build_messages(ContextBuildRequest {
            media: &media,
            analyzer_invocation: Some(invocation),
            ..ContextBuildRequest::new("inspect")
        })
    });
    wait_for_presence(&started)?;

    cancellation.cancel();
    let messages = worker.join().map_err(|_| "context worker panicked")?;

    let serialized = serde_json::to_string(&messages)?;
    assert!(serialized.contains("[attachment:cancelled]"));
    assert!(!serialized.contains("late-controlled-child-success-9f31"));
    let pid = std::fs::read_to_string(pid_path)?.trim().parse::<i32>()?;
    wait_for_process_exit(pid)?;
    wait_for_absence(&fixture.staging_root())?;
    Ok(())
}

#[test]
#[cfg(unix)]
fn malformed_and_oversized_child_output_fail_without_staged_evidence() -> Result<(), Box<dyn Error>>
{
    for (script, output_limit) in [
        ("printf 'not-json'", 1_024usize),
        (
            "i=0; while [ $i -lt 20000 ]; do printf x; i=$((i+1)); done",
            128usize,
        ),
    ] {
        let fixture = VideoFixture::new()?;
        let context = fixture.context(Arc::new(ControlledAnalyzer {
            script: script.to_owned(),
            timeout: Duration::from_secs(1),
            output_limit,
        }));

        let messages = context.build_messages(ContextBuildRequest {
            media: &fixture.media,
            ..ContextBuildRequest::new("inspect")
        });

        let serialized = serde_json::to_string(&messages)?;
        assert!(serialized.contains("[attachment:extraction_failed]"));
        assert!(!serialized.contains("not-json"));
        wait_for_absence(&fixture.staging_root())?;
    }
    Ok(())
}
