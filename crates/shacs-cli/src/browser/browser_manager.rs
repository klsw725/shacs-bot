use super::{
    browser_action_binding, browser_state, browser_viewer, BrowserCancellation,
    BrowserLifecycleFactory, BrowserOwnerBinding, BrowserProcessRunner, BrowserProcesses,
    BrowserSnapshotDestination, BrowserStartup, BrowserState, BrowserStateSnapshot, BrowserStatus,
    BrowserViewState, BrowserViewStatus, BrowserViewerFactory, CdpDocument, ConfigBrowserStartup,
    LinuxBrowserProcessRunner,
};
use serde_json::Value;
use shacs_config::ConfigBundle;
use shacs_core::runtime::{
    BrowserAction, BrowserCallContext, BrowserDocument, BrowserError, BrowserHost,
    BrowserPreparedAction, BrowserRead, BrowserSession, ProcessExecutionReceipt, ProcessGate,
    ProcessSpawnReport, ProcessTerminalOutcome,
};
use std::sync::{Arc, Mutex, OnceLock, TryLockError};
use std::thread;
use std::time::{Duration, Instant};

pub trait BrowserClock: Send + Sync {
    fn now_ms(&self) -> u64;
}
pub struct SystemBrowserClock;
impl BrowserClock for SystemBrowserClock {
    fn now_ms(&self) -> u64 {
        crate::now_millis()
    }
}

struct Generation {
    processes: BrowserProcesses,
    document: CdpDocument,
    session: String,
    last_used: u64,
}

struct Slot {
    generation: Option<Generation>,
    number: u64,
    receipt: Option<ProcessExecutionReceipt>,
    owner: Option<BrowserOwnerBinding>,
}

pub struct BrowserManager {
    slot: Mutex<Slot>,
    status: Mutex<BrowserStatus>,
    startup: Box<dyn BrowserStartup>,
    runner: Box<dyn BrowserProcessRunner>,
    clock: Arc<dyn BrowserClock>,
    snapshot: Option<BrowserSnapshotDestination>,
    viewer: Option<Box<dyn BrowserLifecycleFactory>>,
    cancellation: BrowserCancellation,
}

#[derive(Clone)]
pub struct ManagedBrowserHost {
    manager: Arc<BrowserManager>,
}

impl ManagedBrowserHost {
    pub fn new(
        startup: Box<dyn BrowserStartup>,
        runner: Box<dyn BrowserProcessRunner>,
        clock: Arc<dyn BrowserClock>,
        snapshot: Option<BrowserSnapshotDestination>,
        viewer: Option<Box<dyn BrowserLifecycleFactory>>,
    ) -> Self {
        let view = viewer
            .as_ref()
            .map_or_else(BrowserViewStatus::disabled, |factory| {
                factory.view(runner.as_ref())
            });
        let status =
            BrowserStatus::new(startup.config().enable, runner.available(), clock.now_ms())
                .with_view(view);
        let manager = Arc::new(BrowserManager {
            slot: Mutex::new(Slot {
                generation: None,
                number: 0,
                receipt: None,
                owner: None,
            }),
            status: Mutex::new(status),
            startup,
            runner,
            clock,
            snapshot,
            viewer,
            cancellation: BrowserCancellation::default(),
        });
        let weak = Arc::downgrade(&manager);
        thread::spawn(move || loop {
            thread::sleep(Duration::from_millis(100));
            let Some(manager) = weak.upgrade() else {
                return;
            };
            let _ = manager.maintenance();
        });
        Self { manager }
    }

    pub fn status(&self) -> Result<BrowserStatus, BrowserError> {
        self.manager
            .status
            .lock()
            .map(|status| status.clone())
            .map_err(|_| BrowserError::StateUnavailable)
    }

    pub fn receipt(&self) -> Result<Option<ProcessExecutionReceipt>, BrowserError> {
        self.manager
            .slot
            .lock()
            .map(|slot| slot.receipt.clone())
            .map_err(|_| BrowserError::StateUnavailable)
    }

    pub fn maintenance(&self) -> Result<(), BrowserError> {
        self.manager.maintenance()
    }

    pub fn shutdown(&self) -> Result<(), BrowserError> {
        // Signal before taking the slot: an in-flight pipe write/read owns it.
        // The transport observes cancellation within its bounded poll interval.
        self.manager.cancellation.cancel();
        let mut slot = self
            .manager
            .slot
            .lock()
            .map_err(|_| BrowserError::StateUnavailable)?;
        slot.generation.take();
        self.manager.publish(&slot, BrowserState::Idle, None)
    }
}

impl BrowserManager {
    fn publish(
        &self,
        slot: &Slot,
        state: BrowserState,
        reason: Option<String>,
    ) -> Result<(), BrowserError> {
        let mut status = self
            .status
            .lock()
            .map_err(|_| BrowserError::StateUnavailable)?;
        status.state = state;
        status.reason = reason;
        status.updated_at_ms = self.clock.now_ms();
        status.owner_observed = slot.owner.is_some();
        status.view = self.view(slot, state);
        if let (Some(destination), Some(owner)) = (&self.snapshot, &slot.owner) {
            browser_state::write_browser_snapshot(
                &destination.data_dir,
                &BrowserStateSnapshot {
                    owner: owner.clone(),
                    generation: slot.number,
                    holder_present: slot.generation.is_some() || state == BrowserState::Starting,
                    status: status.clone(),
                },
            )
            .map_err(|_| BrowserError::StateUnavailable)?;
        }
        Ok(())
    }

    /// The viewer facts of the live generation, or of the configuration when
    /// none runs. The viewer starts inside the same startup closure as the
    /// browser, so a usable viewer is `starting` while the browser is.
    fn view(&self, slot: &Slot, state: BrowserState) -> BrowserViewStatus {
        let live = slot
            .generation
            .as_ref()
            .and_then(|generation| generation.processes.view());
        let mut view = live.unwrap_or_else(|| {
            self.viewer
                .as_ref()
                .map_or_else(BrowserViewStatus::disabled, |factory| {
                    factory.view(self.runner.as_ref())
                })
        });
        if state == BrowserState::Starting
            && slot.generation.is_none()
            && view.state == BrowserViewState::Closed
        {
            view.state = BrowserViewState::Starting;
        }
        view
    }

    fn busy(&self) -> BrowserError {
        if let Ok(mut status) = self.status.lock() {
            status.state = BrowserState::Busy;
            status.reason = Some("browser_busy".to_owned());
            status.updated_at_ms = self.clock.now_ms();
        }
        BrowserError::Busy
    }

    fn maintenance(&self) -> Result<(), BrowserError> {
        let mut slot = match self.slot.try_lock() {
            Ok(slot) => slot,
            Err(TryLockError::WouldBlock) => return Ok(()),
            Err(TryLockError::Poisoned(_)) => return Err(BrowserError::StateUnavailable),
        };
        let config = self.startup.config();
        let Some(generation) = slot.generation.as_mut() else {
            return Ok(());
        };
        let failure = if self.clock.now_ms().saturating_sub(generation.last_used)
            >= config.idle_timeout_seconds * 1000
        {
            Some(BrowserError::IdleTimeout)
        } else {
            match generation.processes.alive() {
                Ok(true) => generation
                    .document
                    .poll(Instant::now() + Duration::from_secs(1))
                    .err(),
                Ok(false) => Some(BrowserError::ProcessExited),
                Err(error) => Some(error),
            }
        };
        if let Some(error) = failure {
            slot.generation.take();
            self.publish(&slot, BrowserState::Failed, Some(error.to_string()))?;
        } else if generation.processes.viewer_failed {
            self.publish_or_dispose(
                &mut slot,
                BrowserState::Running,
                Some("viewer_process_exited".to_owned()),
            )?;
        }
        Ok(())
    }

    fn with_generation<T>(
        &self,
        owner: &str,
        context: &BrowserCallContext,
        operation: impl FnOnce(&mut CdpDocument, Instant) -> Result<T, BrowserError>,
    ) -> Result<T, BrowserError> {
        if context.effective_session_id != owner {
            return Err(BrowserError::ContextUnavailable);
        }
        self.cancellation.check()?;
        let mut slot = match self.slot.try_lock() {
            Ok(slot) => slot,
            Err(TryLockError::WouldBlock) => return Err(self.busy()),
            Err(TryLockError::Poisoned(_)) => return Err(BrowserError::StateUnavailable),
        };
        let generation = slot
            .generation
            .as_mut()
            .ok_or(BrowserError::ProcessExited)?;
        if generation.session != owner {
            return Err(self.busy());
        }
        generation.last_used = self.clock.now_ms();
        let start = generation.last_used;
        let timeout = self.startup.config().page_timeout_seconds;
        let result = match generation.processes.alive() {
            Ok(true) => operation(
                &mut generation.document,
                Instant::now() + Duration::from_secs(timeout),
            ),
            Ok(false) => Err(BrowserError::ProcessExited),
            Err(error) => Err(error),
        };
        generation.last_used = self.clock.now_ms();
        let viewer_failed = generation.processes.viewer_failed;
        let result = if generation.last_used.saturating_sub(start) >= timeout * 1000 {
            Err(BrowserError::PageTimeout)
        } else {
            result
        };
        let result = self.cancellation.check().and(result);
        let result = self.dispose_on_fatal(&mut slot, result)?;
        self.publish_or_dispose(
            &mut slot,
            BrowserState::Running,
            viewer_failed.then(|| "viewer_process_exited".to_owned()),
        )?;
        Ok(result)
    }

    /// Record a state. A browser whose state cannot be recorded is not kept:
    /// the generation ends and the in-memory status (which the file write
    /// cannot undo) says why.
    fn publish_or_dispose(
        &self,
        slot: &mut Slot,
        state: BrowserState,
        reason: Option<String>,
    ) -> Result<(), BrowserError> {
        if let Err(error) = self.publish(slot, state, reason) {
            slot.generation.take();
            let _ = self.publish(slot, BrowserState::Failed, Some(error.to_string()));
            return Err(error);
        }
        Ok(())
    }

    /// The one outcome rule for every operation on a generation: a stale
    /// approval, a covered target or a refused input keep the browser; any
    /// other error ends the generation at once (processes, proxy, profile)
    /// and records the failure.
    fn dispose_on_fatal<T>(
        &self,
        slot: &mut Slot,
        result: Result<T, BrowserError>,
    ) -> Result<T, BrowserError> {
        if let Err(error) = &result {
            if super::recoverable(error) {
                self.publish_or_dispose(slot, BrowserState::Running, None)?;
            } else {
                // The generation is gone either way; the original error is the
                // one worth reporting even when its record cannot be written.
                slot.generation.take();
                let _ = self.publish(slot, BrowserState::Failed, Some(error.to_string()));
            }
        }
        result
    }
}

impl BrowserHost for ManagedBrowserHost {
    fn prepare(
        &self,
        session_id: &str,
        arguments: &Value,
    ) -> Result<BrowserPreparedAction, BrowserError> {
        if session_id.is_empty() {
            return Err(BrowserError::ContextUnavailable);
        }
        let slot = self.manager.slot.try_lock().map_err(|error| match error {
            TryLockError::WouldBlock => BrowserError::Busy,
            TryLockError::Poisoned(_) => BrowserError::StateUnavailable,
        })?;
        match &slot.generation {
            Some(generation) if generation.session != session_id => Err(BrowserError::Busy),
            Some(generation) => generation.document.prepare(arguments),
            None => browser_action_binding::empty(arguments),
        }
    }

    fn close_prepared(
        &self,
        context: &BrowserCallContext,
        digest: &str,
    ) -> Result<(), BrowserError> {
        let arguments = serde_json::json!({"action":"close"});
        let mut slot = self.manager.slot.try_lock().map_err(|error| match error {
            TryLockError::WouldBlock => BrowserError::Busy,
            TryLockError::Poisoned(_) => BrowserError::StateUnavailable,
        })?;
        match slot.generation.as_mut() {
            Some(generation) if generation.session != context.effective_session_id => {
                return Err(BrowserError::Busy)
            }
            Some(generation) => {
                let deadline = Instant::now()
                    + Duration::from_secs(self.manager.startup.config().page_timeout_seconds);
                let result = generation
                    .document
                    .execute_prepared(&arguments, digest, false, deadline)
                    .map(|_| ());
                // A fatal error while closing disposes of the generation like
                // any other action would; a stale approval keeps it.
                self.manager.dispose_on_fatal(&mut slot, result)?;
            }
            None => {
                if browser_action_binding::empty(&arguments)?.digest != digest {
                    return Err(BrowserError::StaleRevision);
                }
            }
        }
        slot.generation.take();
        self.manager.publish(&slot, BrowserState::Idle, None)
    }

    fn acquire(
        &self,
        context: &BrowserCallContext,
    ) -> Result<Box<dyn BrowserSession>, BrowserError> {
        if context.effective_session_id.is_empty() {
            return Err(BrowserError::ContextUnavailable);
        }
        self.manager.cancellation.check()?;
        self.manager.maintenance()?;
        let mut slot = match self.manager.slot.try_lock() {
            Ok(slot) => slot,
            Err(TryLockError::WouldBlock) => return Err(self.manager.busy()),
            Err(TryLockError::Poisoned(_)) => return Err(BrowserError::StateUnavailable),
        };
        self.manager.cancellation.check()?;
        if let Some(generation) = slot.generation.as_ref() {
            if generation.session != context.effective_session_id {
                self.manager.publish_or_dispose(
                    &mut slot,
                    BrowserState::Busy,
                    Some("browser_busy".to_owned()),
                )?;
                return Err(BrowserError::Busy);
            }
        } else {
            let config = self.manager.startup.config();
            if !config.enable {
                return Err(BrowserError::Disabled);
            }
            if !self.manager.runner.available() {
                return Err(BrowserError::Unavailable);
            }
            slot.number += 1;
            slot.owner = self.manager.snapshot.as_ref().and_then(|destination| {
                destination
                    .owner
                    .clone()
                    .or_else(|| browser_state::current_owner(&destination.data_dir))
                    .filter(|owner| {
                        std::path::Path::new(&owner.config_path) == destination.config_path
                            && std::path::Path::new(&owner.workspace) == destination.workspace
                    })
            });
            let input = match self.manager.startup.gate(slot.number) {
                Ok(input) => input,
                Err(error) => {
                    self.manager
                        .publish(&slot, BrowserState::Failed, Some(error.to_string()))?;
                    return Err(error);
                }
            };
            let expected = input.envelope.clone();
            self.manager.publish(&slot, BrowserState::Starting, None)?;
            let started_at = self.manager.clock.now_ms();
            let deadline = Instant::now() + Duration::from_secs(config.startup_timeout_seconds);
            let mut result = Err(BrowserError::StartDenied);
            // When the browser core (Xvfb, Chromium, DevTools) finished. The
            // optional viewer starts after this inside the same approved
            // closure with its own budget; its time is not browser time.
            let mut core_finished_ms = None;
            let receipt = ProcessGate::new()
                .evaluate_and_maybe_spawn(input, |_| {
                    result = BrowserProcesses::start(
                        self.manager.runner.as_ref(),
                        self.manager.startup.network_guard(),
                        deadline,
                    )
                    .and_then(|(mut processes, mut peer)| {
                        peer.set_cancellation(self.manager.cancellation.clone());
                        let document =
                            CdpDocument::initialize(peer, deadline).map_err(|error| {
                                if error == BrowserError::PageTimeout {
                                    BrowserError::StartupTimeout
                                } else {
                                    error
                                }
                            })?;
                        core_finished_ms = Some(self.manager.clock.now_ms());
                        let viewer = self.manager.viewer.as_ref().map(|factory| factory.create());
                        processes.start_viewer(
                            self.manager.runner.as_ref(),
                            viewer,
                            Instant::now() + browser_viewer::VIEWER_START_LIMIT,
                        );
                        Ok(Generation {
                            processes,
                            document,
                            session: context.effective_session_id.clone(),
                            last_used: self.manager.clock.now_ms(),
                        })
                    });
                    ProcessSpawnReport::terminal(if result.is_ok() {
                        ProcessTerminalOutcome::Succeeded
                    } else {
                        ProcessTerminalOutcome::Failed
                    })
                })
                .map_err(|_| BrowserError::StartDenied)?;
            let valid = receipt.adapter == expected.adapter
                && receipt.identity == expected.identity
                && receipt.policy_safety_snapshot_ref == expected.policy_safety_snapshot_ref
                && receipt.dispatch_count == 1
                && receipt.terminal_outcome == ProcessTerminalOutcome::Succeeded;
            slot.receipt = Some(receipt);
            let core_elapsed_ms = core_finished_ms
                .unwrap_or_else(|| self.manager.clock.now_ms())
                .saturating_sub(started_at);
            if core_elapsed_ms >= config.startup_timeout_seconds * 1000 {
                result = Err(BrowserError::StartupTimeout);
            }
            result = self.manager.cancellation.check().and(result);
            match result {
                Ok(generation) if valid => slot.generation = Some(generation),
                Ok(_) => return Err(BrowserError::StartDenied),
                Err(error) => {
                    self.manager
                        .publish(&slot, BrowserState::Failed, Some(error.to_string()))?;
                    return Err(error);
                }
            }
            self.manager
                .publish_or_dispose(&mut slot, BrowserState::Running, None)?;
        }
        if let Some(generation) = slot.generation.as_mut() {
            generation.last_used = self.manager.clock.now_ms();
        }
        Ok(Box::new(ManagedBrowserSession {
            manager: Arc::clone(&self.manager),
            owner: context.effective_session_id.clone(),
        }))
    }
}

struct ManagedBrowserSession {
    manager: Arc<BrowserManager>,
    owner: String,
}
impl BrowserSession for ManagedBrowserSession {
    fn execute_prepared(
        &self,
        context: &BrowserCallContext,
        arguments: &Value,
        digest: &str,
    ) -> Result<Option<BrowserRead>, BrowserError> {
        let capture = context
            .browser_screenshots
            .unwrap_or(context.native_image_input_supported);
        self.manager
            .with_generation(&self.owner, context, |document, deadline| {
                document.execute_prepared(arguments, digest, capture, deadline)
            })
    }

    fn current_document(
        &self,
        context: &BrowserCallContext,
    ) -> Result<BrowserDocument, BrowserError> {
        self.manager
            .with_generation(&self.owner, context, CdpDocument::current)
    }
    fn read(&self, context: &BrowserCallContext) -> Result<BrowserRead, BrowserError> {
        let capture = context
            .browser_screenshots
            .unwrap_or(context.native_image_input_supported);
        self.manager
            .with_generation(&self.owner, context, |document, deadline| {
                document.read(capture, deadline)
            })
    }
    fn act(
        &self,
        context: &BrowserCallContext,
        action: &BrowserAction,
    ) -> Result<BrowserRead, BrowserError> {
        let capture = context
            .browser_screenshots
            .unwrap_or(context.native_image_input_supported);
        self.manager
            .with_generation(&self.owner, context, |document, deadline| {
                document.act(action, deadline)?;
                document.read(capture, deadline)
            })
    }
    fn close(&self, context: &BrowserCallContext) -> Result<(), BrowserError> {
        if context.effective_session_id != self.owner {
            return Err(BrowserError::ContextUnavailable);
        }
        let mut slot = self.manager.slot.try_lock().map_err(|error| match error {
            TryLockError::WouldBlock => self.manager.busy(),
            TryLockError::Poisoned(_) => BrowserError::StateUnavailable,
        })?;
        if slot
            .generation
            .as_ref()
            .is_some_and(|generation| generation.session != self.owner)
        {
            return Err(self.manager.busy());
        }
        slot.generation.take();
        self.manager.publish(&slot, BrowserState::Idle, None)
    }
}

static BROWSER: OnceLock<ManagedBrowserHost> = OnceLock::new();

pub fn process_browser_host(bundle: &ConfigBundle) -> ManagedBrowserHost {
    BROWSER
        .get_or_init(|| {
            let snapshot = Some(BrowserSnapshotDestination {
                data_dir: bundle.context.data_dir.clone(),
                owner: None,
                config_path: bundle.context.config_path.clone(),
                workspace: bundle.context.workspace.clone(),
            });
            let owner_mode =
                browser_state::current_marker(&bundle.context.data_dir).map(|marker| marker.mode);
            let viewer = BrowserViewerFactory::new(
                bundle.config.tools.browser.view.clone(),
                browser_viewer::published_viewer_port(owner_mode.as_deref()),
            );
            ManagedBrowserHost::new(
                Box::new(ConfigBrowserStartup::new(bundle.clone())),
                Box::new(LinuxBrowserProcessRunner),
                Arc::new(SystemBrowserClock),
                snapshot,
                Some(Box::new(viewer)),
            )
        })
        .clone()
}

pub fn shutdown_process_browser() {
    if let Some(host) = BROWSER.get() {
        let _ = host.shutdown();
    }
}
