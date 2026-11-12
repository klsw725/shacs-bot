use crate::automation_worker::{self, AutomationOwnerAdapters};
use crate::run_permission_mode::RunKind;
use crate::tests::scripted_agent_loop_adapter;
use crate::{
    automation_producer, durable_work_state_for_owner,
    external_transport_channel_manager_with_runner, now_millis, publish_external_inbound_with_hint,
    requeue_owned_external_work, run_external_agent_processor_with_channels,
    runtime_durable_event_root, runtime_durable_work_payload_root, runtime_owner_id,
    wait_for_ctrl_c_or_runtime_request, AgentLoopChatCompletionAdapter, CliError,
    DurableWorkDispatcher, ExternalChannelSupervisor, ExternalProcessorShutdownControl,
    ExternalSupervisorShutdownReport, ExternalTransportRunner, ExternalTransportRuntimeContext,
    ExternalTransportSpec, GatewayDurableWork, RuntimeOwnerFence, RuntimeShutdownReason,
    TelegramRuntimeConfig, DURABLE_WORK_LEASE_DURATION_MS, TELEGRAM_CHANNEL,
};
use serde_json::{json, Map, Value};
use shacs_channels::OutboundMessage;
use shacs_config::PermissionMode;
use shacs_core::runtime::{
    AutomationDispatchRequest, AutomationExecutionControl, AutomationExecutionReceipt,
    AutomationHookEvaluation, BrowserCallContext, BrowserCapture, BrowserCloseOutcome,
    BrowserControlDescription, BrowserDescription, BrowserDescriptionState, BrowserDocument,
    BrowserError, BrowserHost, BrowserPreparedAction, BrowserSession, ContainerNetworkMode,
    ContainerRuntimeKind, ContainmentSnapshotRef, DockerContainmentSnapshot, ExecutionSnapshot,
    InboundMessage, MessageBus, SessionTurnReservation, AUTOMATION_WORK_KIND,
};
use shacs_core::tools::{BrowserTool, MessageTool, ToolRegistry};
use shacs_cron::{CronJob, CronJobState, CronPayload, CronSchedule};
use shacs_providers::{
    LlmResponse, ProviderClient, ProviderError, ProviderEvent, ProviderRequest, ToolCallRequest,
};
use shacs_session::durable_work::{
    ReplayWorkItem, ReplayWorkState, WorkTerminalKind, MAX_PROJECTED_WORK_IDS,
};
use std::collections::BTreeMap;
use std::error::Error;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};
use std::{fs, thread};

#[path = "spec050_browser_ingress_tests/stage.rs"]
mod stage;
#[path = "spec050_browser_ingress_tests/takeover.rs"]
mod takeover;

type TestResult = Result<(), Box<dyn Error>>;
type Script = Box<dyn Fn(usize, &ProviderRequest) -> LlmResponse + Send + Sync>;
type Processor = thread::JoinHandle<Result<ExternalSupervisorShutdownReport, CliError>>;

const CHAT: &str = "chat";
const SESSION: &str = "telegram:chat";
const PATIENCE: Duration = Duration::from_secs(20);
const HOLD_LIMIT: Duration = Duration::from_secs(60);
const SCREEN: &[u8] = b"\x89PNG\r\n\x1a\nscreen now";
const DIGEST: &str = "capture-digest";
const STATUS_REPLY: &str = "브라우저: 열림\n쓰는 세션: telegram:chat\n답 기다리기: 아님\n조작: 어시스턴트\n넘겨받은 곳: 없음\n화면 보기: 꺼짐(disabled)";
const CLOSE_REPLY: &str = "브라우저를 닫았습니다.";
const STOP_REPLY: &str = "Stop requested. Cancellation requested for the active session turn. Provider/tool execution will stop only if it observes the cancellation token.";

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().expect("test lock poisoned")
}

fn answer(text: &str) -> LlmResponse {
    LlmResponse {
        content: Some(text.to_owned()),
        finish_reason: "stop".to_owned(),
        ..LlmResponse::default()
    }
}

fn call(id: &str, name: &str, arguments: Value) -> LlmResponse {
    LlmResponse {
        finish_reason: "tool_calls".to_owned(),
        tool_calls: vec![ToolCallRequest::new(
            id,
            name,
            arguments.as_object().cloned().unwrap_or_else(Map::new),
        )],
        ..LlmResponse::default()
    }
}

fn eventually(what: &str, mut done: impl FnMut() -> bool) -> TestResult {
    let deadline = Instant::now() + PATIENCE;
    while !done() {
        if Instant::now() >= deadline {
            return Err(format!("timed out waiting for {what}").into());
        }
        thread::sleep(Duration::from_millis(10));
    }
    Ok(())
}

struct Provider {
    script: Script,
    requests: Mutex<Vec<ProviderRequest>>,
    held: Mutex<bool>,
    released: Condvar,
}

impl Provider {
    fn new(
        script: impl Fn(usize, &ProviderRequest) -> LlmResponse + Send + Sync + 'static,
    ) -> Arc<Self> {
        Arc::new(Self {
            script: Box::new(script),
            requests: Mutex::new(Vec::new()),
            held: Mutex::new(false),
            released: Condvar::new(),
        })
    }

    fn requests(&self) -> Vec<ProviderRequest> {
        lock(&self.requests).clone()
    }

    fn hold(&self, held: bool) {
        *lock(&self.held) = held;
        self.released.notify_all();
    }
}

impl ProviderClient for Provider {
    fn chat(&self, request: ProviderRequest) -> Result<LlmResponse, ProviderError> {
        let index = {
            let mut requests = lock(&self.requests);
            requests.push(request.clone());
            requests.len() - 1
        };
        drop(
            self.released
                .wait_timeout_while(lock(&self.held), HOLD_LIMIT, |held| *held)
                .expect("test lock poisoned"),
        );
        Ok((self.script)(index, &request))
    }

    fn chat_stream(
        &self,
        request: ProviderRequest,
        _on_event: &mut dyn FnMut(ProviderEvent),
    ) -> Result<LlmResponse, ProviderError> {
        self.chat(request)
    }
}

struct Host {
    owner: Mutex<Option<String>>,
}

impl Host {
    fn open() -> Arc<Self> {
        Arc::new(Self {
            owner: Mutex::new(Some(SESSION.to_owned())),
        })
    }
}

impl BrowserHost for Host {
    fn acquire(
        &self,
        _context: &BrowserCallContext,
    ) -> Result<Box<dyn BrowserSession>, BrowserError> {
        Err(BrowserError::NotOpen)
    }

    fn prepare(
        &self,
        _session_id: &str,
        arguments: &Value,
    ) -> Result<BrowserPreparedAction, BrowserError> {
        Ok(BrowserPreparedAction {
            description: json!({"action":arguments["action"],"origin":"https://shop.example",
                "role":null,"name":null,"revision":"r1","frame":"frame","loader":"loader",
                "form":null,"focus":null,"text":null,"submit":null,"key":null}),
            digest: DIGEST.to_owned(),
        })
    }

    fn capture_prepared(
        &self,
        _context: &BrowserCallContext,
        digest: &str,
    ) -> Result<BrowserCapture, BrowserError> {
        if digest != DIGEST {
            return Err(BrowserError::StaleRevision);
        }
        Ok(BrowserCapture {
            document: BrowserDocument {
                target_id: "page".to_owned(),
                session_id: "cdp".to_owned(),
                frame_id: "frame".to_owned(),
                loader_id: "loader".to_owned(),
                url: "https://shop.example/cart".to_owned(),
            },
            png: SCREEN.to_vec(),
        })
    }

    fn close_by_user(&self) -> BrowserCloseOutcome {
        match lock(&self.owner).take() {
            Some(_) => BrowserCloseOutcome::Closed,
            None => BrowserCloseOutcome::NothingOpen,
        }
    }

    fn describe(&self) -> BrowserDescription {
        let session = lock(&self.owner).clone();
        BrowserDescription {
            state: if session.is_some() {
                BrowserDescriptionState::Open
            } else {
                BrowserDescriptionState::Closed
            },
            session,
            reply_wait_remaining_minutes: None,
            control: BrowserControlDescription::default(),
        }
    }
}

fn adapter(
    root: &Path,
    mode: PermissionMode,
    tools: ToolRegistry,
    provider: &Arc<Provider>,
    host: &Arc<Host>,
) -> Result<AgentLoopChatCompletionAdapter, Box<dyn Error>> {
    let mut adapter = scripted_agent_loop_adapter(
        root,
        json!({"permissions":{"mode":mode}}),
        tools,
        Vec::new(),
        mode,
    )?;
    adapter.client = provider.clone();
    adapter.browser_host = Some(host.clone());
    adapter.native_image_input_supported = false;
    adapter.send_progress = false;
    adapter.defaults.max_tool_iterations = 4;
    Ok(adapter)
}

/// A cron run that is already due for the chat's own session.
fn cron_job(id: &str, instruction: &str) -> CronJob {
    CronJob {
        id: id.to_owned(),
        name: id.to_owned(),
        enabled: true,
        schedule: CronSchedule::at(0),
        payload: CronPayload {
            message: instruction.to_owned(),
            session_key: Some(SESSION.to_owned()),
            ..CronPayload::default()
        },
        state: CronJobState {
            next_run_at_ms: Some(1),
            ..CronJobState::default()
        },
        created_at_ms: 0,
        updated_at_ms: 0,
        delete_after_run: true,
    }
}

struct Ingress {
    root: tempfile::TempDir,
    adapter: Arc<AgentLoopChatCompletionAdapter>,
    provider: Arc<Provider>,
    bus: MessageBus,
    context: ExternalTransportRuntimeContext,
    delivered: mpsc::Receiver<OutboundMessage>,
    heard: Vec<String>,
    stop: Arc<AtomicBool>,
    exited: Arc<AtomicBool>,
    processor: Option<Processor>,
    gateway: GatewayDurableWork,
}

impl Ingress {
    /// The external processor over the durable work the gateway opens for it, on the thread the
    /// supervisor gives it. The gateway's worker comes from the same `gateway`, as `run_gateway`
    /// starts it.
    fn start(provider: Arc<Provider>, host: &Arc<Host>) -> Result<Self, Box<dyn Error>> {
        let root = tempfile::tempdir()?;
        let adapter = Arc::new(adapter(
            root.path(),
            PermissionMode::Default,
            ToolRegistry::new(),
            &provider,
            host,
        )?);
        let bus = MessageBus::new();
        let mut context = ExternalTransportRuntimeContext::new(root.path().join("metadata"), 1);
        let gateway = GatewayDurableWork::new(
            root.path().to_path_buf(),
            runtime_owner_id(std::process::id(), now_millis()),
        );
        let durable_work = gateway.open_ingress(&mut context, &bus, None)?;
        let (delivered_tx, delivered) = mpsc::channel();
        let runner: ExternalTransportRunner = Arc::new(move |_, _, outbound, stop, _| {
            while !stop.load(Ordering::SeqCst) {
                if let Ok(message) = outbound.recv_timeout(Duration::from_millis(10)) {
                    let _ = delivered_tx.send(message);
                }
            }
        });
        let stop = Arc::new(AtomicBool::new(false));
        let exited = Arc::new(AtomicBool::new(false));
        let processor = ExternalChannelSupervisor::spawn_processor(exited.clone(), {
            let (adapter, bus, context, stop, exited) = (
                adapter.clone(),
                bus.clone(),
                context.clone(),
                stop.clone(),
                exited.clone(),
            );
            move || {
                let spec = ExternalTransportSpec::Telegram(TelegramRuntimeConfig {
                    token: "telegram-token".to_owned(),
                    poll_timeout_seconds: 1,
                    poll_limit: 1,
                });
                let hello = context.spec035_hello;
                let channels = external_transport_channel_manager_with_runner(
                    vec![spec],
                    bus.clone(),
                    1,
                    context,
                    runner,
                );
                run_external_agent_processor_with_channels(
                    adapter,
                    bus,
                    channels,
                    hello,
                    ExternalProcessorShutdownControl {
                        stop,
                        reason: Arc::new(Mutex::new(RuntimeShutdownReason::Stop)),
                        exited,
                        startup_tx: None,
                    },
                    durable_work,
                )
            }
        });
        Ok(Self {
            root,
            adapter,
            provider,
            bus,
            context,
            delivered,
            heard: Vec::new(),
            stop,
            exited,
            processor: Some(processor),
            gateway,
        })
    }

    fn say(&self, text: &str) {
        assert!(
            publish_external_inbound_with_hint(
                &self.bus,
                InboundMessage::new(TELEGRAM_CHANNEL, "user", CHAT, text),
                &self.context.metadata_path("telegram"),
                &self.context,
            ),
            "intake refused {text:?}"
        );
    }

    fn hear(&mut self, text: &str) -> TestResult {
        let deadline = Instant::now() + PATIENCE;
        while !self.heard.iter().any(|heard| heard == text) {
            let left = deadline.saturating_duration_since(Instant::now());
            let message = self
                .delivered
                .recv_timeout(left)
                .map_err(|_| format!("no reply {text:?}; heard {:?}", self.heard))?;
            assert_eq!(
                (message.channel.as_str(), message.chat_id.as_str()),
                (TELEGRAM_CHANNEL, CHAT)
            );
            self.heard.push(message.content);
        }
        Ok(())
    }

    fn requests(&self) -> usize {
        self.provider.requests().len()
    }

    fn session_files(&self) -> Result<BTreeMap<PathBuf, Vec<u8>>, Box<dyn Error>> {
        let mut files = BTreeMap::new();
        let mut folders = vec![self.root.path().join("workspace").join("sessions")];
        while let Some(folder) = folders.pop() {
            let Ok(entries) = fs::read_dir(&folder) else {
                continue;
            };
            for entry in entries {
                let path = entry?.path();
                if path.is_dir() {
                    folders.push(path);
                } else {
                    files.insert(path.clone(), fs::read(&path)?);
                }
            }
        }
        Ok(files)
    }

    fn turn_state(&self) -> (Vec<String>, Vec<String>, Option<bool>) {
        let turns = &self.adapter.session_turn_lock;
        (
            turns.busy_session_keys(),
            turns.active_session_keys(),
            turns
                .cancellation_token(SESSION)
                .map(|token| token.is_cancelled()),
        )
    }

    fn work(&self) -> Result<Vec<(bool, bool)>, Box<dyn Error>> {
        let (state, _) =
            durable_work_state_for_owner(self.root.path(), &self.gateway.lease_owner_ref)?;
        Ok(state
            .work
            .items
            .values()
            .map(|item| {
                (
                    item.state.is_terminal(),
                    item.cancellation_requested_sequence.is_some(),
                )
            })
            .collect())
    }

    /// Queue a cron run that is already due for the chat's own session, the way the cron
    /// runtime does.
    fn entrust(&self, id: &str, instruction: &str) -> TestResult {
        automation_producer::enqueue_cron(&self.adapter, &cron_job(id, instruction))?;
        Ok(())
    }

    /// The queued automation runs as the durable store has them.
    fn runs(&self) -> Result<Vec<ReplayWorkItem>, Box<dyn Error>> {
        let (state, _) =
            durable_work_state_for_owner(self.root.path(), &self.gateway.lease_owner_ref)?;
        Ok(state
            .work
            .items
            .values()
            .filter(|item| item.work_kind == AUTOMATION_WORK_KIND)
            .cloned()
            .collect())
    }

    /// How each queued automation run ended, if it has.
    fn entrusted(&self) -> Result<Vec<Option<WorkTerminalKind>>, Box<dyn Error>> {
        Ok(self.runs()?.iter().map(|run| run.terminal_kind).collect())
    }

    /// What the store recorded for each automation run that ended: the job result with its
    /// reason, which `entrusted` leaves out.
    fn endings(&self) -> Result<Vec<String>, Box<dyn Error>> {
        Ok(self
            .runs()?
            .iter()
            .map(|run| {
                let result = run
                    .terminal_facts
                    .as_ref()
                    .and_then(|facts| facts.get("job_result"))
                    .map_or_else(|| "none".to_owned(), Value::to_string);
                format!(
                    "{} attempt={} state={:?} job_result={result}",
                    run.work_id, run.attempt, run.state
                )
            })
            .collect())
    }

    /// How many provider requests asked for `instruction`.
    fn asked(&self, instruction: &str) -> usize {
        self.provider
            .requests()
            .iter()
            .filter(|request| {
                request
                    .messages
                    .last()
                    .is_some_and(|message| message.to_string().contains(instruction))
            })
            .count()
    }

    fn running(&self) -> bool {
        self.processor
            .as_ref()
            .is_some_and(|processor| !processor.is_finished())
    }

    /// Stop the external processor the way the gateway's shutdown does.
    fn halt(&mut self) -> TestResult {
        self.stop.store(true, Ordering::SeqCst);
        let processor = self.processor.take().ok_or("processor already joined")?;
        processor
            .join()
            .map_err(|_| "external processor panicked")??;
        Ok(())
    }

    fn finish(mut self) -> TestResult {
        self.halt()
    }
}

impl Drop for Ingress {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        self.provider.hold(false);
    }
}

#[test]
fn browser_command_bypasses_busy_session_through_external_ingress() -> TestResult {
    // Given: a turn of the session held inside the provider and another message queued behind it.
    let provider = Provider::new(|index, _| {
        answer(
            ["첫째 답", "둘째 답", "셋째 답"]
                .get(index)
                .unwrap_or(&"남는 답"),
        )
    });
    let host = Host::open();
    let mut ingress = Ingress::start(provider.clone(), &host)?;
    provider.hold(true);
    ingress.say("첫째 글");
    eventually("the first turn to reach the provider", || {
        ingress.requests() == 1
    })?;
    ingress.say("둘째 글");
    assert_eq!(ingress.work()?, [(false, false); 2]);
    let files_before = ingress.session_files()?;
    let turns_before = ingress.turn_state();
    assert_eq!(
        turns_before,
        (
            vec![SESSION.to_owned()],
            vec![SESSION.to_owned()],
            Some(false)
        )
    );

    // When
    ingress.say("/browser status");
    ingress.hear(STATUS_REPLY)?;
    ingress.say("/browser close");
    ingress.hear(CLOSE_REPLY)?;

    // Then: both answers came while the turn was still held, and nothing of the session moved.
    assert_eq!(ingress.heard, [STATUS_REPLY, CLOSE_REPLY]);
    assert_eq!(ingress.requests(), 1);
    assert_eq!(ingress.turn_state(), turns_before);
    assert_eq!(ingress.session_files()?, files_before);
    assert_eq!(host.describe().state, BrowserDescriptionState::Closed);
    eventually("the browser commands to finish", || {
        ingress.work().is_ok_and(|work| {
            work.iter().filter(|(terminal, _)| *terminal).count() == 2
                && work.iter().all(|(_, cancelled)| !cancelled)
        })
    })?;

    // When: the held turn goes on.
    provider.hold(false);
    ingress.hear("첫째 답")?;
    ingress.hear("둘째 답")?;

    // Then: the running turn and the queued message were handled in the order sent.
    assert_eq!(
        ingress.heard,
        [STATUS_REPLY, CLOSE_REPLY, "첫째 답", "둘째 답"]
    );
    let requests = provider.requests();
    assert_eq!(requests.len(), 2);
    let second = serde_json::to_string(&requests[1].messages)?;
    assert!(second.contains("첫째 글") && second.contains("둘째 글"));
    assert!(!second.contains("/browser"), "{second}");
    eventually("every message to finish", || {
        ingress.work().is_ok_and(|work| work == [(true, false); 4])
    })?;
    for bytes in ingress.session_files()?.values() {
        assert!(!String::from_utf8_lossy(bytes).contains("/browser"));
    }

    // When: `/stop` arrives at the same place.
    provider.hold(true);
    ingress.say("셋째 글");
    eventually("the third turn to reach the provider", || {
        ingress.requests() == 3
    })?;
    ingress.say("넷째 글");
    ingress.say("/stop");
    ingress.hear(STOP_REPLY)?;

    // Then: it still cancels the running turn and the queued message.
    assert_eq!(ingress.turn_state().2, Some(true));
    assert_eq!(
        ingress
            .work()?
            .iter()
            .filter(|(_, cancelled)| *cancelled)
            .count(),
        2
    );
    provider.hold(false);
    eventually("the stopped work to finish", || {
        ingress
            .work()
            .is_ok_and(|work| work.iter().all(|(terminal, _)| *terminal))
    })?;
    assert_eq!(ingress.requests(), 3);
    assert!(!ingress.heard.iter().any(|heard| heard == "남는 답"));
    ingress.finish()
}

#[test]
fn browser_command_is_answered_before_automation_queued_for_running_session() -> TestResult {
    // Given: the gateway's automation worker beside the external processor, a turn of the
    // session held inside the provider, and two automation runs due for that same session.
    const ENTRUSTED: &str = "맡은 일을 해";
    let provider = Provider::new(|_, request| {
        let asked = request
            .messages
            .last()
            .map(Value::to_string)
            .unwrap_or_default();
        answer(if asked.contains(ENTRUSTED) {
            "맡은 일 답"
        } else {
            "사람 글 답"
        })
    });
    let host = Host::open();
    let mut ingress = Ingress::start(provider.clone(), &host)?;
    let worker = ingress.gateway.start_worker(ingress.adapter.clone())?;
    provider.hold(true);
    ingress.say("첫째 글");
    eventually("the first turn to reach the provider", || {
        ingress.requests() == 1
    })?;
    ingress.entrust("job-1", ENTRUSTED)?;
    ingress.entrust("job-2", ENTRUSTED)?;

    // When: the commands arrive one after another, so the intake thread has gone through its
    // whole round, durable work included, before it reads the second one.
    ingress.say("/browser status");
    ingress.hear(STATUS_REPLY)?;
    ingress.say("/browser close");
    ingress.hear(CLOSE_REPLY)?;

    // Then: both answers came while the turn was held and the automation still waited for it.
    assert_eq!(ingress.heard, [STATUS_REPLY, CLOSE_REPLY]);
    assert_eq!(ingress.requests(), 1);
    assert_eq!(ingress.entrusted()?, [None, None]);

    // When: the held turn ends.
    provider.hold(false);
    ingress.hear("사람 글 답")?;

    // Then: the worker runs the automation that waited.
    eventually("the automation runs to finish", || {
        ingress
            .entrusted()
            .is_ok_and(|ended| ended.iter().all(Option::is_some))
    })?;
    assert_eq!(
        ingress.entrusted()?,
        [Some(WorkTerminalKind::Succeeded); 2],
        "{:#?}",
        ingress.endings()?
    );
    assert_eq!(ingress.asked(ENTRUSTED), 2);
    eventually("the turns that report the automation to finish", || {
        ingress
            .work()
            .is_ok_and(|work| work.iter().all(|(terminal, _)| *terminal))
    })?;

    // When: `/stop` arrives while a turn is held and automation is due for its session.
    let asked = ingress.requests();
    provider.hold(true);
    ingress.say("둘째 글");
    eventually("the second turn to reach the provider", || {
        ingress.requests() == asked + 1
    })?;
    ingress.entrust("job-3", ENTRUSTED)?;
    ingress.say("/stop");
    ingress.hear(STOP_REPLY)?;

    // Then: it is answered while the turn is held, and cancels that turn.
    assert_eq!(ingress.requests(), asked + 1);
    assert_eq!(ingress.turn_state().2, Some(true));
    assert!(!worker.failure_signal().load(Ordering::SeqCst));
    provider.hold(false);
    worker.stop()?;
    ingress.finish()
}

#[test]
fn automation_run_stays_queued_until_its_session_is_free() -> TestResult {
    // Given: the worker beside the external processor, a turn of the session still running,
    // and automation due for that session and then for another one.
    const ENTRUSTED: &str = "맡은 일을 해";
    const ELSEWHERE: &str = "다른 세션 일을 해";
    const OTHER_SESSION: &str = "telegram:other-chat";
    let provider = entrusting_provider(ENTRUSTED, "사람 글");
    let host = Host::open();
    let ingress = Ingress::start(provider, &host)?;
    let worker = ingress.gateway.start_worker(ingress.adapter.clone())?;
    let turn = ingress
        .adapter
        .session_turn_lock
        .acquire(SESSION)
        .map_err(|error| format!("{error:?}"))?;
    ingress.entrust("job-1", ENTRUSTED)?;
    let mut elsewhere = cron_job("job-2", ELSEWHERE);
    elsewhere.payload.session_key = Some(OTHER_SESSION.to_owned());
    automation_producer::enqueue_cron(&ingress.adapter, &elsewhere)?;
    let run_of = |session: &str| -> Result<ReplayWorkItem, Box<dyn Error>> {
        ingress
            .runs()?
            .into_iter()
            .find(|run| run.session_key == session)
            .ok_or_else(|| format!("no run queued for {session}").into())
    };
    let ended = |session: &str| {
        worker.failure_signal().load(Ordering::SeqCst)
            || run_of(session).is_ok_and(|run| run.terminal_kind.is_some())
    };

    // When: the worker has gone past the busy session's run and finished the other one.
    eventually("the other session's run to finish", || ended(OTHER_SESSION))?;

    // Then: the busy session's run was neither started nor ended. It is still queued.
    let waiting = run_of(SESSION)?;
    assert_eq!(
        (waiting.state, waiting.attempt, waiting.terminal_kind),
        (ReplayWorkState::Pending, 0, None),
        "{:#?}",
        ingress.endings()?
    );
    assert_eq!(
        run_of(OTHER_SESSION)?.terminal_kind,
        Some(WorkTerminalKind::Succeeded),
        "{:#?}",
        ingress.endings()?
    );
    assert_eq!((ingress.asked(ENTRUSTED), ingress.asked(ELSEWHERE)), (0, 1));

    // When: the session's turn ends.
    drop(turn);
    eventually("the queued run to finish", || ended(SESSION))?;

    // Then: it ran once, on its first lease.
    let ran = run_of(SESSION)?;
    assert_eq!(
        (ran.terminal_kind, ran.attempt),
        (Some(WorkTerminalKind::Succeeded), 1),
        "{:#?}",
        ingress.endings()?
    );
    assert_eq!(ingress.asked(ENTRUSTED), 1);
    assert!(!worker.failure_signal().load(Ordering::SeqCst));
    eventually("the turns that report the automation to finish", || {
        ingress
            .work()
            .is_ok_and(|work| work.iter().all(|(terminal, _)| *terminal))
    })?;
    worker.stop()?;
    ingress.finish()
}

/// The gateway's adapters, with a stop landing on the session right after the worker claimed
/// its turn for a run.
struct StopOnClaim<'a>(&'a AgentLoopChatCompletionAdapter);

impl AutomationOwnerAdapters for StopOnClaim<'_> {
    fn snapshot(&self, request: &ProviderRequest) -> Result<ExecutionSnapshot, String> {
        self.0.snapshot(request)
    }

    fn hooks(&self, request: &AutomationDispatchRequest) -> AutomationHookEvaluation {
        self.0.hooks(request)
    }

    fn execute(
        &self,
        request: AutomationDispatchRequest,
        control: AutomationExecutionControl,
    ) -> AutomationExecutionReceipt {
        self.0.execute(request, control)
    }

    fn claim_session_turn(&self, session_key: &str) -> Option<SessionTurnReservation> {
        let claim = self.0.claim_session_turn(session_key);
        self.0
            .session_turn_lock
            .cancel_active_or_reserved(session_key);
        claim
    }
}

#[test]
fn stop_on_claimed_automation_turn_ends_the_run_as_cancelled() -> TestResult {
    // Given: an automation run due for a session, and a stop that lands on the session between
    // the worker's claim of the turn and the turn itself.
    const ENTRUSTED: &str = "맡은 일을 해";
    let provider = entrusting_provider(ENTRUSTED, "사람 글");
    let host = Host::open();
    let root = tempfile::tempdir()?;
    let adapter = adapter(
        root.path(),
        PermissionMode::Default,
        ToolRegistry::new(),
        &provider,
        &host,
    )?;
    let owner = runtime_owner_id(std::process::id(), now_millis());
    automation_producer::enqueue_cron(&adapter, &cron_job("job-1", ENTRUSTED))?;
    let mut worker = DurableWorkDispatcher::open(
        runtime_durable_event_root(root.path()),
        runtime_durable_work_payload_root(root.path()),
        MessageBus::new(),
        owner.clone(),
        DURABLE_WORK_LEASE_DURATION_MS,
    )?;

    // When: the worker's pass takes the run.
    automation_worker::process_due_automation(&mut worker, root.path(), &StopOnClaim(&adapter))?;

    // Then: the run ended as a cancelled one without reaching the provider, and the session
    // is free again.
    let (state, _) = durable_work_state_for_owner(root.path(), &owner)?;
    let runs = state
        .work
        .items
        .values()
        .filter(|item| item.work_kind == AUTOMATION_WORK_KIND)
        .collect::<Vec<_>>();
    assert_eq!(
        (runs.len(), runs[0].terminal_kind, runs[0].attempt),
        (1, Some(WorkTerminalKind::Failed), 1)
    );
    let facts = runs[0].terminal_facts.as_ref().map(Value::to_string);
    assert!(
        facts
            .as_deref()
            .is_some_and(|facts| facts.contains("automation:cancelled")),
        "{facts:?}"
    );
    assert!(provider.requests().is_empty());
    assert!(!adapter.session_turn_lock.is_busy(SESSION));
    Ok(())
}

/// The gateway's adapters, with the session's turn ending right after the worker was refused
/// it for a run.
struct FreedOnRefusal<'a> {
    adapters: &'a AgentLoopChatCompletionAdapter,
    turn: Mutex<Option<SessionTurnReservation>>,
}

impl AutomationOwnerAdapters for FreedOnRefusal<'_> {
    fn snapshot(&self, request: &ProviderRequest) -> Result<ExecutionSnapshot, String> {
        self.adapters.snapshot(request)
    }

    fn hooks(&self, request: &AutomationDispatchRequest) -> AutomationHookEvaluation {
        self.adapters.hooks(request)
    }

    fn execute(
        &self,
        request: AutomationDispatchRequest,
        control: AutomationExecutionControl,
    ) -> AutomationExecutionReceipt {
        self.adapters.execute(request, control)
    }

    fn claim_session_turn(&self, session_key: &str) -> Option<SessionTurnReservation> {
        let claim = self.adapters.claim_session_turn(session_key);
        if claim.is_none() {
            lock(&self.turn).take();
        }
        claim
    }
}

/// The automation runs as the durable store has them.
fn automation_runs(root: &Path, owner: &str) -> Result<Vec<ReplayWorkItem>, Box<dyn Error>> {
    let (state, _) = durable_work_state_for_owner(root, owner)?;
    Ok(state
        .work
        .items
        .values()
        .filter(|item| item.work_kind == AUTOMATION_WORK_KIND)
        .cloned()
        .collect())
}

#[test]
fn later_run_of_a_refused_session_waits_behind_the_refused_one() -> TestResult {
    // Given: two automation runs due for one session, whose running turn ends right after the
    // worker was refused it for the first run.
    const FIRST: &str = "먼저 맡은 일";
    const SECOND: &str = "나중에 맡은 일";
    let provider = Provider::new(|_, _| answer("맡은 일 답"));
    let host = Host::open();
    let root = tempfile::tempdir()?;
    let adapter = adapter(
        root.path(),
        PermissionMode::Default,
        ToolRegistry::new(),
        &provider,
        &host,
    )?;
    let owner = runtime_owner_id(std::process::id(), now_millis());
    automation_producer::enqueue_cron(&adapter, &cron_job("job-1", FIRST))?;
    automation_producer::enqueue_cron(&adapter, &cron_job("job-2", SECOND))?;
    let mut worker = DurableWorkDispatcher::open(
        runtime_durable_event_root(root.path()),
        runtime_durable_work_payload_root(root.path()),
        MessageBus::new(),
        owner.clone(),
        DURABLE_WORK_LEASE_DURATION_MS,
    )?;
    let adapters = FreedOnRefusal {
        adapters: &adapter,
        turn: Mutex::new(adapter.claim_session_turn(SESSION)),
    };
    assert!(lock(&adapters.turn).is_some());

    // When: the worker's pass goes over both runs.
    automation_worker::process_due_automation(&mut worker, root.path(), &adapters)?;

    // Then: the session is free, yet the second run did not go ahead of the first.
    assert!(!adapter.session_turn_lock.is_busy(SESSION));
    assert!(provider.requests().is_empty());
    assert_eq!(
        automation_runs(root.path(), &owner)?
            .iter()
            .map(|run| (run.state, run.attempt))
            .collect::<Vec<_>>(),
        [(ReplayWorkState::Pending, 0); 2]
    );

    // When: the next pass runs.
    automation_worker::process_due_automation(&mut worker, root.path(), &adapters)?;

    // Then: both ran, in the order they were queued.
    let asked = provider
        .requests()
        .iter()
        .filter_map(|request| request.messages.last().map(Value::to_string))
        .map(|asked| (asked.contains(FIRST), asked.contains(SECOND)))
        .collect::<Vec<_>>();
    assert_eq!(asked, [(true, false), (false, true)]);
    assert_eq!(
        automation_runs(root.path(), &owner)?
            .iter()
            .map(|run| run.terminal_kind)
            .collect::<Vec<_>>(),
        [Some(WorkTerminalKind::Succeeded); 2]
    );
    Ok(())
}

#[test]
fn automation_behind_a_full_report_of_waiting_runs_still_runs() -> TestResult {
    // Given: as many automation runs waiting for a busy session as the admission report lists,
    // and a run for another session queued after all of them.
    const WAITING: &str = "기다리는 일";
    const OTHER: &str = "다른 대화가 맡긴 일";
    const OTHER_SESSION: &str = "telegram:other";
    let provider = Provider::new(|_, _| answer("맡은 일 답"));
    let host = Host::open();
    let root = tempfile::tempdir()?;
    let adapter = adapter(
        root.path(),
        PermissionMode::Default,
        ToolRegistry::new(),
        &provider,
        &host,
    )?;
    let owner = runtime_owner_id(std::process::id(), now_millis());
    for index in 0..MAX_PROJECTED_WORK_IDS {
        automation_producer::enqueue_cron(&adapter, &cron_job(&format!("job-{index}"), WAITING))?;
    }
    let mut other = cron_job("job-other", OTHER);
    other.payload.session_key = Some(OTHER_SESSION.to_owned());
    automation_producer::enqueue_cron(&adapter, &other)?;
    let mut worker = DurableWorkDispatcher::open(
        runtime_durable_event_root(root.path()),
        runtime_durable_work_payload_root(root.path()),
        MessageBus::new(),
        owner.clone(),
        DURABLE_WORK_LEASE_DURATION_MS,
    )?;
    let _turn = adapter
        .claim_session_turn(SESSION)
        .ok_or("the session's turn")?;

    // When: the worker's pass runs.
    automation_worker::process_due_automation(&mut worker, root.path(), &adapter)?;

    // Then: the other session's run was done, and every waiting run is still queued.
    let asked = provider
        .requests()
        .iter()
        .filter_map(|request| request.messages.last().map(Value::to_string))
        .map(|asked| asked.contains(OTHER))
        .collect::<Vec<_>>();
    assert_eq!(asked, [true]);
    let (others, waiting): (Vec<_>, Vec<_>) = automation_runs(root.path(), &owner)?
        .into_iter()
        .partition(|run| run.session_key == OTHER_SESSION);
    assert_eq!(
        others
            .iter()
            .map(|run| run.terminal_kind)
            .collect::<Vec<_>>(),
        [Some(WorkTerminalKind::Succeeded)]
    );
    assert_eq!(
        waiting
            .iter()
            .map(|run| (run.state, run.attempt))
            .collect::<Vec<_>>(),
        vec![(ReplayWorkState::Pending, 0); MAX_PROJECTED_WORK_IDS]
    );
    Ok(())
}

/// Answers by what the last message asks for, so that each kind of turn is told apart.
fn entrusting_provider(entrusted: &'static str, said: &'static str) -> Arc<Provider> {
    Provider::new(move |_, request| {
        let asked = request
            .messages
            .last()
            .map(Value::to_string)
            .unwrap_or_default();
        answer(if asked.contains(entrusted) {
            "맡은 일 답"
        } else if asked.contains(said) {
            "사람 글 답"
        } else {
            "알림 답"
        })
    })
}

#[test]
fn commands_are_answered_while_automation_runs_past_its_lease() -> TestResult {
    // Given: the worker and the external processor as the gateway opens them, and an automation
    // run of the session held inside the provider.
    const ENTRUSTED: &str = "맡은 일을 해";
    const SAID: &str = "사람 글";
    let provider = entrusting_provider(ENTRUSTED, SAID);
    let host = Host::open();
    let mut ingress = Ingress::start(provider.clone(), &host)?;
    let worker = ingress.gateway.start_worker(ingress.adapter.clone())?;
    provider.hold(true);
    ingress.entrust("job-1", ENTRUSTED)?;
    eventually("the automation run to reach the provider", || {
        ingress.requests() == 1
    })?;
    let lease_ends = ingress.runs()?[0]
        .lease_expires_at_ms
        .ok_or("the running automation holds no lease")?;

    // When: the run outlives its lease, and commands arrive after that.
    thread::sleep(Duration::from_millis(
        lease_ends.saturating_sub(now_millis()) + 100,
    ));
    ingress.say("/browser status");
    ingress.hear(STATUS_REPLY)?;
    ingress.say("/browser close");
    ingress.hear(CLOSE_REPLY)?;

    // Then: both are answered, and the run is still the one lease the worker took.
    assert_eq!(ingress.heard, [STATUS_REPLY, CLOSE_REPLY]);
    assert!(ingress.running());
    assert!(!worker.failure_signal().load(Ordering::SeqCst));
    let runs = ingress.runs()?;
    assert_eq!(
        (runs.len(), runs[0].state, runs[0].attempt),
        (1, ReplayWorkState::Leased, 1)
    );
    assert!(now_millis() > lease_ends);
    assert_eq!(ingress.requests(), 1);

    // When: the held run goes on.
    provider.hold(false);
    eventually("the automation run to finish", || {
        ingress
            .entrusted()
            .is_ok_and(|ended| ended.iter().all(Option::is_some))
    })?;

    // Then: it ended as itself, having run once.
    let runs = ingress.runs()?;
    assert_eq!(
        (runs.len(), runs[0].terminal_kind, runs[0].attempt),
        (1, Some(WorkTerminalKind::Succeeded), 1)
    );
    assert_eq!(ingress.asked(ENTRUSTED), 1);
    eventually("the turn that reports the automation to finish", || {
        ingress
            .work()
            .is_ok_and(|work| work.iter().all(|(terminal, _)| *terminal))
    })?;

    // When: a message arrives after all that.
    ingress.say(SAID);
    ingress.hear("사람 글 답")?;

    // Then: it got its turn, and the worker and the processor end cleanly.
    assert_eq!(ingress.asked(SAID), 1);
    assert_eq!(ingress.asked(ENTRUSTED), 1);
    assert!(!worker.failure_signal().load(Ordering::SeqCst));
    worker.stop()?;
    ingress.finish()
}

#[test]
fn worker_keeps_working_while_turn_runs_past_its_lease() -> TestResult {
    // Given: a turn the external processor leased so long ago that the lease has run out.
    const ENTRUSTED: &str = "맡은 일을 해";
    let provider = entrusting_provider(ENTRUSTED, "사람 글");
    let host = Host::open();
    let root = tempfile::tempdir()?;
    let adapter = Arc::new(adapter(
        root.path(),
        PermissionMode::Default,
        ToolRegistry::new(),
        &provider,
        &host,
    )?);
    let bus = MessageBus::new();
    let mut context = ExternalTransportRuntimeContext::new(root.path().join("metadata"), 1);
    let gateway = GatewayDurableWork::new(
        root.path().to_path_buf(),
        runtime_owner_id(std::process::id(), now_millis()),
    );
    let mut ingress = gateway.open_ingress(&mut context, &bus, None)?;
    ingress.dispatcher.enqueue_inbound(
        "long-turn",
        &InboundMessage::new(TELEGRAM_CHANNEL, "user", "other-chat", "오래 걸리는 글"),
        None,
        None,
    )?;
    let owner = ingress.dispatcher.lease_owner_ref().to_owned();
    let (state, admission) = durable_work_state_for_owner(root.path(), &owner)?;
    ingress
        .dispatcher
        .dispatch_due(&state.work, &admission, 1)?;
    let (state, _) = durable_work_state_for_owner(root.path(), &owner)?;
    let turn = &state.work.items["long-turn"];
    assert_eq!(turn.state, ReplayWorkState::Leased);
    assert!(turn
        .lease_expires_at_ms
        .is_some_and(|ends| ends < now_millis()));

    // When: the gateway's worker runs beside it and automation becomes due.
    let worker = gateway.start_worker(adapter.clone())?;
    automation_producer::enqueue_cron(&adapter, &cron_job("job-1", ENTRUSTED))?;
    let runs = || -> Result<Vec<ReplayWorkItem>, CliError> {
        let (state, _) = durable_work_state_for_owner(root.path(), &owner)?;
        Ok(state
            .work
            .items
            .values()
            .filter(|item| item.work_kind == AUTOMATION_WORK_KIND)
            .cloned()
            .collect())
    };
    eventually("the automation run to finish", || {
        worker.failure_signal().load(Ordering::SeqCst)
            || runs().is_ok_and(|runs| runs.iter().all(|run| run.terminal_kind.is_some()))
    })?;

    // Then: the worker took the run and ended it, and the turn's lease was left alone.
    assert!(!worker.failure_signal().load(Ordering::SeqCst));
    let runs = runs()?;
    assert_eq!(
        (runs.len(), runs[0].terminal_kind, runs[0].attempt),
        (1, Some(WorkTerminalKind::Succeeded), 1)
    );
    let (state, _) = durable_work_state_for_owner(root.path(), &owner)?;
    let turn = &state.work.items["long-turn"];
    assert_eq!((turn.state, turn.attempt), (ReplayWorkState::Leased, 1));
    worker.stop()?;
    Ok(())
}

#[test]
fn processor_shutdown_leaves_running_automation_to_the_worker() -> TestResult {
    // Given: an automation run the worker holds inside the provider.
    const ENTRUSTED: &str = "맡은 일을 해";
    let provider = entrusting_provider(ENTRUSTED, "사람 글");
    let host = Host::open();
    let mut ingress = Ingress::start(provider.clone(), &host)?;
    let worker = ingress.gateway.start_worker(ingress.adapter.clone())?;
    provider.hold(true);
    ingress.entrust("job-1", ENTRUSTED)?;
    eventually("the automation run to reach the provider", || {
        ingress.requests() == 1
    })?;

    // When: the gateway stops the external processor first, as its shutdown does.
    ingress.halt()?;

    // Then: the shutdown asked the busy session's run to stop, and the lease the worker took is
    // still in place.
    let runs = ingress.runs()?;
    assert_eq!(
        (runs.len(), runs[0].state, runs[0].attempt),
        (1, ReplayWorkState::Leased, 1)
    );
    assert!(runs[0].cancellation_requested_sequence.is_some());

    // When: the worker stops after it.
    provider.hold(false);
    worker.stop()?;

    // Then: the worker ended the run itself as a cancelled one, and it ran once.
    let runs = ingress.runs()?;
    assert_eq!(
        (
            runs.len(),
            runs[0].state,
            runs[0].terminal_kind,
            runs[0].attempt
        ),
        (
            1,
            ReplayWorkState::Terminal,
            Some(WorkTerminalKind::Failed),
            1
        )
    );
    let facts = runs[0].terminal_facts.as_ref().map(Value::to_string);
    assert!(
        facts
            .as_deref()
            .is_some_and(|facts| facts.contains("automation:cancelled")),
        "{facts:?}"
    );
    assert_eq!(ingress.asked(ENTRUSTED), 1);
    Ok(())
}

#[test]
fn shutdown_requeue_returns_turns_and_leaves_other_work_of_the_owner() -> TestResult {
    // Given: a turn the processor leased, and an automation run leased under the same gateway
    // owner that nothing asked to stop.
    let provider = entrusting_provider("맡은 일을 해", "사람 글");
    let host = Host::open();
    let root = tempfile::tempdir()?;
    let adapter = adapter(
        root.path(),
        PermissionMode::Default,
        ToolRegistry::new(),
        &provider,
        &host,
    )?;
    let bus = MessageBus::new();
    let mut context = ExternalTransportRuntimeContext::new(root.path().join("metadata"), 1);
    let gateway = GatewayDurableWork::new(
        root.path().to_path_buf(),
        runtime_owner_id(std::process::id(), now_millis()),
    );
    let mut ingress = gateway.open_ingress(&mut context, &bus, None)?;
    let owner = ingress.dispatcher.lease_owner_ref().to_owned();
    ingress.dispatcher.enqueue_inbound(
        "turn",
        &InboundMessage::new(TELEGRAM_CHANNEL, "user", CHAT, "사람 글"),
        None,
        None,
    )?;
    automation_producer::enqueue_cron(&adapter, &cron_job("job-1", "맡은 일을 해"))?;
    let (state, admission) = durable_work_state_for_owner(root.path(), &owner)?;
    ingress
        .dispatcher
        .dispatch_due(&state.work, &admission, now_millis())?;
    let run = state
        .work
        .items
        .values()
        .find(|item| item.work_kind == AUTOMATION_WORK_KIND)
        .ok_or("the automation run was not queued")?;
    let mut worker = DurableWorkDispatcher::open(
        runtime_durable_event_root(root.path()),
        runtime_durable_work_payload_root(root.path()),
        MessageBus::new(),
        owner.clone(),
        DURABLE_WORK_LEASE_DURATION_MS,
    )?;
    worker.lease_work(run, now_millis())?;

    // When
    requeue_owned_external_work(&mut ingress)?;

    // Then
    let (state, _) = durable_work_state_for_owner(root.path(), &owner)?;
    assert_eq!(state.work.items["turn"].state, ReplayWorkState::Pending);
    let run = &state.work.items[&run.work_id];
    assert_eq!(
        (run.state, run.attempt, run.lease_owner_ref.as_deref()),
        (ReplayWorkState::Leased, 1, Some(owner.as_str()))
    );
    Ok(())
}

/// How the gateway's shutdown wait ends over the component signals it polls.
fn gateway_wait(
    data_dir: &Path,
    signals: Vec<Arc<AtomicBool>>,
) -> Result<RuntimeShutdownReason, Box<dyn Error>> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    Ok(runtime.block_on(async {
        tokio::time::timeout(
            PATIENCE,
            wait_for_ctrl_c_or_runtime_request(
                data_dir.to_path_buf(),
                RuntimeOwnerFence::new(),
                "owner".to_owned(),
                signals,
            ),
        )
        .await
    })?)
}

#[test]
fn automation_run_that_panics_ends_the_gateway_wait() -> TestResult {
    // Given: the worker and the external processor as the gateway opens them, and an automation
    // run whose provider call panics.
    const ENTRUSTED: &str = "맡은 일을 해";
    let provider = Provider::new(|_, request| {
        let asked = request
            .messages
            .last()
            .map(Value::to_string)
            .unwrap_or_default();
        assert!(!asked.contains(ENTRUSTED), "the automation run panics");
        answer("답")
    });
    let host = Host::open();
    let ingress = Ingress::start(provider, &host)?;
    let worker = ingress.gateway.start_worker(ingress.adapter.clone())?;
    let signals = vec![ingress.exited.clone(), worker.failure_signal()];

    // When: the worker leases the run and the run panics. Nothing stops the worker.
    ingress.entrust("job-1", ENTRUSTED)?;
    eventually("the worker to signal its end", || {
        worker.failure_signal().load(Ordering::SeqCst)
    })?;

    // Then: the run is left as the worker leased it, the processor is still up, and the gateway's
    // wait ends on the worker's signal.
    let runs = ingress.runs()?;
    assert_eq!(
        (runs.len(), runs[0].state, runs[0].attempt),
        (1, ReplayWorkState::Leased, 1)
    );
    assert_eq!(ingress.asked(ENTRUSTED), 1);
    assert!(ingress.running());
    assert!(!ingress.exited.load(Ordering::SeqCst));
    assert_eq!(
        gateway_wait(ingress.root.path(), signals)?,
        RuntimeShutdownReason::StartupFailed
    );

    // When: the gateway stops its parts on that.
    let stopped = worker.stop();

    // Then: the worker reports the panic.
    let reason = stopped
        .expect_err("a panicked worker stopped cleanly")
        .to_string();
    assert!(
        reason.contains("surface approval worker panicked"),
        "{reason}"
    );
    ingress.finish()
}

#[test]
fn processor_thread_that_panics_or_fails_ends_the_gateway_wait() -> TestResult {
    for panics in [true, false] {
        // Given: a supervisor whose processor thread ends on its own, by a panic or by an error
        // the processor did not report through its signal.
        let root = tempfile::tempdir()?;
        let exited = Arc::new(AtomicBool::new(false));
        let supervisor = ExternalChannelSupervisor {
            stop: Arc::new(AtomicBool::new(false)),
            shutdown_reason: Arc::new(Mutex::new(RuntimeShutdownReason::Stop)),
            exited: exited.clone(),
            handles: vec![ExternalChannelSupervisor::spawn_processor(
                exited,
                move || {
                    assert!(!panics, "the processor panics");
                    Err(CliError::Runtime("the processor failed".to_owned()))
                },
            )],
        };
        let health = supervisor.health_signal();

        // When: nothing stops it.
        eventually("the processor thread to signal its end", || {
            health.load(Ordering::SeqCst)
        })?;

        // Then: the gateway's wait ends on that signal, and stopping reports what happened.
        assert_eq!(
            gateway_wait(root.path(), vec![health])?,
            RuntimeShutdownReason::StartupFailed,
            "panics={panics}"
        );
        let reason = supervisor
            .stop(RuntimeShutdownReason::StartupFailed)
            .expect_err("a processor that ended on its own stopped cleanly")
            .to_string();
        let expected = if panics {
            "external runtime panicked"
        } else {
            "the processor failed"
        };
        assert!(reason.contains(expected), "panics={panics}: {reason}");
    }
    Ok(())
}

fn captured_path(request: &ProviderRequest) -> String {
    request
        .messages
        .iter()
        .filter(|message| message["role"] == "tool" && message["tool_call_id"] == "capture")
        .filter_map(|message| message["content"].as_str())
        .filter_map(|content| serde_json::from_str::<Value>(content).ok())
        .filter_map(|parts| {
            let text = parts[0]["text"].as_str()?;
            let path = text.strip_prefix("browser_capture: ")?.lines().next()?;
            Some(path.to_owned())
        })
        .next_back()
        .unwrap_or_default()
}

#[test]
fn capture_path_reaches_message_tool_with_distinct_media_roots() -> TestResult {
    // Given: the tools as production registers them and the loop config the CLI makes.
    let folder = tempfile::tempdir()?;
    let root = folder.path().canonicalize()?;
    let media = root.join("data").join("media");
    let host = Host::open();
    let message_tool = MessageTool::new(root.join("workspace")).with_media_roots([media.clone()]);
    let mut tools = ToolRegistry::new();
    tools.register(BrowserTool::new(host.clone()));
    tools.register(message_tool.clone());
    let provider = Provider::new(|index, request| match index {
        0 => call("capture", "browser", json!({"action":"capture"})),
        1 => call(
            "send",
            "message",
            json!({"content":"지금 화면입니다","media":[captured_path(request)]}),
        ),
        _ => answer("보냈습니다."),
    });
    let mut adapter = adapter(
        &root,
        PermissionMode::BypassPermissions,
        tools,
        &provider,
        &host,
    )?;
    adapter.message_tool = Some(message_tool);
    adapter.containment_snapshot = Some(ContainmentSnapshotRef {
        contained: Some(true),
        backend: None,
        digest: Some("test-contained".to_owned()),
        summary: Some("test containment".to_owned()),
    });
    let mut config = adapter.loop_config_for(RunKind::Interactive);
    config.permission_rule_input.containment = DockerContainmentSnapshot {
        contained: Some(true),
        runtime: ContainerRuntimeKind::Docker,
        root_user: Some(false),
        privileged: Some(false),
        host_mounts_summary: Vec::new(),
        network_mode: ContainerNetworkMode::None,
        digest: Some("test-contained".to_owned()),
        summary: Some("non-privileged test containment".to_owned()),
    };
    assert_eq!(
        config.permission_mode_snapshot.mode,
        PermissionMode::BypassPermissions
    );
    assert_eq!(
        config
            .tool_images
            .as_ref()
            .map(|images| images.media_root.clone()),
        Some(media.join("api"))
    );
    assert_eq!(config.media_roots, std::slice::from_ref(&media));

    // When
    let bus = MessageBus::new();
    let (_turn, mut sent, _children) = adapter.process_external_inbound_with_streaming(
        InboundMessage::new(TELEGRAM_CHANNEL, "user", CHAT, "지금 화면 보여줘"),
        config,
        &bus,
    )?;
    while let Some(message) = bus.try_consume_outbound() {
        sent.push(message);
    }

    // Then: the path `capture` returned is the file that went out to the chat.
    let requests = provider.requests();
    assert_eq!(requests.len(), 3, "{requests:?}");
    let path = PathBuf::from(captured_path(&requests[1]));
    assert!(path.is_absolute(), "{:?}", requests[1].messages.last());
    assert_eq!(
        path.parent(),
        Some(media.join("api/tool-results").as_path())
    );
    assert_eq!(fs::read(&path)?, SCREEN);
    let attached = sent
        .iter()
        .filter(|message| !message.media.is_empty())
        .collect::<Vec<_>>();
    assert_eq!(attached.len(), 1, "{:?}", requests[2].messages.last());
    assert_eq!(
        (attached[0].channel.as_str(), attached[0].chat_id.as_str()),
        (TELEGRAM_CHANNEL, CHAT)
    );
    assert_eq!(attached[0].media, [path.to_string_lossy()]);
    assert_eq!(fs::read(&attached[0].media[0])?, SCREEN);
    let file_name = path.file_name().ok_or("capture file name")?;
    assert!(!media.join("tool-results").join(file_name).exists());
    assert!(!serde_json::to_string(&requests[2].messages)?.contains("image_url"));
    Ok(())
}
