//! Spec 050 PRD 010 through the external ingress: the real browser manager over the fakes of
//! `stage`, the real durable line, processor and outbound worker, and a Telegram whose HTTP
//! calls alone are faked.

use super::stage::{Faults, Stage, Timing};
use super::{answer, call, eventually, lock, Processor, Script, TestResult, HOLD_LIMIT, PATIENCE};
use crate::browser::{
    BrowserControl, BrowserEpisodeRecord, BrowserEpisodeRecords, BrowserOpenEpisode, BrowserState,
    BrowserStatus,
};
use crate::tests::scripted_agent_loop_adapter;
use crate::{
    automation_producer, browser_resume_inbound, browser_resume_notifier,
    dispatch_due_external_work, drain_outbound, durable_work_state_for_owner,
    external_transport_channel_manager_with_runner, now_millis, publish_external_inbound_with_hint,
    run_external_agent_processor_with_channels, runtime_durable_event_root,
    runtime_durable_work_payload_root, runtime_owner_id, sha256_hex, telegram_sent_message_id,
    telegram_update_to_inbound, AgentLoopChatCompletionAdapter, DurableWorkDispatcher,
    ExternalChannelSupervisor, ExternalProcessorShutdownControl, ExternalTransportRunner,
    ExternalTransportRuntimeContext, ExternalTransportSpec, GatewayDurableWork,
    RuntimeShutdownReason, SessionManager, SlackRuntimeConfig, TelegramRuntimeConfig,
    DURABLE_INBOUND_ENQUEUE_LOCK, DURABLE_WORK_LEASE_DURATION_MS, TELEGRAM_CHANNEL,
};
use serde_json::{json, Map, Value};
use shacs_channels::{OutboundMessage, WebSocketServerEvent};
use shacs_config::{Config, PermissionMode};
use shacs_core::runtime::{
    BrowserArrival, BrowserCallContext, BrowserCloseOutcome, BrowserError, BrowserHost,
    BrowserPendingResume, BrowserSession, InboundMessage, MessageBus, SessionTurnReservation,
    AUTOMATION_WORK_KIND, BROWSER_ARRIVAL_METADATA, BROWSER_EPISODE_NOTICE_METADATA,
    BROWSER_RESUME_EVENT, BROWSER_RESUME_HINT_PREFIX, BROWSER_RESUME_WORK_PREFIX,
};
use shacs_core::tools::{pending_ask_user_id, BrowserTool, ToolRegistry};
use shacs_providers::{LlmResponse, ProviderClient, ProviderError, ProviderEvent, ProviderRequest};
use shacs_session::durable_work::{ReplayWorkItem, ReplayWorkState, WorkTerminalKind};
use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Condvar, Mutex};
use std::time::{Duration, Instant};
use std::{fs, thread};

#[path = "reply_route.rs"]
mod reply_route;
#[path = "websocket_resume.rs"]
mod websocket_resume;

const ROOM_A: &str = "chat";
const SESSION_A: &str = "telegram:chat";
const ROOM_B: &str = "room-b";
const ROOM_C: &str = "room-c";
const SESSION_C: &str = "telegram:room-c";
const MINUTES: u64 = 15;
/// How often the outbound worker tries one message.
const SEND_ATTEMPTS: usize = 2;
const LIMIT_MS: u64 = MINUTES * 60_000;
const QUESTION: &str = "브라우저를 넘깁니다: 로그인이 필요합니다";
const TAKEN: &str = "브라우저를 넘겨받았습니다(";
const RETURNED: &str = "브라우저를 돌려줬습니다.";
const OVER: &str = "그 넘겨받기는 이미 끝났습니다.";
const NOT_THIS_NOTICE: &str = "이 메시지는 다른 메시지에 단 답글이라 지금 넘겨받기의 답으로 쓰지 않았습니다. 끝나면 지금 넘겨받기 안내에 답글을 달거나 답글 없이 답해 주세요.";
const BEFORE_NOTICE: &str = "이 메시지는 넘겨받기 안내가 가기 전에 보낸 것이라 답으로 쓰지 않았습니다. 끝나면 다시 답해 주세요.";
const RETURN_REPLY_MISMATCH: &str = "답글로 보낸 /browser return은 답글이 가리키는 안내의 넘겨받기만 돌려줍니다. 그 넘겨받기는 지금 열려 있지 않습니다. 지금 넘겨받기를 돌려주려면 답글 없이 /browser return을 보내 주세요.";
const FORGERY: &str = "automation result lacks an owner-accepted terminal boundary";
/// What the assistant is told when its paused `handoff` goes on, by how the episode ended.
const ENDINGS: [&str; 7] = [
    "browser_returned: 사용자가 돌려줬습니다. 다시 look하세요.",
    "browser_takeover_timeout: 넘겨받기 시간이 지났습니다. 다시 look하세요.",
    "browser_closed: 넘겨받은 동안 브라우저가 닫혔습니다. 페이지를 다시 여세요.",
    "browser_view_failed: 화면 보기가 꺼져 돌려받았습니다. 다시 look하세요.",
    "browser_handoff_answer: 사용자가 답했고 브라우저를 돌려받았습니다. 다시 look하세요.",
    "browser_takeover_over: 넘겨받기는 이미 끝났습니다. 다시 look하세요.",
    "browser_handoff_unavailable: 넘겨주지 못했습니다(",
];

fn handoff(index: usize) -> LlmResponse {
    call(
        &format!("handoff-{index}"),
        "browser",
        json!({"action":"handoff","reason":"로그인이 필요합니다"}),
    )
}

fn last(request: &ProviderRequest) -> String {
    request
        .messages
        .last()
        .map(Value::to_string)
        .unwrap_or_default()
}

fn told(request: &ProviderRequest) -> Option<&'static str> {
    let last = last(request);
    last.contains(r#""role":"tool""#)
        .then(|| ENDINGS.into_iter().find(|ending| last.contains(ending)))
        .flatten()
}

fn word(ending: &str) -> &str {
    ending.split(':').next().unwrap_or(ending)
}

/// Hands over when asked to, and says how each paused `handoff` went on.
fn assistant(index: usize, request: &ProviderRequest) -> LlmResponse {
    if let Some(ending) = told(request) {
        return answer(&format!("이어서: {}", word(ending)));
    }
    if last(request).contains("넘겨줘") {
        return handoff(index);
    }
    answer("답했어요")
}

#[derive(Default)]
struct Gate {
    held: bool,
    passes: usize,
}

/// A provider whose requests can be held and let through one at a time.
struct Model {
    script: Script,
    requests: Mutex<Vec<ProviderRequest>>,
    gate: Mutex<Gate>,
    moved: Condvar,
}

impl Model {
    fn new(
        script: impl Fn(usize, &ProviderRequest) -> LlmResponse + Send + Sync + 'static,
    ) -> Arc<Self> {
        Arc::new(Self {
            script: Box::new(script),
            requests: Mutex::new(Vec::new()),
            gate: Mutex::new(Gate::default()),
            moved: Condvar::new(),
        })
    }

    fn requests(&self) -> Vec<ProviderRequest> {
        lock(&self.requests).clone()
    }

    fn hold(&self) {
        lock(&self.gate).held = true;
    }

    fn pass(&self) {
        lock(&self.gate).passes += 1;
        self.moved.notify_all();
    }

    fn free(&self) {
        *lock(&self.gate) = Gate::default();
        self.moved.notify_all();
    }
}

impl ProviderClient for Model {
    fn chat(&self, request: ProviderRequest) -> Result<LlmResponse, ProviderError> {
        let index = {
            let mut requests = lock(&self.requests);
            requests.push(request.clone());
            requests.len() - 1
        };
        let mut gate = self
            .moved
            .wait_timeout_while(lock(&self.gate), HOLD_LIMIT, |gate| {
                gate.held && gate.passes == 0
            })
            .expect("test lock poisoned")
            .0;
        gate.passes = gate.passes.saturating_sub(1);
        drop(gate);
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

#[derive(Clone, Debug, PartialEq, Eq)]
struct Sent {
    id: String,
    chat: String,
    text: String,
    reply_to: Option<String>,
    /// The takeover episode this message is the notice of.
    notice: Option<String>,
    heard: bool,
}

#[derive(Default)]
struct Line {
    sent: Vec<Sent>,
    /// Every message a send was tried for, the failed ones included.
    tried: Vec<OutboundMessage>,
    attempts: usize,
    failing: bool,
    /// Telegram answers a send with success but no `message_id`.
    nameless: bool,
}

/// The fake Telegram: what it was sent, and whether the outbound worker runs.
#[derive(Default)]
struct Wire {
    line: Mutex<Line>,
    /// Held by the outbound worker while it sends, so a pause is in force once it is set.
    paused: Mutex<bool>,
}

impl Wire {
    fn send(&self, message: OutboundMessage) -> Result<Vec<String>, String> {
        let mut line = lock(&self.line);
        line.attempts += 1;
        line.tried.push(message.clone());
        if line.failing {
            return Err("Telegram sendMessage failed".to_owned());
        }
        let number = 9000 + line.attempts;
        let response = if line.nameless {
            json!({"ok":true,"result":{}})
        } else {
            json!({"ok":true,"result":{"message_id":number}})
        };
        let id = telegram_sent_message_id(&response)?;
        line.sent.push(Sent {
            id: id.clone(),
            chat: message.chat_id,
            text: message.content,
            reply_to: message.reply_to,
            notice: message
                .metadata
                .get(BROWSER_EPISODE_NOTICE_METADATA)
                .and_then(Value::as_str)
                .map(str::to_owned),
            heard: false,
        });
        Ok(vec![id])
    }

    fn pause(&self, paused: bool) {
        *lock(&self.paused) = paused;
    }
}

/// The browser host as the transports see it. It notes every delivery report on its way to
/// the manager.
struct Ear {
    host: Arc<dyn BrowserHost>,
    told: Mutex<Vec<(String, Option<String>)>>,
}

impl BrowserHost for Ear {
    fn acquire(
        &self,
        context: &BrowserCallContext,
    ) -> Result<Box<dyn BrowserSession>, BrowserError> {
        self.host.acquire(context)
    }

    fn arrival(&self) -> BrowserArrival {
        self.host.arrival()
    }

    fn episode_notice_delivered(&self, episode: &str, message_id: Option<&str>) {
        lock(&self.told).push((episode.to_owned(), message_id.map(str::to_owned)));
        self.host.episode_notice_delivered(episode, message_id);
    }
}

/// What must not move when something is refused.
#[derive(Debug, PartialEq)]
struct Seen {
    files: BTreeMap<PathBuf, Vec<u8>>,
    requests: usize,
    records: BrowserEpisodeRecords,
}

struct Desk {
    stage: Stage,
    adapter: Arc<AgentLoopChatCompletionAdapter>,
    model: Arc<Model>,
    bus: MessageBus,
    context: ExternalTransportRuntimeContext,
    gateway: GatewayDurableWork,
    wire: Arc<Wire>,
    ear: Arc<Ear>,
    /// How often the manager offered a continuation to the notifier.
    notified: Arc<AtomicUsize>,
    stop: Arc<AtomicBool>,
    processor: Option<Processor>,
    updates: AtomicU64,
    root: tempfile::TempDir,
}

impl Drop for Desk {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        self.model.free();
        if let Some(processor) = self.processor.take() {
            let _ = processor.join();
        }
    }
}

impl Desk {
    fn start(model: Arc<Model>) -> Result<Self, Box<dyn Error>> {
        Self::timed(model, Timing::default())
    }

    fn timed(model: Arc<Model>, timing: Timing) -> Result<Self, Box<dyn Error>> {
        let mut desk = Self::idle(model, timing)?;
        desk.run()?;
        Ok(desk)
    }

    /// A desk whose external channels do not run yet.
    fn idle(model: Arc<Model>, timing: Timing) -> Result<Self, Box<dyn Error>> {
        let root = tempfile::tempdir()?;
        let mut config = Config::default();
        config.agents.defaults.workspace =
            root.path().join("workspace").to_string_lossy().into_owned();
        config.permissions.set_mode(PermissionMode::Default);
        config.tools.browser.enable = true;
        let mut adapter = scripted_agent_loop_adapter(
            root.path(),
            serde_json::to_value(&config)?,
            ToolRegistry::new(),
            Vec::new(),
            PermissionMode::Default,
        )?;
        let stage = Stage::start(&root.path().join("config.json"), timing)?;
        let host: Arc<dyn BrowserHost> = Arc::new(stage.host.clone());
        let mut tools = ToolRegistry::new();
        tools.register(BrowserTool::new(host.clone()));
        adapter.tools = tools;
        adapter.client = model.clone();
        adapter.browser_host = Some(host.clone());
        adapter.native_image_input_supported = false;
        adapter.send_progress = false;
        adapter.defaults.max_tool_iterations = 4;
        let gateway = GatewayDurableWork::new(
            root.path().to_path_buf(),
            runtime_owner_id(std::process::id(), now_millis()),
        );
        Ok(Self {
            stage,
            adapter: Arc::new(adapter),
            model,
            bus: MessageBus::new(),
            context: ExternalTransportRuntimeContext::new(
                root.path().join("metadata"),
                SEND_ATTEMPTS,
            ),
            gateway,
            wire: Arc::new(Wire::default()),
            ear: Arc::new(Ear {
                host,
                told: Mutex::new(Vec::new()),
            }),
            notified: Arc::new(AtomicUsize::new(0)),
            stop: Arc::new(AtomicBool::new(false)),
            processor: None,
            updates: AtomicU64::new(0),
            root,
        })
    }

    /// Opens the durable line and starts the processor over it, the way the gateway does for
    /// its external channels.
    fn run(&mut self) -> TestResult {
        let durable_work = self
            .gateway
            .open_ingress(&mut self.context, &self.bus, None)?;
        let ear: Arc<dyn BrowserHost> = self.ear.clone();
        self.context.set_browser_host(Some(ear));
        let (notify, notified) = (
            browser_resume_notifier(&self.context, &self.bus),
            self.notified.clone(),
        );
        self.stage.host.set_resume_notifier(move |resume| {
            notified.fetch_add(1, Ordering::SeqCst);
            notify(resume)
        });
        let wire = self.wire.clone();
        let runner: ExternalTransportRunner = Arc::new(move |spec, _, outbound, stop, context| {
            let deliveries = context.metadata_path(spec.channel());
            while !stop.load(Ordering::SeqCst) {
                let paused = lock(&wire.paused);
                if !*paused {
                    drain_outbound(
                        &outbound,
                        context.send_attempts,
                        &stop,
                        Some(&deliveries),
                        context.browser_host(),
                        |message| wire.send(message),
                    );
                }
                drop(paused);
                thread::sleep(Duration::from_millis(5));
            }
        });
        self.stop = Arc::new(AtomicBool::new(false));
        let exited = Arc::new(AtomicBool::new(false));
        self.processor = Some(ExternalChannelSupervisor::spawn_processor(
            exited.clone(),
            {
                let (adapter, bus, context, stop) = (
                    self.adapter.clone(),
                    self.bus.clone(),
                    self.context.clone(),
                    self.stop.clone(),
                );
                move || {
                    let specs = vec![
                        ExternalTransportSpec::Telegram(TelegramRuntimeConfig {
                            token: "telegram-token".to_owned(),
                            poll_timeout_seconds: 1,
                            poll_limit: 1,
                        }),
                        ExternalTransportSpec::Slack(SlackRuntimeConfig {
                            app_token: "slack-app-token".to_owned(),
                            bot_token: "slack-bot-token".to_owned(),
                            channel_ids: Vec::new(),
                            allowed_senders: Vec::new(),
                        }),
                    ];
                    let hello = context.spec035_hello;
                    let channels = external_transport_channel_manager_with_runner(
                        specs,
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
            },
        ));
        Ok(())
    }

    /// Stops the processor the way the gateway's shutdown does.
    fn halt(&mut self) -> TestResult {
        self.stop.store(true, Ordering::SeqCst);
        let processor = self.processor.take().ok_or("processor already joined")?;
        processor
            .join()
            .map_err(|_| "external processor panicked")??;
        Ok(())
    }

    fn update(&self, chat: &str, text: &str, reply_to: Option<&str>) -> Value {
        let id = self.updates.fetch_add(1, Ordering::SeqCst) + 1;
        let mut message = json!({"message_id":id,"chat":{"id":chat},"from":{"id":"user"},
            "text":text});
        if let Some(reply_to) = reply_to {
            // Telegram numbers its messages.
            let replied = reply_to
                .parse::<u64>()
                .map_or(json!(reply_to), |id| json!(id));
            message["reply_to_message"] = json!({"message_id":replied});
        }
        json!({"update_id":id,"message":message})
    }

    /// A Telegram update reaches the ingress that feeds `bus`.
    fn arrive(&self, bus: &MessageBus, update: &Value) {
        let inbound = telegram_update_to_inbound(update).expect("an update with text");
        self.take_in(bus, inbound);
    }

    fn take_in(&self, bus: &MessageBus, inbound: InboundMessage) {
        let hints = self.context.metadata_path(&inbound.channel);
        assert!(
            publish_external_inbound_with_hint(bus, inbound, &hints, &self.context),
            "intake refused"
        );
    }

    fn say(&self, chat: &str, text: &str) {
        self.arrive(&self.bus, &self.update(chat, text, None));
    }

    fn reply(&self, chat: &str, text: &str, to: &str) {
        self.arrive(&self.bus, &self.update(chat, text, Some(to)));
    }

    fn sent(&self) -> Vec<Sent> {
        lock(&self.wire.line).sent.clone()
    }

    /// The next message `chat` was sent that begins with `start` and was not heard yet.
    fn hear(&self, chat: &str, start: &str) -> Result<Sent, Box<dyn Error>> {
        let deadline = Instant::now() + PATIENCE;
        loop {
            {
                let mut line = lock(&self.wire.line);
                let found = line
                    .sent
                    .iter_mut()
                    .find(|sent| !sent.heard && sent.chat == chat && sent.text.starts_with(start));
                if let Some(sent) = found {
                    sent.heard = true;
                    return Ok(sent.clone());
                }
            }
            if Instant::now() >= deadline {
                return Err(
                    format!("{chat} never heard {start:?}; sent {:#?}", self.sent()).into(),
                );
            }
            thread::sleep(Duration::from_millis(10));
        }
    }

    fn unheard(&self) -> Vec<Sent> {
        self.sent().into_iter().filter(|sent| !sent.heard).collect()
    }

    fn records(&self) -> BrowserEpisodeRecords {
        self.stage.host.episode_records()
    }

    /// The open episode once its notice was delivered.
    fn asked(&self) -> Result<BrowserOpenEpisode, Box<dyn Error>> {
        eventually("the notice of the open episode to be delivered", || {
            self.records().open.is_some_and(|open| open.asked)
        })?;
        Ok(self.records().open.ok_or("no open episode")?)
    }

    /// Asks room A's assistant to hand over, and waits for the question to be delivered.
    fn hand_over(&self, text: &str) -> Result<(BrowserOpenEpisode, Sent), Box<dyn Error>> {
        self.hand_over_in(ROOM_A, text)
    }

    fn hand_over_in(
        &self,
        room: &str,
        text: &str,
    ) -> Result<(BrowserOpenEpisode, Sent), Box<dyn Error>> {
        self.say(room, text);
        let question = self.hear(room, QUESTION)?;
        let open = self.asked()?;
        assert!(open.paused, "{open:?}");
        assert_eq!(open.notice_ids, std::slice::from_ref(&question.id));
        assert_eq!(question.notice.as_deref(), Some(open.episode.as_str()));
        Ok((open, question))
    }

    /// An episode that is open, whether or not its notice was delivered.
    fn opened(&self) -> Result<BrowserOpenEpisode, Box<dyn Error>> {
        eventually("an episode to open", || self.records().open.is_some())?;
        Ok(self.records().open.ok_or("no open episode")?)
    }

    /// `room` takes the browser over and gives it back. Returns the episode.
    fn cycle(&self, room: &str) -> Result<String, Box<dyn Error>> {
        self.say(room, "/browser takeover");
        self.hear(room, TAKEN)?;
        let open = self.asked()?;
        self.say(room, "/browser return");
        self.hear(room, RETURNED)?;
        Ok(open.episode)
    }

    /// Keeps `session` from starting a turn for as long as the reservation lives, so what
    /// the durable line has for it stays there.
    fn hold(&self, session: &str) -> Result<SessionTurnReservation, Box<dyn Error>> {
        let deadline = Instant::now() + PATIENCE;
        loop {
            if let Some(reservation) = self.adapter.session_turn_lock.try_reserve(session) {
                return Ok(reservation);
            }
            if Instant::now() >= deadline {
                return Err(format!("{session} stayed busy").into());
            }
            thread::sleep(Duration::from_millis(10));
        }
    }

    /// The delivery reports the transports made: the episode and the channel's message id.
    fn told(&self) -> Vec<(String, Option<String>)> {
        lock(&self.ear.told).clone()
    }

    fn notified(&self) -> usize {
        self.notified.load(Ordering::SeqCst)
    }

    fn attempts(&self) -> usize {
        lock(&self.wire.line).attempts
    }

    /// The message whose text begins with `start`, as the outbound worker last had it to
    /// send.
    fn outbound(&self, start: &str) -> Result<OutboundMessage, Box<dyn Error>> {
        Ok(lock(&self.wire.line)
            .tried
            .iter()
            .rev()
            .find(|message| message.content.starts_with(start))
            .cloned()
            .ok_or_else(|| format!("nothing that begins with {start:?} was tried"))?)
    }

    /// The message the outbound worker last tried to send.
    fn tried(&self) -> Result<OutboundMessage, Box<dyn Error>> {
        Ok(lock(&self.wire.line)
            .tried
            .last()
            .cloned()
            .ok_or("nothing was tried")?)
    }

    /// The outbound worker's delivery records in order: the digest of the text and how the
    /// delivery stood.
    fn deliveries(&self) -> Vec<(String, String)> {
        fs::read_to_string(self.context.metadata_path("telegram"))
            .ok()
            .and_then(|text| serde_json::from_str::<Value>(&text).ok())
            .and_then(|root| root["deliveries"].as_array().cloned())
            .unwrap_or_default()
            .iter()
            .map(|record| {
                let text = |key: &str| record[key].as_str().unwrap_or_default().to_owned();
                (text("content_sha256"), text("status"))
            })
            .collect()
    }

    /// The delivery records once there are `count` of them.
    fn recorded(&self, count: usize) -> Result<Vec<(String, String)>, Box<dyn Error>> {
        eventually(&format!("{count} delivery records"), || {
            self.deliveries().len() >= count
        })?;
        Ok(self.deliveries())
    }

    /// Waits until everything the durable line holds has ended.
    fn quiet(&self) -> TestResult {
        eventually("the durable line to empty", || {
            self.work()
                .is_ok_and(|work| work.iter().all(|(terminal, _)| *terminal))
        })
    }

    /// The results the session recorded for one tool call.
    fn results(&self, session: &str, tool_call_id: &str) -> Vec<Value> {
        self.messages(session)
            .into_iter()
            .filter(|message| message["role"] == "tool" && message["tool_call_id"] == tool_call_id)
            .collect()
    }

    fn waiting(&self, episode: &str) -> Option<BrowserEpisodeRecord> {
        self.records()
            .waiting
            .into_iter()
            .find(|record| record.episode == episode)
    }

    fn settled(&self, episode: &str) -> Option<BrowserEpisodeRecord> {
        self.records()
            .settled
            .into_iter()
            .find(|record| record.episode == episode)
    }

    fn requests(&self) -> usize {
        self.model.requests().len()
    }

    /// What the provider was last asked to answer.
    fn last_asked(&self) -> String {
        self.model.requests().last().map(last).unwrap_or_default()
    }

    fn messages(&self, session: &str) -> Vec<Value> {
        SessionManager::new(&self.adapter.workspace)
            .ok()
            .and_then(|sessions| sessions.read_session_file(session))
            .and_then(|session| session["messages"].as_array().cloned())
            .unwrap_or_default()
    }

    /// The call the session's turn is paused on.
    fn paused_call(&self, session: &str) -> Option<String> {
        pending_ask_user_id(&self.messages(session))
    }

    fn files(&self) -> Result<BTreeMap<PathBuf, Vec<u8>>, Box<dyn Error>> {
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

    fn seen(&self) -> Result<Seen, Box<dyn Error>> {
        Ok(Seen {
            files: self.files()?,
            requests: self.requests(),
            records: self.records(),
        })
    }

    fn turn_state(&self) -> (Vec<String>, Vec<String>, Option<bool>) {
        let turns = &self.adapter.session_turn_lock;
        (
            turns.busy_session_keys(),
            turns.active_session_keys(),
            turns
                .cancellation_token(SESSION_A)
                .map(|token| token.is_cancelled()),
        )
    }

    /// Whether each item of the line ended, and whether its cancellation was asked for.
    fn work(&self) -> Result<Vec<(bool, bool)>, Box<dyn Error>> {
        Ok(self
            .items()?
            .iter()
            .map(|item| {
                (
                    item.state.is_terminal(),
                    item.cancellation_requested_sequence.is_some(),
                )
            })
            .collect())
    }

    fn status(&self) -> Result<BrowserStatus, Box<dyn Error>> {
        Ok(self.stage.host.status()?)
    }

    fn line(&self) -> Result<DurableWorkDispatcher, Box<dyn Error>> {
        Ok(DurableWorkDispatcher::open(
            runtime_durable_event_root(self.root.path()),
            runtime_durable_work_payload_root(self.root.path()),
            MessageBus::new(),
            self.gateway.lease_owner_ref.clone(),
            DURABLE_WORK_LEASE_DURATION_MS,
        )?)
    }

    fn items(&self) -> Result<Vec<ReplayWorkItem>, Box<dyn Error>> {
        let (state, _) =
            durable_work_state_for_owner(self.root.path(), &self.gateway.lease_owner_ref)?;
        Ok(state.work.items.values().cloned().collect())
    }

    fn item(&self, work_id: &str) -> Option<ReplayWorkItem> {
        self.items()
            .ok()?
            .into_iter()
            .find(|item| item.work_id == work_id)
    }

    /// The continuations of ended episodes the durable line holds.
    fn resumes(&self) -> Result<Vec<ReplayWorkItem>, Box<dyn Error>> {
        Ok(self
            .items()?
            .into_iter()
            .filter(|item| item.work_id.starts_with(BROWSER_RESUME_WORK_PREFIX))
            .collect())
    }

    /// The stored messages whose text is `text`, as the line has them on disk.
    fn stored(&self, text: &str) -> Result<Vec<(ReplayWorkItem, Value)>, Box<dyn Error>> {
        let line = self.line()?;
        let mut stored = Vec::new();
        for item in self.items()? {
            let payload = line.read_payload_json(&item)?;
            if payload["content"] == text {
                stored.push((item, payload));
            }
        }
        Ok(stored)
    }

    /// The arrival stamps of the stored messages whose text is `text`, with where each
    /// item stands on the line.
    fn stamps(&self, text: &str) -> Result<Vec<(ReplayWorkState, Value)>, Box<dyn Error>> {
        Ok(self
            .stored(text)?
            .into_iter()
            .map(|(item, payload)| {
                (
                    item.state,
                    payload["metadata"][BROWSER_ARRIVAL_METADATA].clone(),
                )
            })
            .collect())
    }

    fn ended(&self, work_id: &str) -> Option<WorkTerminalKind> {
        self.item(work_id).and_then(|item| item.terminal_kind)
    }

    fn settles(&self, work_id: &str, kind: WorkTerminalKind) -> TestResult {
        eventually(&format!("{work_id} to end {kind:?}"), || {
            self.ended(work_id) == Some(kind)
        })
    }
}

fn resume_of(record: &BrowserEpisodeRecord) -> BrowserPendingResume {
    BrowserPendingResume {
        episode: record.episode.clone(),
        reason: record.reason.clone(),
        session_id: record.session.clone(),
        tool_call_id: record.tool_call_id.clone().unwrap_or_default(),
        channel: record.channel.clone(),
        chat_id: record.chat_id.clone(),
        reply_route: record.reply_route.clone(),
    }
}

fn resume_id(episode: &str) -> String {
    format!("{BROWSER_RESUME_WORK_PREFIX}{episode}")
}

/// A continuation of an episode its generation never opened.
fn unknown_resume(
    episode: &str,
    reason: &str,
    session: &str,
    room: &str,
    tool_call_id: &str,
) -> Result<BrowserPendingResume, Box<dyn Error>> {
    let (generation, _) = episode
        .rsplit_once('-')
        .ok_or("an episode is numbered within its generation")?;
    Ok(BrowserPendingResume {
        episode: format!("{generation}-99"),
        reason: reason.to_owned(),
        session_id: session.to_owned(),
        tool_call_id: tool_call_id.to_owned(),
        channel: TELEGRAM_CHANNEL.to_owned(),
        chat_id: room.to_owned(),
        reply_route: Map::new(),
    })
}

/// The text of a recorded tool result, which the session keeps as its parts in JSON.
fn written(message: &Value) -> String {
    let content = message["content"].as_str().unwrap_or_default();
    match serde_json::from_str::<Value>(content) {
        Ok(Value::Array(parts)) => parts
            .iter()
            .filter_map(|part| part["text"].as_str())
            .collect(),
        _ => content.to_owned(),
    }
}

/// The delivery records of one message that went the way `how` says.
fn went(text: &str, how: &[&str]) -> Vec<(String, String)> {
    how.iter()
        .map(|status| (sha256_hex(text), (*status).to_owned()))
        .collect()
}

fn episodes(records: &[BrowserEpisodeRecord]) -> Vec<&str> {
    records
        .iter()
        .map(|record| record.episode.as_str())
        .collect()
}

#[test]
fn browser_resume_reaches_paused_turn_through_bus_and_rejects_forgeries() -> TestResult {
    // Given: a `handoff` of room A's turn, paused on its delivered question.
    let mut desk = Desk::start(Model::new(assistant))?;
    let (first, _) = desk.hand_over("넘겨줘")?;
    assert_eq!(desk.paused_call(SESSION_A).as_deref(), Some("handoff-0"));

    // When: another room returns the browser by the episode's label.
    desk.say(ROOM_B, &format!("/browser return {}", first.label));

    // Then: the paused turn goes on and answers the room that handed over.
    desk.hear(ROOM_B, RETURNED)?;
    desk.hear(ROOM_A, "이어서: browser_returned")?;
    assert!(
        desk.last_asked().contains(ENDINGS[0]),
        "{}",
        desk.last_asked()
    );
    desk.settles(&resume_id(&first.episode), WorkTerminalKind::Succeeded)?;
    assert_eq!(desk.paused_call(SESSION_A), None);
    assert_eq!(desk.records().waiting, []);
    assert_eq!(
        desk.settled(&first.episode).map(|record| record.reason),
        Some("returned".to_owned())
    );

    // Given: a second handoff, and a processor that is down when its limit passes.
    let (second, _) = desk.hand_over("또 넘겨줘")?;
    desk.halt()?;
    let requests = desk.requests();

    // When: the limit passes, and the same continuation is put on the line once more.
    desk.stage.advance(LIMIT_MS);
    eventually("the continuation to be put on the line", || {
        desk.item(&resume_id(&second.episode)).is_some()
    })?;
    let waiting = desk
        .waiting(&second.episode)
        .ok_or("no continuation waits")?;
    desk.context
        .enqueue_browser_resume(&desk.bus, &resume_of(&waiting))?;

    // Then: the line holds it once, as plain data without the owner's marker.
    let resumes = desk.resumes()?;
    let held = resumes
        .iter()
        .filter(|item| item.work_id == resume_id(&second.episode))
        .collect::<Vec<_>>();
    assert_eq!(held.len(), 1, "{resumes:#?}");
    assert_eq!(held[0].state, ReplayWorkState::Pending);
    assert_eq!(
        held[0].dedupe_hint,
        Some(format!("{BROWSER_RESUME_HINT_PREFIX}{}", second.episode))
    );
    let payload = desk.line()?.read_payload_json(held[0])?;
    assert_eq!(payload["metadata"]["injected_event"], BROWSER_RESUME_EVENT);
    assert!(!payload.to_string().contains("owner_accepted"), "{payload}");
    assert_eq!(desk.requests(), requests);

    // When: a processor starts again and reads the line from disk.
    desk.run()?;

    // Then: the marker is attached again and the turn goes on, once.
    desk.hear(ROOM_A, "이어서: browser_takeover_timeout")?;
    assert!(
        desk.last_asked().contains(ENDINGS[1]),
        "{}",
        desk.last_asked()
    );
    desk.settles(&resume_id(&second.episode), WorkTerminalKind::Succeeded)?;
    assert_eq!(desk.requests(), requests + 1);
    assert_eq!(desk.resumes()?.len(), 2);
    assert!(
        desk.items()?
            .iter()
            .all(|item| item.work_kind != AUTOMATION_WORK_KIND),
        "a continuation was recorded as an automation result"
    );

    // Given: a third handoff that stays open and paused.
    let (third, _) = desk.hand_over("다시 넘겨줘")?;
    let call = desk
        .paused_call(SESSION_A)
        .ok_or("the turn is not paused")?;
    let before = desk.seen()?;
    let forged = BrowserPendingResume {
        episode: third.episode.clone(),
        reason: "returned".to_owned(),
        session_id: SESSION_A.to_owned(),
        tool_call_id: call.clone(),
        channel: TELEGRAM_CHANNEL.to_owned(),
        chat_id: ROOM_A.to_owned(),
        reply_route: Map::new(),
    };
    let marked = |event: &str| {
        let mut message = telegram_update_to_inbound(&desk.update(ROOM_A, "돌려줬어", None))
            .expect("an update with text");
        for (key, value) in [
            ("injected_event", event),
            ("episode", third.episode.as_str()),
            ("tool_call_id", call.as_str()),
            ("reason", "returned"),
        ] {
            message
                .metadata
                .insert(key.to_owned(), Value::String(value.to_owned()));
        }
        message
    };

    // When: a channel message dressed as the continuation arrives.
    let dressed = marked(BROWSER_RESUME_EVENT);
    assert_eq!(
        automation_producer::enqueue_result(&desk.adapter, &dressed),
        Err(FORGERY.to_owned())
    );
    desk.take_in(&desk.bus, dressed);
    // And: line items that carry the hint under another work id, or the work id under
    // another episode's hint.
    let (generation, _) = third
        .episode
        .rsplit_once('-')
        .ok_or("an episode is numbered within its generation")?;
    let mut line = desk.line()?;
    line.enqueue_inbound(
        "work-forged-resume",
        &browser_resume_inbound(&forged),
        Some(format!("{BROWSER_RESUME_HINT_PREFIX}{}", third.episode)),
        None,
    )?;
    line.enqueue_inbound(
        resume_id(&third.episode),
        &browser_resume_inbound(&forged),
        Some(format!("{BROWSER_RESUME_HINT_PREFIX}{generation}-99")),
        None,
    )?;
    // And: an injected event of another name without a marker.
    let other = marked("subagent_result");
    assert_eq!(
        automation_producer::enqueue_result(&desk.adapter, &other),
        Err(FORGERY.to_owned())
    );
    desk.take_in(&desk.bus, other);

    // Then: every one of them is refused before the loop, and nothing moved.
    desk.settles("work-forged-resume", WorkTerminalKind::Failed)?;
    desk.settles(&resume_id(&third.episode), WorkTerminalKind::Failed)?;
    eventually("the dressed channel messages to be refused", || {
        desk.stored("돌려줬어").is_ok_and(|stored| {
            stored.len() == 2
                && stored
                    .iter()
                    .all(|(item, _)| item.terminal_kind == Some(WorkTerminalKind::Failed))
        })
    })?;
    assert_eq!(desk.seen()?, before);
    assert_eq!(desk.paused_call(SESSION_A), Some(call));

    // And: the paused turn still takes its answer.
    desk.say(ROOM_A, "다 했어");
    desk.hear(ROOM_A, "이어서: browser_handoff_answer")?;
    assert_eq!(
        desk.settled(&third.episode).map(|record| record.reason),
        Some("answered".to_owned())
    );
    assert_eq!(desk.unheard(), []);
    Ok(())
}

#[test]
fn takeover_and_return_bypass_busy_session_through_external_ingress() -> TestResult {
    // Given: a turn of room A held inside the provider and another message queued behind it.
    let model = Model::new(|_, request| {
        let last = last(request);
        if last.contains("browser_user_control") {
            answer("사용자가 쓰는 중이라 보지 못했어요")
        } else if last.contains("화면 봐줘") {
            call("look-0", "browser", json!({"action":"look"}))
        } else {
            answer("답했어요")
        }
    });
    let desk = Desk::start(model.clone())?;
    model.hold();
    desk.say(ROOM_A, "화면 봐줘");
    eventually("the first turn to reach the provider", || {
        desk.requests() == 1
    })?;
    desk.say(ROOM_A, "줄에 선 글");
    assert_eq!(desk.work()?, [(false, false); 2]);
    let files = desk.files()?;
    let turns = desk.turn_state();
    assert_eq!(
        turns,
        (
            vec![SESSION_A.to_owned()],
            vec![SESSION_A.to_owned()],
            Some(false)
        )
    );

    // When: the same room takes the browser over.
    desk.say(ROOM_A, "/browser takeover");

    // Then: the answer comes while the turn is still held, and nothing of the session moved.
    let taken = desk.hear(ROOM_A, TAKEN)?;
    let open = desk.asked()?;
    assert_eq!(taken.notice.as_deref(), Some(open.episode.as_str()));
    assert!(!open.paused, "{open:?}");
    assert_eq!(desk.status()?.control, BrowserControl::User);
    assert_eq!(desk.requests(), 1);
    assert_eq!(desk.turn_state(), turns);
    assert_eq!(desk.files()?, files);

    // When: the held turn goes on to use the browser.
    model.pass();
    eventually("the turn to come back with its tool result", || {
        desk.requests() == 2
    })?;

    // Then: its call is refused, because the user controls the browser.
    assert!(
        desk.last_asked().contains("browser_user_control"),
        "{}",
        desk.last_asked()
    );
    assert_eq!(desk.turn_state(), turns);
    let files = desk.files()?;

    // When: the same room returns the browser.
    desk.say(ROOM_A, "/browser return");

    // Then: that answer too comes before the turn ends.
    desk.hear(ROOM_A, RETURNED)?;
    assert_eq!(desk.records().open, None);
    assert_eq!(
        desk.settled(&open.episode).map(|record| record.reason),
        Some("returned".to_owned())
    );
    assert_eq!(desk.status()?.control, BrowserControl::Assistant);
    assert_eq!(desk.requests(), 2);
    assert_eq!(desk.turn_state(), turns);
    assert_eq!(desk.files()?, files);
    eventually("the two commands to finish", || {
        desk.work().is_ok_and(|work| {
            work.iter().filter(|(terminal, _)| *terminal).count() == 2
                && work.iter().all(|(_, cancelled)| !cancelled)
        })
    })?;
    assert_eq!(desk.unheard(), []);

    // When: the provider answers.
    model.free();

    // Then: the running turn and the queued message end in the order sent, uncancelled.
    desk.hear(ROOM_A, "사용자가 쓰는 중이라 보지 못했어요")?;
    desk.hear(ROOM_A, "답했어요")?;
    eventually("every message to finish", || {
        desk.work().is_ok_and(|work| work == [(true, false); 4])
    })?;
    assert_eq!(desk.requests(), 3);
    assert!(
        desk.last_asked().contains("줄에 선 글"),
        "{}",
        desk.last_asked()
    );
    assert_eq!(desk.resumes()?, []);
    for bytes in desk.files()?.values() {
        assert!(!String::from_utf8_lossy(bytes).contains("/browser"));
    }
    assert_eq!(desk.unheard(), []);
    Ok(())
}

#[test]
fn resumed_handoff_turn_stops_reply_wait_clock() -> TestResult {
    // Given: a reply wait shorter than the idle timeout, which Spec 049 goes on counting
    // while a turn runs.
    const REPLY_WAIT_MS: u64 = 5 * 60_000;
    let model = Model::new(assistant);
    let desk = Desk::timed(
        model.clone(),
        Timing {
            reply_wait_minutes: 5,
            idle_timeout_seconds: 1_800,
            ..Timing::default()
        },
    )?;
    // And: a paused `handoff` whose episode ends at its limit, and a provider that holds
    // the turn that goes on from it.
    let (first, _) = desk.hand_over("넘겨줘")?;
    model.hold();
    desk.stage.advance(LIMIT_MS);
    eventually("the continued turn to reach the provider", || {
        desk.requests() == 2
    })?;
    assert!(
        desk.last_asked().contains(ENDINGS[1]),
        "{}",
        desk.last_asked()
    );
    assert_eq!(desk.status()?.control, BrowserControl::Assistant);

    // When: more than the reply wait passes while that turn runs.
    desk.stage.advance(REPLY_WAIT_MS + 60_000);

    // Then: the browser stays open and no reply is waited for.
    let running = desk.status()?;
    assert_eq!(running.state, BrowserState::Running, "{running:?}");
    assert_eq!(running.reply_wait_deadline_ms, None);

    // When: the turn ends.
    model.free();
    desk.hear(ROOM_A, "이어서: browser_takeover_timeout")?;
    desk.settles(&resume_id(&first.episode), WorkTerminalKind::Succeeded)?;

    // Then: the reply wait counts from that moment.
    let ended = desk.stage.now();
    eventually("the reply wait to start", || {
        desk.status()
            .is_ok_and(|status| status.reply_wait_deadline_ms.is_some())
    })?;
    assert_eq!(
        desk.status()?.reply_wait_deadline_ms,
        Some(ended + REPLY_WAIT_MS)
    );

    // When: a continuation the loop discards arrives a while later.
    desk.stage.advance(60_000);
    let (generation, _) = first
        .episode
        .rsplit_once('-')
        .ok_or("an episode is numbered within its generation")?;
    let unknown = BrowserPendingResume {
        episode: format!("{generation}-99"),
        reason: "limit".to_owned(),
        session_id: SESSION_A.to_owned(),
        tool_call_id: "handoff-0".to_owned(),
        channel: TELEGRAM_CHANNEL.to_owned(),
        chat_id: ROOM_A.to_owned(),
        reply_route: Map::new(),
    };
    let before = desk.seen()?;
    desk.context.enqueue_browser_resume(&desk.bus, &unknown)?;
    eventually("the unknown continuation to leave the line", || {
        desk.ended(&resume_id(&unknown.episode)).is_some()
    })?;

    // Then: it started no turn, and the wait still counts from the end of the real one.
    assert_eq!(desk.seen()?, before);
    desk.stage.upkeep();
    assert_eq!(
        desk.status()?.reply_wait_deadline_ms,
        Some(ended + REPLY_WAIT_MS)
    );
    desk.stage.advance(REPLY_WAIT_MS - 60_000 - 1_000);
    assert_eq!(desk.status()?.state, BrowserState::Running);

    // When: the reply wait is over.
    desk.stage.advance(1_000);

    // Then: the browser closes for that reason.
    let closed = desk.status()?;
    assert_eq!(closed.state, BrowserState::Failed, "{closed:?}");
    assert!(
        closed
            .reason
            .as_deref()
            .is_some_and(|reason| reason.starts_with("browser_reply_wait_timeout")),
        "{closed:?}"
    );
    assert_eq!(closed.reply_wait_deadline_ms, None);
    Ok(())
}

#[test]
fn unresumed_episode_record_survives_later_episodes() -> TestResult {
    const REPLY_WAIT_MS: u64 = 30 * 60_000;
    // Given: room A's `handoff` episode ended at its limit while room A cannot start a
    // turn, so its continuation stays on the durable line.
    let desk = Desk::start(Model::new(assistant))?;
    let (first, _) = desk.hand_over("넘겨줘")?;
    let held = desk.hold(SESSION_A)?;
    desk.stage.advance(LIMIT_MS);
    eventually("the continuation to wait on the line", || {
        desk.item(&resume_id(&first.episode))
            .is_some_and(|item| item.state == ReplayWorkState::Pending)
    })?;
    let requests = desk.requests();

    // When: room B opens and ends nine episodes.
    let mut others = Vec::new();
    for _ in 0..9 {
        others.push(desk.cycle(ROOM_B)?);
    }

    // Then: only the last eight of those are kept, and the record that waits is not one
    // of them.
    let records = desk.records();
    assert_eq!(episodes(&records.waiting), [first.episode.as_str()]);
    assert_eq!(episodes(&records.settled), others[1..]);
    assert_eq!(desk.paused_call(SESSION_A).as_deref(), Some("handoff-0"));
    assert_eq!(desk.requests(), requests);

    // When: the continuation is let through.
    drop(held);

    // Then: room A's paused turn goes on with the limit, and room A is answered.
    desk.hear(ROOM_A, "이어서: browser_takeover_timeout")?;
    assert!(
        desk.last_asked().contains(ENDINGS[1]),
        "{}",
        desk.last_asked()
    );
    desk.settles(&resume_id(&first.episode), WorkTerminalKind::Succeeded)?;
    assert_eq!(desk.requests(), requests + 1);
    // And: its record moved behind the others, within the limit of eight.
    let records = desk.records();
    assert_eq!(records.waiting, []);
    let mut kept = others[2..].iter().map(String::as_str).collect::<Vec<_>>();
    kept.push(&first.episode);
    assert_eq!(episodes(&records.settled), kept);

    // When: the same continuation is put on the line again.
    let before = desk.seen()?;
    let again = desk
        .settled(&first.episode)
        .ok_or("the first episode is not kept")?;
    desk.context
        .enqueue_browser_resume(&desk.bus, &resume_of(&again))?;
    desk.stage.upkeep();

    // Then: the line still holds it once, and no turn started.
    assert_eq!(desk.resumes()?.len(), 1);
    desk.quiet()?;
    assert_eq!(desk.seen()?, before);

    // Given: room A paused on a second `handoff`, and a made-up continuation for that
    // call that waits on the line ahead of the real one.
    let (second, _) = desk.hand_over("또 넘겨줘")?;
    let call = desk.paused_call(SESSION_A).ok_or("room A is not paused")?;
    let held = desk.hold(SESSION_A)?;
    let made_up = unknown_resume(&second.episode, "limit", SESSION_A, ROOM_A, &call)?;
    desk.context.enqueue_browser_resume(&desk.bus, &made_up)?;
    desk.stage.advance(LIMIT_MS);
    eventually("the second continuation to wait on the line", || {
        desk.item(&resume_id(&second.episode))
            .is_some_and(|item| item.state == ReplayWorkState::Pending)
    })?;
    // And: that browser closed after its reply wait, and room C paused on a `handoff` of
    // the next one, which room B returns while room C cannot start a turn.
    desk.stage.advance(REPLY_WAIT_MS + 1_000);
    let closed = desk.status()?;
    assert_eq!(closed.state, BrowserState::Failed, "{closed:?}");
    let (third, _) = desk.hand_over_in(ROOM_C, "넘겨줘")?;
    let held_c = desk.hold(SESSION_C)?;
    desk.say(ROOM_B, &format!("/browser return {}", third.label));
    desk.hear(ROOM_B, RETURNED)?;
    eventually("room C's continuation to wait on the line", || {
        desk.item(&resume_id(&third.episode))
            .is_some_and(|item| item.state == ReplayWorkState::Pending)
    })?;
    let between = desk.cycle(ROOM_B)?;

    // Then: both records wait, whatever ended in between.
    let records = desk.records();
    assert_eq!(
        episodes(&records.waiting),
        [second.episode.as_str(), third.episode.as_str()]
    );
    assert_eq!(
        records.settled.last().map(|record| record.episode.as_str()),
        Some(between.as_str())
    );
    let requests = desk.requests();

    // When: room C is let through.
    drop(held_c);

    // Then: its turn goes on with its own ending, and room A's record still waits.
    desk.hear(ROOM_C, "이어서: browser_returned")?;
    desk.settles(&resume_id(&third.episode), WorkTerminalKind::Succeeded)?;
    assert_eq!(episodes(&desk.records().waiting), [second.episode.as_str()]);
    assert_eq!(desk.requests(), requests + 1);

    // When: room A is let through, the made-up continuation first.
    drop(held);

    // Then: the made-up one is thrown away without moving the record, so the real one
    // still continues room A's turn.
    desk.hear(ROOM_A, "이어서: browser_takeover_timeout")?;
    desk.settles(&resume_id(&second.episode), WorkTerminalKind::Succeeded)?;
    assert!(desk.ended(&resume_id(&made_up.episode)).is_some());
    assert_eq!(desk.requests(), requests + 2);
    assert_eq!(desk.records().waiting, []);
    for (episode, reason) in [(&second.episode, "limit"), (&third.episode, "returned")] {
        assert_eq!(
            desk.settled(episode).map(|record| record.reason),
            Some(reason.to_owned())
        );
    }

    // When: eight episodes in all have ended after the first one continued.
    for _ in 0..5 {
        desk.cycle(ROOM_B)?;
    }

    // Then: the first record made room for them.
    assert_eq!(desk.settled(&first.episode), None);
    assert_eq!(desk.records().settled.len(), 8);
    assert_eq!(desk.unheard(), []);
    Ok(())
}

#[test]
fn handoff_opening_failure_gives_one_result_and_no_resume() -> TestResult {
    const NOTICE: &str = "browser_closed: 브라우저가 닫혔습니다";
    let cases = [
        (
            "the new websockify never listens",
            Faults {
                proxy_deaf_beside_input: true,
                ..Faults::default()
            },
            "failed",
        ),
        (
            "the view-only x11vnc cannot be confirmed gone",
            Faults {
                view_only_unreapable: true,
                ..Faults::default()
            },
            "closed",
        ),
        (
            "the input x11vnc cannot be reaped while undoing",
            Faults {
                proxy_deaf_beside_input: true,
                input_unreapable: true,
                ..Faults::default()
            },
            "closed",
        ),
    ];
    for (name, faults, reason) in cases {
        // Given: an assistant that hands over, and once more when it is told the browser
        // closed; and viewer processes that fail the switch.
        let desk = Desk::start(Model::new(|index, request| {
            let last = last(request);
            if last.contains(r#""role":"tool""#) && last.contains(NOTICE) {
                return handoff(index);
            }
            assistant(index, request)
        }))?;
        desk.stage.fault(faults);

        // When: the assistant hands over.
        desk.say(ROOM_A, "넘겨줘");

        // Then: the turn is not paused. The call has one result, which the same turn's
        // next request carried.
        desk.hear(ROOM_A, "이어서: browser_handoff_unavailable")?;
        desk.quiet()?;
        assert_eq!(desk.requests(), 2, "{name}");
        assert_eq!(desk.paused_call(SESSION_A), None, "{name}");
        let results = desk.results(SESSION_A, "handoff-0");
        assert_eq!(results.len(), 1, "{name}: {results:#?}");
        let result = written(&results[0]);
        assert!(
            result.starts_with(ENDINGS[6]) && result.ends_with(')'),
            "{name}: {result}"
        );
        assert!(
            desk.last_asked().contains(&result),
            "{name}: {}",
            desk.last_asked()
        );
        // And: nothing is left to continue.
        assert_eq!(desk.notified(), 0, "{name}");
        assert_eq!(desk.resumes()?, [], "{name}");
        let records = desk.records();
        assert_eq!(
            (records.open, records.waiting),
            (None, Vec::new()),
            "{name}"
        );
        let failed = records.settled.last().ok_or("the episode is not kept")?;
        assert_eq!(
            (
                failed.reason.as_str(),
                failed.paused,
                failed.tool_call_id.as_deref(),
                failed.session.as_str()
            ),
            (reason, false, Some("handoff-0"), SESSION_A),
            "{name}"
        );
        assert_eq!(records.settled.len(), 1, "{name}");

        // When: more than the takeover limit passes.
        let before = desk.seen()?;
        desk.stage.advance(LIMIT_MS + 60_000);
        desk.quiet()?;

        // Then: no second result, no new turn, no request.
        assert_eq!(desk.seen()?, before, "{name}");
        assert_eq!(desk.notified(), 0, "{name}");
        assert_eq!(desk.unheard(), [], "{name}");

        // When: a continuation for that very episode, call and reason is put on the line.
        let made_up = resume_of(failed);
        assert_eq!(made_up.tool_call_id, "handoff-0", "{name}");
        desk.context.enqueue_browser_resume(&desk.bus, &made_up)?;

        // Then: the loop throws it away.
        eventually("the made-up continuation to leave the line", || {
            desk.ended(&resume_id(&failed.episode)).is_some()
        })?;
        assert_eq!(desk.seen()?, before, "{name}");
        assert_eq!(desk.results(SESSION_A, "handoff-0").len(), 1, "{name}");

        // When: the same session hands over once the viewers work again.
        desk.stage.fault(Faults::default());
        let (open, _) = desk.hand_over("또 넘겨줘")?;

        // Then: the turn pauses as usual, and at the limit it is continued once.
        let call = desk
            .paused_call(SESSION_A)
            .ok_or("the turn is not paused")?;
        assert_eq!(desk.notified(), 0, "{name}");
        desk.stage.advance(LIMIT_MS);
        desk.hear(ROOM_A, "이어서: browser_takeover_timeout")?;
        desk.settles(&resume_id(&open.episode), WorkTerminalKind::Succeeded)?;
        assert_eq!(desk.notified(), 1, "{name}");
        assert_eq!(desk.resumes()?.len(), 2, "{name}");
        assert_eq!(desk.results(SESSION_A, &call).len(), 1, "{name}");
        assert_eq!(desk.paused_call(SESSION_A), None, "{name}");
        assert_eq!(desk.records().waiting, [], "{name}");
        assert_eq!(desk.unheard(), [], "{name}");
    }
    Ok(())
}

#[test]
fn handoff_question_is_asked_only_after_outbound_delivery() -> TestResult {
    // Given: an outbound worker that is not sending.
    let desk = Desk::start(Model::new(assistant))?;
    desk.wire.pause(true);

    // When: the assistant hands over, and the user's "다 했어" arrives meanwhile.
    desk.say(ROOM_A, "넘겨줘");
    eventually("the turn to pause on its question", || {
        desk.paused_call(SESSION_A).is_some()
    })?;
    desk.quiet()?;
    let queued = desk.opened()?;
    desk.say(ROOM_A, "다 했어");
    desk.quiet()?;

    // Then: the channel took the question, but it is not asked and nothing answers it.
    assert!(
        queued.paused && !queued.asked && queued.notice_ids.is_empty(),
        "{queued:?}"
    );
    assert_eq!(desk.records().open, Some(queued.clone()));
    assert_eq!(
        (desk.attempts(), desk.told(), desk.deliveries()),
        (0, Vec::new(), Vec::new())
    );
    assert_eq!(desk.requests(), 1);

    // When: the worker sends.
    desk.wire.pause(false);

    // Then: the question is asked under the id Telegram gave it, and the early message
    // was told why it was not an answer.
    let question = desk.hear(ROOM_A, QUESTION)?;
    desk.hear(ROOM_A, BEFORE_NOTICE)?;
    let asked = desk.asked()?;
    assert_eq!(asked.notice_ids, std::slice::from_ref(&question.id));
    assert_eq!(
        desk.told(),
        [(queued.episode.clone(), Some(question.id.clone()))]
    );
    assert_eq!(
        desk.recorded(4)?,
        [
            went(&question.text, &["pending", "sent"]),
            went(BEFORE_NOTICE, &["pending", "sent"])
        ]
        .concat()
    );
    // And: a message that arrives now answers it.
    desk.say(ROOM_A, "다 했어");
    desk.hear(ROOM_A, "이어서: browser_handoff_answer")?;

    // Given: a Telegram that refuses every send.
    lock(&desk.wire.line).failing = true;
    let (attempts, told) = (desk.attempts(), desk.told());
    let recorded = desk.recorded(6)?.len();

    // When: the assistant hands over again.
    desk.say(ROOM_A, "또 넘겨줘");
    let failed = desk.recorded(recorded + 2)?;

    // Then: the delivery failed after its retries, nothing was reported and the question
    // is not asked.
    let notice = desk.tried()?;
    assert!(notice.content.starts_with(QUESTION), "{notice:?}");
    assert_eq!(
        failed[recorded..],
        went(&notice.content, &["pending", "failed"])
    );
    assert_eq!(desk.attempts(), attempts + SEND_ATTEMPTS);
    assert_eq!(desk.told(), told);
    let unasked = desk.opened()?;
    assert!(
        unasked.paused && !unasked.asked && unasked.notice_ids.is_empty(),
        "{unasked:?}"
    );

    // When: a worker that was told to stop takes the same question, with a Telegram that
    // would accept it.
    lock(&desk.wire.line).failing = false;
    let (queue, stopped) = mpsc::channel();
    queue.send(notice.clone())?;
    drain_outbound(
        &stopped,
        SEND_ATTEMPTS,
        &Arc::new(AtomicBool::new(true)),
        Some(&desk.context.metadata_path("telegram")),
        desk.context.browser_host(),
        |message| desk.wire.send(message),
    );

    // Then: it gave up without sending, and that too is no delivery.
    assert_eq!(
        desk.deliveries()[recorded + 2..],
        went(&notice.content, &["pending", "failed"])
    );
    assert_eq!(desk.attempts(), attempts + SEND_ATTEMPTS);
    assert_eq!(desk.told(), told);
    assert_eq!(desk.records().open, Some(unasked.clone()));
    desk.say(ROOM_B, &format!("/browser return {}", unasked.label));
    desk.hear(ROOM_B, RETURNED)?;
    desk.hear(ROOM_A, "이어서: browser_returned")?;

    // Given: a Telegram that answers a send with success but without a message id.
    lock(&desk.wire.line).nameless = true;
    let (told, sent) = (desk.told(), desk.sent());
    let recorded = desk.recorded(recorded + 8)?.len();

    // When: the assistant hands over once more.
    desk.say(ROOM_A, "다시 넘겨줘");
    let failed = desk.recorded(recorded + 2)?;

    // Then: that is a failed delivery.
    let notice = desk.tried()?;
    assert_eq!(
        failed[recorded..],
        went(&notice.content, &["pending", "failed"])
    );
    assert_eq!((desk.told(), desk.sent()), (told, sent));
    let unasked = desk.opened()?;
    assert!(
        !unasked.asked && unasked.notice_ids.is_empty(),
        "{unasked:?}"
    );
    lock(&desk.wire.line).nameless = false;
    desk.say(ROOM_B, &format!("/browser return {}", unasked.label));
    desk.hear(ROOM_B, RETURNED)?;
    desk.hear(ROOM_A, "이어서: browser_returned")?;

    // Given: a worker that is not sending, a `/browser takeover` that was returned by its
    // label, and a second one that is open.
    desk.wire.pause(true);
    let told = desk.told();
    desk.say(ROOM_B, "/browser takeover");
    let first = desk.opened()?;
    desk.say(ROOM_B, &format!("/browser return {}", first.label));
    eventually("the first takeover to end", || {
        desk.settled(&first.episode).is_some()
    })?;
    desk.say(ROOM_B, "/browser takeover");
    eventually("the second takeover to open", || {
        desk.records()
            .open
            .is_some_and(|open| open.episode != first.episode)
    })?;
    desk.quiet()?;
    let second = desk.opened()?;
    assert!(!first.asked && first.notice_ids.is_empty(), "{first:?}");
    assert!(!second.asked && second.notice_ids.is_empty(), "{second:?}");
    assert_eq!(desk.told(), told);

    // When: the worker sends all three answers.
    desk.wire.pause(false);
    let early = desk.hear(ROOM_B, TAKEN)?;
    desk.hear(ROOM_B, RETURNED)?;
    let current = desk.hear(ROOM_B, TAKEN)?;

    // Then: the answer of the open takeover makes it asked under its own id, and the
    // delivery of the ended one changed nothing.
    let asked = desk.asked()?;
    assert_eq!(
        (asked.episode.as_str(), asked.notice_ids.as_slice()),
        (second.episode.as_str(), [current.id.clone()].as_slice())
    );
    assert_eq!(
        desk.told()[told.len()..],
        [
            (first.episode.clone(), Some(early.id.clone())),
            (second.episode.clone(), Some(current.id.clone()))
        ]
    );
    // And: neither does a late report for the ended episode or for one that never was.
    let before = desk.records();
    desk.ear
        .episode_notice_delivered(&first.episode, Some("late"));
    let never = unknown_resume(&second.episode, "limit", SESSION_A, ROOM_A, "none")?;
    desk.ear
        .episode_notice_delivered(&never.episode, Some("never"));
    assert_eq!(desk.records(), before);
    desk.say(ROOM_B, "/browser return");
    desk.hear(ROOM_B, RETURNED)?;
    assert_eq!(desk.unheard(), []);

    // Given: a WebSocket chat, and no browser another session is using.
    assert_ne!(
        desk.stage.host.close_by_user(),
        BrowserCloseOutcome::NothingOpen
    );
    eventually("the browser to close", || {
        desk.status()
            .is_ok_and(|status| status.state == BrowserState::Idle)
    })?;
    let frame = |text: &str| json!({"type":"message","chat_id":"ws-room","text":text});
    let mut in_sink = Vec::new();

    // When: its assistant hands over and the question is written to the connection.
    desk.adapter
        .process_websocket_frame_events(frame("넘겨줘"), "client", "ws-room", &mut |event| {
            if matches!(&event, WebSocketServerEvent::Message { text, .. }
                if text.starts_with(QUESTION))
            {
                in_sink.push(desk.records().open.map(|open| open.asked));
            }
            true
        })
        .map_err(|error| error.to_string())?;

    // Then: it was not asked while the event only sat in the sink, and is once written.
    assert_eq!(in_sink, [Some(false)]);
    let written = desk.opened()?;
    assert!(
        written.paused && written.asked && written.notice_ids.is_empty(),
        "{written:?}"
    );
    let mut answers = Vec::new();
    desk.adapter
        .process_websocket_frame_events(frame("다 했어"), "client", "ws-room", &mut |event| {
            if let WebSocketServerEvent::Message { text, .. } = event {
                answers.push(text);
            }
            true
        })
        .map_err(|error| error.to_string())?;
    assert_eq!(answers, ["이어서: browser_handoff_answer"]);
    assert_eq!(
        desk.settled(&written.episode).map(|record| record.reason),
        Some("answered".to_owned())
    );

    // When: the write of the next question fails.
    desk.adapter
        .process_websocket_frame_events(frame("또 넘겨줘"), "client", "ws-room", &mut |event| {
            !matches!(&event, WebSocketServerEvent::Message { text, .. }
                    if text.starts_with(QUESTION))
        })
        .map_err(|error| error.to_string())?;

    // Then: it stays unasked.
    let unwritten = desk.opened()?;
    assert_ne!(unwritten.episode, written.episode);
    assert!(unwritten.paused && !unwritten.asked, "{unwritten:?}");
    Ok(())
}

#[test]
fn stale_answer_and_queued_return_stay_bound_to_their_episode() -> TestResult {
    let stamp = |episode: &str, reply_to: Option<&str>| json!({"episode":episode,"asked":true,"reply_to":reply_to});
    let model = Model::new(assistant);
    let mut desk = Desk::start(model.clone())?;

    // Given: a `handoff` episode with its delivered question.
    let (first, first_question) = desk.hand_over("넘겨줘")?;

    // When: a reply to some other message arrives, with a stamp of its own that claims to
    // answer the question.
    let mut dressed =
        telegram_update_to_inbound(&desk.update(ROOM_A, "표시를 달고 온 글", Some("77")))
            .expect("an update with text");
    dressed.metadata.insert(
        BROWSER_ARRIVAL_METADATA.to_owned(),
        stamp(&first.episode, Some(&first_question.id)),
    );
    desk.take_in(&desk.bus, dressed);

    // Then: the ingress wrote its own stamp over that one, with the message Telegram said
    // it replies to, so it is no answer.
    desk.hear(ROOM_A, NOT_THIS_NOTICE)?;
    assert_eq!(
        desk.stamps("표시를 달고 온 글")?,
        [(ReplayWorkState::Terminal, stamp(&first.episode, Some("77")))]
    );
    assert_eq!(desk.records().open, Some(first.clone()));

    // Given: three updates sent during that episode which Telegram hands on late.
    let late_answer = desk.update(ROOM_A, "다 했어", Some(&first_question.id));
    let late_return = desk.update(ROOM_A, "/browser return", Some(&first_question.id));
    let late_plain = desk.update(ROOM_A, "늦게 닿은 다 했어", None);
    // And: the episode ended at its limit and the same room is in a second one, whose
    // question was delivered.
    desk.stage.advance(LIMIT_MS);
    desk.hear(ROOM_A, "이어서: browser_takeover_timeout")?;
    let (second, _) = desk.hand_over("또 넘겨줘")?;
    let before = desk.seen()?;

    // When: the two replies to the first question arrive.
    desk.arrive(&desk.bus, &late_answer);
    desk.arrive(&desk.bus, &late_return);

    // Then: neither ends the second episode, and each is told why.
    desk.hear(ROOM_A, NOT_THIS_NOTICE)?;
    desk.hear(ROOM_A, RETURN_REPLY_MISMATCH)?;
    desk.quiet()?;
    assert_eq!(desk.seen()?, before);

    // When: the message that replies to nothing arrives.
    desk.arrive(&desk.bus, &late_plain);

    // Then: it arrived during the second episode, so it answers that one.
    desk.hear(ROOM_A, "이어서: browser_handoff_answer")?;
    assert_eq!(
        desk.stamps("늦게 닿은 다 했어")?,
        [(ReplayWorkState::Terminal, stamp(&second.episode, None))]
    );
    assert_eq!(
        desk.settled(&second.episode).map(|record| record.reason),
        Some("answered".to_owned())
    );

    // When: `/browser return` replies to the question of the episode that is open.
    let (third, third_question) = desk.hand_over("다시 넘겨줘")?;
    desk.reply(ROOM_A, "/browser return", &third_question.id);

    // Then: that episode ends.
    desk.hear(ROOM_A, RETURNED)?;
    desk.hear(ROOM_A, "이어서: browser_returned")?;
    assert_eq!(
        desk.settled(&third.episode).map(|record| record.reason),
        Some("returned".to_owned())
    );
    desk.quiet()?;

    // Given: a turn of room A held in the provider while the room took the browser over.
    let requests = desk.requests();
    model.hold();
    desk.say(ROOM_A, "한 번 더 넘겨줘");
    eventually("the held turn to reach the provider", || {
        desk.requests() == requests + 1
    })?;
    desk.say(ROOM_A, "/browser takeover");
    desk.hear(ROOM_A, TAKEN)?;
    let fourth = desk.asked()?;
    // And: a "다 끝났어" that waits on the line behind that turn, and a `/browser return`
    // the line handed to a bus nobody reads.
    desk.say(ROOM_A, "다 끝났어");
    let aside = MessageBus::new();
    desk.arrive(&aside, &desk.update(ROOM_A, "/browser return", None));
    assert_eq!(
        desk.stamps("다 끝났어")?,
        [(ReplayWorkState::Pending, stamp(&fourth.episode, None))]
    );
    let leased = |desk: &Desk| -> Result<Vec<Value>, Box<dyn Error>> {
        Ok(desk
            .stamps("/browser return")?
            .into_iter()
            .filter(|(state, _)| *state == ReplayWorkState::Leased)
            .map(|(_, stamp)| stamp)
            .collect())
    };
    assert_eq!(leased(&desk)?, [stamp(&fourth.episode, None)]);
    // And: that takeover ended at its limit, and the held turn handed over.
    desk.stage.advance(LIMIT_MS);
    eventually("the takeover to end at its limit", || {
        desk.settled(&fourth.episode).is_some()
    })?;
    model.free();
    desk.hear(ROOM_A, QUESTION)?;
    let fifth = desk.asked()?;
    assert!(fifth.paused, "{fifth:?}");
    assert_eq!(
        desk.stamps("다 끝났어")?,
        [(ReplayWorkState::Pending, stamp(&fourth.episode, None))]
    );
    let before = desk.seen()?;

    // When: the line's two messages are let out.
    desk.bus
        .publish_inbound(aside.try_consume_inbound().ok_or("nothing was held")?);

    // Then: neither ends the episode that is open now. The return is told its takeover
    // is over, and the answer that it came before this question.
    desk.hear(ROOM_A, OVER)?;
    desk.hear(ROOM_A, BEFORE_NOTICE)?;
    desk.quiet()?;
    assert_eq!(desk.seen()?, before);
    desk.say(ROOM_A, "다 했어");
    desk.hear(ROOM_A, "이어서: browser_handoff_answer")?;
    desk.quiet()?;

    // Given: a sixth episode paused on a `handoff`, again with a leased `/browser return`
    // and a "줄에 선 다 했어" behind it.
    let (sixth, _) = desk.hand_over("넘겨줘, 마지막으로")?;
    let aside = MessageBus::new();
    desk.arrive(&aside, &desk.update(ROOM_A, "/browser return", None));
    desk.say(ROOM_A, "줄에 선 다 했어");
    assert_eq!(leased(&desk)?, [stamp(&sixth.episode, None)]);
    // And: it ended at its limit, and the room took the browser over by itself.
    desk.stage.advance(LIMIT_MS);
    eventually("the continuation to wait on the line", || {
        desk.item(&resume_id(&sixth.episode))
            .is_some_and(|item| item.state == ReplayWorkState::Pending)
    })?;
    desk.say(ROOM_A, "/browser takeover");
    desk.hear(ROOM_A, TAKEN)?;
    let seventh = desk.asked()?;
    assert_eq!(
        desk.stamps("줄에 선 다 했어")?,
        [(ReplayWorkState::Pending, stamp(&sixth.episode, None))]
    );
    let requests = desk.requests();

    // When: the processor stops and another one reads the line from disk.
    desk.halt()?;
    drop(aside);
    desk.run()?;

    // Then: the return is told its takeover is over, and the answer closes the paused
    // call as over. The takeover that is open stays open.
    desk.hear(ROOM_A, OVER)?;
    desk.hear(ROOM_A, "이어서: browser_takeover_over")?;
    desk.quiet()?;
    assert!(
        desk.last_asked().contains(ENDINGS[5]),
        "{}",
        desk.last_asked()
    );
    assert_eq!(desk.requests(), requests + 1);
    assert_eq!(desk.records().open, Some(seventh.clone()));
    assert_eq!(desk.records().waiting, []);
    assert_eq!(
        desk.settled(&sixth.episode).map(|record| record.reason),
        Some("limit".to_owned())
    );
    assert_eq!(desk.paused_call(SESSION_A), None);

    // And: the room's own return ends it.
    desk.say(ROOM_A, "/browser return");
    desk.hear(ROOM_A, RETURNED)?;
    assert_eq!(
        desk.settled(&seventh.episode).map(|record| record.reason),
        Some("returned".to_owned())
    );
    assert_eq!(desk.unheard(), []);
    Ok(())
}

#[test]
fn due_dispatch_waits_for_priority_intake_in_progress() -> TestResult {
    // Given: a line no processor reads, with a message that waits on it.
    let mut desk = Desk::start(Model::new(assistant))?;
    desk.halt()?;
    desk.say(ROOM_A, "줄에 선 글");
    let waits = |desk: &Desk| -> Result<Vec<ReplayWorkState>, Box<dyn Error>> {
        Ok(desk.items()?.into_iter().map(|item| item.state).collect())
    };
    assert_eq!(waits(&desk)?, [ReplayWorkState::Pending]);
    // And: an intake that is between putting its command on the line and dispatching it.
    let intake = DURABLE_INBOUND_ENQUEUE_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .map_err(|_| "the intake lock is poisoned")?;

    // When: the processor's dispatch pass runs meanwhile.
    let mut line = desk.line()?;
    let root = desk.root.path().to_path_buf();
    let (entered, enters) = mpsc::channel();
    let (left, leaves) = mpsc::channel();
    let pass = thread::spawn(move || {
        let _ = entered.send(());
        let passed = dispatch_due_external_work(&mut line, &root, &BTreeSet::new())
            .map_err(|error| error.to_string());
        let _ = left.send(());
        passed
    });
    enters.recv_timeout(PATIENCE)?;

    // Then: it does not get through, and leases nothing, until the intake is through.
    assert!(leaves.recv_timeout(Duration::from_millis(200)).is_err());
    assert_eq!(waits(&desk)?, [ReplayWorkState::Pending]);
    drop(intake);
    pass.join().map_err(|_| "the dispatch pass panicked")??;
    assert_eq!(waits(&desk)?, [ReplayWorkState::Leased]);
    Ok(())
}
