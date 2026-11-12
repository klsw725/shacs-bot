use super::{
    automation_worker, classify_runtime_ownership_marker, durable_work_admission_error,
    durable_work_state_for_owner, now_millis, read_runtime_ownership_marker,
    runtime_durable_event_root, runtime_durable_work_payload_root, runtime_ownership_marker_path,
    sleep_with_stop, AgentLoopChatCompletionAdapter, CliError, RuntimeOwnershipMutationLock,
    RuntimeOwnershipState, SessionManager, ThreadEndSignal, DURABLE_WORK_LEASE_DURATION_MS,
};
use shacs_api::ApiError;
use shacs_core::runtime::{
    AgentLoop, DurableWorkDispatcher, MessageBus, SurfaceActionOutcomeKind, SurfaceApprovalRequest,
    SURFACE_APPROVAL_WORK_KIND,
};
use shacs_redaction::redact_string;
use shacs_session::durable_work::WorkTerminalKind;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

pub(crate) struct SurfaceApprovalWorker {
    stop: Arc<AtomicBool>,
    failed: Arc<AtomicBool>,
    handle: Option<thread::JoinHandle<Result<(), CliError>>>,
}

impl SurfaceApprovalWorker {
    fn spawn(work: impl FnOnce(&AtomicBool) -> Result<(), CliError> + Send + 'static) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let failed = Arc::new(AtomicBool::new(false));
        let (worker_stop, worker_failed) = (stop.clone(), failed.clone());
        let handle = thread::spawn(move || {
            let failure = ThreadEndSignal::armed(worker_failed);
            let result = work(&worker_stop);
            if result.is_ok() {
                failure.disarm();
            }
            result
        });
        Self {
            stop,
            failed,
            handle: Some(handle),
        }
    }

    /// Set once the worker thread ended on an error or a panic. Nothing else
    /// runs the approval and automation passes, so the runtime that owns the
    /// worker stops on it; `stop` returns the error.
    pub(crate) fn failure_signal(&self) -> Arc<AtomicBool> {
        self.failed.clone()
    }

    pub(crate) fn stop(mut self) -> Result<(), CliError> {
        self.stop.store(true, Ordering::SeqCst);
        self.join()
    }

    fn join(&mut self) -> Result<(), CliError> {
        let Some(handle) = self.handle.take() else {
            return Ok(());
        };
        handle
            .join()
            .map_err(|_| CliError::Api(ApiError::internal("surface approval worker panicked")))?
    }
}

impl Drop for SurfaceApprovalWorker {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        let _ = self.join();
    }
}

pub(crate) fn start_surface_approval_worker(
    adapter: Arc<AgentLoopChatCompletionAdapter>,
    data_dir: PathBuf,
    lease_owner_ref: String,
) -> Result<SurfaceApprovalWorker, CliError> {
    let mut dispatcher = DurableWorkDispatcher::open(
        runtime_durable_event_root(&data_dir),
        runtime_durable_work_payload_root(&data_dir),
        MessageBus::new(),
        lease_owner_ref,
        DURABLE_WORK_LEASE_DURATION_MS,
    )
    .map_err(|error| {
        CliError::InvalidArguments(format!(
            "surface approval dispatcher could not start: {}",
            redact_string(&error.to_string())
        ))
    })?;
    Ok(SurfaceApprovalWorker::spawn(move |stop| {
        while !stop.load(Ordering::SeqCst) {
            process_due_surface_approvals(&adapter, &mut dispatcher, &data_dir)?;
            automation_worker::process_due_automation(&mut dispatcher, &data_dir, adapter.as_ref())
                .map_err(CliError::Runtime)?;
            sleep_with_stop(stop, Duration::from_millis(50));
        }
        process_due_surface_approvals(&adapter, &mut dispatcher, &data_dir)?;
        automation_worker::process_due_automation(&mut dispatcher, &data_dir, adapter.as_ref())
            .map_err(CliError::Runtime)
    }))
}

pub(crate) fn process_due_surface_approvals(
    adapter: &AgentLoopChatCompletionAdapter,
    dispatcher: &mut DurableWorkDispatcher,
    data_dir: &Path,
) -> Result<(), CliError> {
    let (state, admission) = durable_work_state_for_owner(data_dir, dispatcher.lease_owner_ref())?;
    if !admission.writable {
        return Err(durable_work_admission_error(&admission));
    }
    for work_id in admission.runnable_work_ids {
        let Some(item) = state.work.items.get(&work_id) else {
            continue;
        };
        if item.work_kind != SURFACE_APPROVAL_WORK_KIND {
            continue;
        }
        dispatcher.lease_work(item, now_millis()).map_err(|error| {
            CliError::InvalidArguments(format!(
                "surface approval work lease failed: {}",
                redact_string(&error.to_string())
            ))
        })?;
        let request = match dispatcher
            .read_payload_json(item)
            .map_err(|error| error.to_string())
            .and_then(|value| {
                SurfaceApprovalRequest::parse(value).map_err(|error| error.to_string())
            }) {
            Ok(request) => request,
            Err(error) => {
                eprintln!(
                    "surface approval work {} failed: {}",
                    item.work_id,
                    redact_string(&error)
                );
                dispatcher
                    .record_terminal(item, WorkTerminalKind::Failed, "failed")
                    .map_err(|error| {
                        CliError::InvalidArguments(format!(
                            "surface approval terminal recording failed: {}",
                            redact_string(&error.to_string())
                        ))
                    })?;
                continue;
            }
        };
        let terminal =
            if !surface_approval_target_owner_is_active(data_dir, &request.target_owner_id)? {
                (WorkTerminalKind::Superseded, "stale_lineage")
            } else {
                match adapter.process_surface_approval_request(&request) {
                    Ok(outcome) => match outcome.kind {
                        SurfaceActionOutcomeKind::Completed => {
                            (WorkTerminalKind::Succeeded, "success")
                        }
                        SurfaceActionOutcomeKind::StaleLineage => {
                            (WorkTerminalKind::Superseded, "stale_lineage")
                        }
                        SurfaceActionOutcomeKind::Unavailable => {
                            (WorkTerminalKind::Blocked, "unavailable")
                        }
                        SurfaceActionOutcomeKind::Requested => {
                            (WorkTerminalKind::Blocked, "requested")
                        }
                    },
                    Err(error) => {
                        eprintln!(
                            "surface approval work {} failed: {}",
                            item.work_id,
                            redact_string(&error.to_string())
                        );
                        (WorkTerminalKind::Failed, "failed")
                    }
                }
            };
        dispatcher
            .record_terminal(item, terminal.0, terminal.1)
            .map_err(|error| {
                CliError::InvalidArguments(format!(
                    "surface approval terminal recording failed: {}",
                    redact_string(&error.to_string())
                ))
            })?;
    }
    Ok(())
}

fn surface_approval_target_owner_is_active(
    data_dir: &Path,
    target_owner_id: &str,
) -> Result<bool, CliError> {
    let marker_path = runtime_ownership_marker_path(data_dir);
    let _mutation_lock = RuntimeOwnershipMutationLock::acquire(&marker_path)?;
    let Some(marker) = read_runtime_ownership_marker(&marker_path)? else {
        return Ok(false);
    };
    if marker.owner_id != target_owner_id {
        return Ok(false);
    }
    Ok(
        classify_runtime_ownership_marker(marker, now_millis()).state
            == RuntimeOwnershipState::Active,
    )
}

impl AgentLoopChatCompletionAdapter {
    fn process_surface_approval_request(
        &self,
        request: &SurfaceApprovalRequest,
    ) -> Result<shacs_core::runtime::SurfaceActionOutcome, ApiError> {
        let sessions = SessionManager::new(&self.workspace).map_err(|error| {
            ApiError::internal(format!("session manager could not be initialized: {error}"))
        })?;
        let bus = MessageBus::new();
        let tools = self.tools.clone();
        // The question came from an interactive run; its approval must meet
        // the same mode (Spec 048 PRD 050 scope 2).
        let config = self.loop_config_for(crate::run_permission_mode::RunKind::Interactive);
        let mut loop_runtime = AgentLoop::new(
            bus,
            sessions,
            self.context_builder(),
            &tools,
            self.client.as_ref(),
            config,
        )
        .with_session_turn_lock(self.session_turn_lock.clone());
        if let Some(message_tool) = &self.message_tool {
            loop_runtime = loop_runtime.with_message_tool_delivery(message_tool.clone());
        }
        loop_runtime
            .process_permission_approval_by_lineage(
                &request.session_key,
                &request.lineage,
                request.approve(),
            )
            .map_err(|error| ApiError::internal(format!("surface approval failed: {error}")))
    }
}

#[cfg(any(test, feature = "spec031-test-fixture"))]
#[doc(hidden)]
#[path = "surface_approval_worker/fixture.rs"]
pub mod spec031_surface_approval_fixture;

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    #[test]
    fn drop_requests_worker_stop_when_explicit_stop_is_skipped() {
        // Given: a worker whose thread has already exited, but whose stop flag is still false.
        let stop = Arc::new(AtomicBool::new(false));
        let observed_stop = stop.clone();
        let worker = SurfaceApprovalWorker {
            stop,
            failed: Arc::new(AtomicBool::new(false)),
            handle: Some(thread::spawn(|| Ok(()))),
        };

        // When: startup or shutdown exits without calling explicit stop.
        drop(worker);

        // Then: Drop requests stop so the owned thread is not left detached.
        assert!(observed_stop.load(Ordering::SeqCst));
    }

    #[test]
    fn failed_pass_raises_failure_signal_and_stop_returns_error() {
        // Given: a worker whose pass fails, and one that runs until it is stopped.
        let failing =
            SurfaceApprovalWorker::spawn(|_| Err(CliError::Runtime("pass failed".to_owned())));
        let running = SurfaceApprovalWorker::spawn(|stop| {
            while !stop.load(Ordering::SeqCst) {
                thread::yield_now();
            }
            Ok(())
        });
        let (failed, healthy) = (failing.failure_signal(), running.failure_signal());

        // When: both are stopped, which waits for their threads.
        let failure = failing.stop();
        let success = running.stop();

        // Then: only the failed worker raised the signal the gateway watches, and reports why.
        assert!(failed.load(Ordering::SeqCst));
        assert!(!healthy.load(Ordering::SeqCst));
        assert!(matches!(failure, Err(CliError::Runtime(reason)) if reason == "pass failed"));
        assert!(success.is_ok());
    }

    #[test]
    fn panicked_pass_raises_failure_signal_before_stop() {
        // Given: a worker whose pass panics.
        let panicking = SurfaceApprovalWorker::spawn(|_| panic!("pass panicked"));
        let failed = panicking.failure_signal();

        // When: nothing stops or joins it.
        let deadline = Instant::now() + Duration::from_secs(20);
        while !failed.load(Ordering::SeqCst) && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(5));
        }

        // Then: the signal the gateway watches is up, and stopping reports the panic.
        assert!(failed.load(Ordering::SeqCst));
        let reason = panicking
            .stop()
            .expect_err("a panicked worker stopped cleanly")
            .to_string();
        assert!(
            reason.contains("surface approval worker panicked"),
            "{reason}"
        );
    }
}
