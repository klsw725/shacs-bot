use super::*;

pub(crate) struct SurfaceApprovalWorker {
    stop: Arc<AtomicBool>,
    handle: Option<thread::JoinHandle<Result<(), CliError>>>,
}

impl SurfaceApprovalWorker {
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
    let stop = Arc::new(AtomicBool::new(false));
    let worker_stop = stop.clone();
    let handle = thread::spawn(move || {
        while !worker_stop.load(Ordering::SeqCst) {
            process_due_surface_approvals(&adapter, &mut dispatcher, &data_dir)?;
            automation_worker::process_due_automation(&mut dispatcher, &data_dir, adapter.as_ref())
                .map_err(CliError::Runtime)?;
            sleep_with_stop(&worker_stop, Duration::from_millis(50));
        }
        process_due_surface_approvals(&adapter, &mut dispatcher, &data_dir)?;
        automation_worker::process_due_automation(&mut dispatcher, &data_dir, adapter.as_ref())
            .map_err(CliError::Runtime)
    });
    Ok(SurfaceApprovalWorker {
        stop,
        handle: Some(handle),
    })
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
    for work_id in admission.due_work_ids {
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
        let mut loop_runtime = AgentLoop::new(
            bus,
            sessions,
            self.context_builder(),
            &tools,
            self.client.as_ref(),
            self.loop_config(),
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

    #[test]
    fn drop_requests_worker_stop_when_explicit_stop_is_skipped() {
        // Given: a worker whose thread has already exited, but whose stop flag is still false.
        let stop = Arc::new(AtomicBool::new(false));
        let observed_stop = stop.clone();
        let worker = SurfaceApprovalWorker {
            stop,
            handle: Some(thread::spawn(|| Ok(()))),
        };

        // When: startup or shutdown exits without calling explicit stop.
        drop(worker);

        // Then: Drop requests stop so the owned thread is not left detached.
        assert!(observed_stop.load(Ordering::SeqCst));
    }
}
