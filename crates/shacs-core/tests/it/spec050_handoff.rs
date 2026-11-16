use crate::spec048_auto_mode::harness::{agent_loop_config, answer, RoutingProvider};
use crate::spec048_auto_mode::support::{
    self, Exec, Options, Rules, Setup, TestResult, Verdict, ALL_MODES,
};
use crate::spec050_conversational_choice::{
    agent, browser, content, open, tool_result, turn_edges, Host, BROWSER_GUIDANCE, DESCRIPTION,
    GUIDANCE, OWNER,
};
use serde_json::{json, Map, Value};
use shacs_config::{RememberedPermissionEffect, RememberedPermissionMatcher};
use shacs_core::runtime::{
    decide_permission, evaluate_static_rules, session_remembered_context_digest,
    stamp_browser_arrival, AgentLoop, AgentLoopCommandResult, AgentLoopConfig,
    ApprovalDecisionKind, BrowserArrival, BrowserCallContext, BrowserCapture, BrowserCloseOutcome,
    BrowserClosureNotice, BrowserControlDescription, BrowserDescription, BrowserError,
    BrowserHandoffEpisode, BrowserHost, BrowserPendingResume, BrowserPreparedAction,
    BrowserReturnOutcome, BrowserReturnTarget, BrowserSession, BrowserTakeover,
    BrowserTakeoverDescription, BrowserTakeoverError, BrowserTakeoverRequest,
    BrowserViewDescription, InboundMessage, MessageBus, OutboundMessage,
    OwnerAcceptedBrowserResume, PermissionMode, PermissionPolicyDecision,
    PermissionPolicyDecisionKind, PermissionPolicyInput, PermissionPolicyReason, RuntimeInterrupt,
    RuntimeToolCall, RuntimeToolExecutionReport, SafetyCapability, SessionManager,
    SessionRememberedPermissionRule, StaticRuleDecisionKind, StaticRuleReason,
    ToolExecutionContext, BROWSER_EPISODE_NOTICE_METADATA, BROWSER_RESUME_EVENT,
};
use shacs_core::tools::{AskUserTool, BrowserTool, Tool, ToolRegistry};
use shacs_providers::LlmResponse;
use std::sync::{Arc, Mutex, MutexGuard};

const NONCE: &str = "9f2c41d07be35a68";
const VIEW: &str = "https://view.example/shacs-view.html";
const MINUTES: u64 = 15;
const REASON: &str = "로그인이 필요합니다";
const ASKED: &str = "브라우저 넘겨줘";
const NOT_THIS_NOTICE: &str = "이 메시지는 다른 메시지에 단 답글이라 지금 넘겨받기의 답으로 쓰지 않았습니다. 끝나면 지금 넘겨받기 안내에 답글을 달거나 답글 없이 답해 주세요.";
const BEFORE_NOTICE: &str = "이 메시지는 넘겨받기 안내가 가기 전에 보낸 것이라 답으로 쓰지 않았습니다. 끝나면 다시 답해 주세요.";
const RETURN_REPLY_MISMATCH: &str = "답글로 보낸 /browser return은 답글이 가리키는 안내의 넘겨받기만 돌려줍니다. 그 넘겨받기는 지금 열려 있지 않습니다. 지금 넘겨받기를 돌려주려면 답글 없이 /browser return을 보내 주세요.";
const RETURN_NOT_ASKED: &str = "넘겨받기 안내가 전달되기 전에 보낸 /browser return이라 돌려주지 않았습니다. 다시 보내거나 /browser return <회차 표시>를 쓰세요. 회차 표시는 /browser status에 있습니다.";
const RETURNED: &str = "브라우저를 돌려줬습니다.";
const NOT_TAKEN: &str = "넘겨받은 상태가 아닙니다.";
const OVER: &str = "그 넘겨받기는 이미 끝났습니다.";
const USAGE: &str = "사용법: /browser status | takeover | return [회차] | close";
const ANSWERED: &str =
    "browser_handoff_answer: 사용자가 답했고 브라우저를 돌려받았습니다. 다시 look하세요.";
const TAKEOVER_OVER: &str = "browser_takeover_over: 넘겨받기는 이미 끝났습니다. 다시 look하세요.";
const STOPPED: &str = "browser_cancelled: 실행이 중단되었습니다";
const ENDINGS: [(&str, &str); 4] = [
    (
        "returned",
        "browser_returned: 사용자가 돌려줬습니다. 다시 look하세요.",
    ),
    (
        "limit",
        "browser_takeover_timeout: 넘겨받기 시간이 지났습니다. 다시 look하세요.",
    ),
    (
        "closed",
        "browser_closed: 넘겨받은 동안 브라우저가 닫혔습니다. 페이지를 다시 여세요.",
    ),
    (
        "view_failed",
        "browser_view_failed: 화면 보기가 꺼져 돌려받았습니다. 다시 look하세요.",
    ),
];

#[derive(Clone)]
struct Episode {
    id: String,
    label: String,
    session: String,
    channel: String,
    chat_id: String,
    route: Map<String, Value>,
    call: Option<String>,
    asked: bool,
    notices: Vec<String>,
}

#[derive(Default)]
struct Control {
    view_off: Option<String>,
    opened: usize,
    open: Option<Episode>,
    waiting: Vec<BrowserPendingResume>,
    ended: Vec<(String, String)>,
    log: Vec<String>,
    targets: Vec<BrowserReturnTarget>,
}

impl Control {
    fn finish(&mut self, reason: &str) -> Option<BrowserPendingResume> {
        let episode = self.open.take()?;
        self.ended.push((episode.id.clone(), reason.to_owned()));
        let call = episode.call?;
        if matches!(reason, "answered" | "cancelled") {
            return None;
        }
        let record = BrowserPendingResume {
            episode: episode.id,
            reason: reason.to_owned(),
            session_id: episode.session,
            tool_call_id: call,
            channel: episode.channel,
            chat_id: episode.chat_id,
            reply_route: episode.route,
        };
        self.waiting.push(record.clone());
        Some(record)
    }
}

#[derive(Clone, Default)]
struct Desk {
    page: Host,
    control: Arc<Mutex<Control>>,
}

impl Desk {
    fn control(&self) -> MutexGuard<'_, Control> {
        self.control.lock().expect("desk control lock poisoned")
    }

    fn taken(&self) -> bool {
        self.control().open.is_some()
    }

    fn open_episode(&self) -> Option<String> {
        self.control()
            .open
            .as_ref()
            .map(|episode| episode.id.clone())
    }

    fn log(&self) -> Vec<String> {
        self.control().log.clone()
    }

    fn ended(&self) -> Vec<(String, String)> {
        self.control().ended.clone()
    }

    fn last_target(&self) -> Option<BrowserReturnTarget> {
        self.control().targets.last().cloned()
    }

    fn end(&self, reason: &str) -> Option<BrowserPendingResume> {
        let record = self.control().finish(reason);
        if reason == "closed" {
            self.page.state().owner = None;
        }
        record
    }

    fn free(&self) -> Result<(), BrowserError> {
        if self.taken() {
            Err(BrowserError::UserControl)
        } else {
            Ok(())
        }
    }
}

impl BrowserHost for Desk {
    fn close_prepared(
        &self,
        context: &BrowserCallContext,
        digest: &str,
    ) -> Result<(), BrowserError> {
        self.free()?;
        self.page.close_prepared(context, digest)
    }

    fn prepare(
        &self,
        session_id: &str,
        arguments: &Value,
    ) -> Result<BrowserPreparedAction, BrowserError> {
        self.free()?;
        self.page.prepare(session_id, arguments)
    }

    fn acquire(
        &self,
        context: &BrowserCallContext,
    ) -> Result<Box<dyn BrowserSession>, BrowserError> {
        self.free()?;
        self.page.acquire(context)
    }

    fn capture_prepared(
        &self,
        context: &BrowserCallContext,
        digest: &str,
    ) -> Result<BrowserCapture, BrowserError> {
        self.free()?;
        self.page.capture_prepared(context, digest)
    }

    fn turn_started(&self, session_id: &str) {
        self.page.turn_started(session_id);
    }

    fn turn_finished(&self, session_id: &str) {
        self.page.turn_finished(session_id);
    }

    fn closure_notice(&self, session_id: &str) -> Option<BrowserClosureNotice> {
        self.page.closure_notice(session_id)
    }

    fn acknowledge_closure_notice(&self, session_id: &str, token: &str) {
        self.page.acknowledge_closure_notice(session_id, token);
    }

    fn close_for_session(&self, session_id: &str) -> bool {
        self.control()
            .log
            .push(format!("close_for_session {session_id}"));
        self.page.close_for_session(session_id)
    }

    fn close_by_user(&self) -> BrowserCloseOutcome {
        self.page.close_by_user()
    }

    fn describe(&self) -> BrowserDescription {
        let mut description = self.page.describe();
        let control = self.control();
        description.control = BrowserControlDescription {
            takeover: control
                .open
                .as_ref()
                .map(|episode| BrowserTakeoverDescription {
                    session: episode.session.clone(),
                    label: episode.label.clone(),
                    remaining_minutes: MINUTES,
                }),
            view: control.view_off.clone().map_or_else(
                || BrowserViewDescription::Open(VIEW.to_owned()),
                BrowserViewDescription::Off,
            ),
        };
        description
    }

    fn takeover(
        &self,
        request: &BrowserTakeoverRequest,
    ) -> Result<BrowserTakeover, BrowserTakeoverError> {
        let taken = {
            let mut control = self.control();
            if control.open.is_some() {
                return Err(BrowserTakeoverError::AlreadyTaken);
            }
            if let Some(reason) = control.view_off.clone() {
                return Err(BrowserTakeoverError::ViewUnavailable(reason));
            }
            control.opened += 1;
            let episode = Episode {
                id: format!("{NONCE}-{}", control.opened),
                label: format!("{}-{}", &NONCE[..4], control.opened),
                session: request.session_id.clone(),
                channel: request.channel.clone(),
                chat_id: request.chat_id.clone(),
                route: request.reply_route.clone(),
                call: request.tool_call_id.clone(),
                asked: false,
                notices: Vec::new(),
            };
            control.open = Some(episode.clone());
            BrowserTakeover {
                episode: episode.id,
                label: episode.label,
                view_url: VIEW.to_owned(),
                minutes: MINUTES,
            }
        };
        let mut page = self.page.state();
        if page.owner.is_none() {
            page.owner = Some(request.session_id.clone());
        }
        Ok(taken)
    }

    fn episode_notice_delivered(&self, episode: &str, message_id: Option<&str>) {
        let mut control = self.control();
        if let Some(open) = control.open.as_mut().filter(|open| open.id == episode) {
            open.asked = true;
            open.notices.extend(message_id.map(str::to_owned));
        }
    }

    fn arrival(&self) -> BrowserArrival {
        let control = self.control();
        BrowserArrival {
            episode: control.open.as_ref().map(|episode| episode.id.clone()),
            asked: control.open.as_ref().is_some_and(|episode| episode.asked),
        }
    }

    fn handoff_episode(&self, session_id: &str, tool_call_id: &str) -> BrowserHandoffEpisode {
        self.control()
            .open
            .as_ref()
            .filter(|open| open.session == session_id && open.call.as_deref() == Some(tool_call_id))
            .map_or(BrowserHandoffEpisode::Over, |open| {
                BrowserHandoffEpisode::Open {
                    episode: open.id.clone(),
                    channel: open.channel.clone(),
                    chat_id: open.chat_id.clone(),
                    notice_ids: open.notices.clone(),
                }
            })
    }

    fn finish_handoff(&self, session_id: &str, tool_call_id: &str, episode: &str) -> bool {
        let mut control = self.control();
        control.log.push(format!(
            "finish_handoff {session_id} {tool_call_id} {episode}"
        ));
        let matches = control.open.as_ref().is_some_and(|open| {
            open.id == episode
                && open.session == session_id
                && open.call.as_deref() == Some(tool_call_id)
        });
        if matches {
            control.finish("answered");
        }
        matches
    }

    fn return_takeover(&self, target: &BrowserReturnTarget) -> BrowserReturnOutcome {
        let mut control = self.control();
        control.targets.push(target.clone());
        let open = control.open.clone();
        let outcome = match target {
            BrowserReturnTarget::Label(label) => {
                if open.is_some_and(|open| &open.label == label) {
                    BrowserReturnOutcome::Returned
                } else {
                    BrowserReturnOutcome::Over
                }
            }
            BrowserReturnTarget::Arrival {
                channel,
                chat_id,
                reply_to: Some(reply_to),
                ..
            } => {
                if open.is_some_and(|open| {
                    &open.channel == channel
                        && &open.chat_id == chat_id
                        && open.notices.contains(reply_to)
                }) {
                    BrowserReturnOutcome::Returned
                } else {
                    BrowserReturnOutcome::ReplyMismatch
                }
            }
            BrowserReturnTarget::Arrival {
                reply_to: None,
                arrival,
                ..
            } => match &arrival.episode {
                None => BrowserReturnOutcome::NotTaken,
                Some(arrived) if open.is_some_and(|open| &open.id == arrived) => {
                    if arrival.asked {
                        BrowserReturnOutcome::Returned
                    } else {
                        BrowserReturnOutcome::NotAsked
                    }
                }
                Some(_) => BrowserReturnOutcome::Over,
            },
        };
        if outcome == BrowserReturnOutcome::Returned {
            control.finish("returned");
        }
        outcome
    }

    fn cancel_takeover(&self, session_id: &str) {
        let mut control = self.control();
        control.log.push(format!("cancel_takeover {session_id}"));
        if control
            .open
            .as_ref()
            .is_some_and(|open| open.session == session_id)
        {
            control.finish("cancelled");
        }
        control
            .waiting
            .retain(|record| record.session_id != session_id);
    }

    fn pending_resume(&self, episode: &str) -> Option<BrowserPendingResume> {
        self.control()
            .waiting
            .iter()
            .find(|record| record.episode == episode)
            .cloned()
    }

    fn resume_settled(&self, episode: &str) {
        let mut control = self.control();
        control.log.push(format!("resume_settled {episode}"));
        control.waiting.retain(|record| record.episode != episode);
    }

    fn handoff_closed(&self, session_id: &str, tool_call_id: &str) {
        let mut control = self.control();
        control
            .log
            .push(format!("handoff_closed {session_id} {tool_call_id}"));
        control.waiting.retain(|record| {
            record.session_id != session_id || record.tool_call_id != tool_call_id
        });
    }
}

#[derive(Debug, Clone)]
struct Sent {
    id: String,
    message: OutboundMessage,
}

impl Sent {
    fn episode(&self) -> Option<&str> {
        self.message
            .metadata
            .get(BROWSER_EPISODE_NOTICE_METADATA)
            .and_then(Value::as_str)
    }

    fn stop_reason(&self) -> Option<&str> {
        self.message
            .metadata
            .get("stop_reason")
            .and_then(Value::as_str)
    }

    fn room(&self) -> (&str, &str) {
        (&self.message.channel, &self.message.chat_id)
    }
}

struct Room {
    bus: MessageBus,
    desk: Desk,
    sent: usize,
}

impl Room {
    fn new(desk: &Desk) -> Self {
        Self {
            bus: MessageBus::new(),
            desk: desk.clone(),
            sent: 0,
        }
    }

    fn deliver(&mut self) -> Vec<Sent> {
        let mut delivered = Vec::new();
        while let Some(message) = self.bus.consume_outbound() {
            self.sent += 1;
            let sent = Sent {
                id: (100 + self.sent).to_string(),
                message,
            };
            if let Some(episode) = sent.episode() {
                self.desk
                    .episode_notice_delivered(episode, Some(sent.id.as_str()));
            }
            delivered.push(sent);
        }
        delivered
    }

    fn deliver_one(&mut self) -> Result<Sent, Box<dyn std::error::Error>> {
        let mut delivered = self.deliver();
        if delivered.len() != 1 {
            return Err(format!("expected one sent message: {delivered:?}").into());
        }
        Ok(delivered.remove(0))
    }

    fn through(
        &self,
        message: InboundMessage,
    ) -> Result<InboundMessage, Box<dyn std::error::Error>> {
        self.bus.publish_inbound(message);
        Ok(self.bus.consume_inbound().ok_or("inbound on the bus")?)
    }
}

fn user(chat: &str, text: &str, reply_to: Option<&str>) -> InboundMessage {
    let mut message = InboundMessage::new("telegram", "user-1", chat, text);
    if let Some(reply_to) = reply_to {
        message
            .metadata
            .insert("reply_to_message_id".to_owned(), json!(reply_to));
    }
    message
}

/// A message as the durable queue hands it over: stamped when it arrived,
/// processed later.
fn queued(desk: &Desk, mut message: InboundMessage) -> InboundMessage {
    stamp_browser_arrival(Some(desk as &dyn BrowserHost), &mut message);
    message
        .metadata
        .insert("durable_work_id".to_owned(), json!("work-queued"));
    message
}

fn unmarked(record: &BrowserPendingResume) -> InboundMessage {
    let mut metadata = Map::new();
    metadata.insert("injected_event".to_owned(), json!(BROWSER_RESUME_EVENT));
    metadata.insert("episode".to_owned(), json!(record.episode));
    metadata.insert("tool_call_id".to_owned(), json!(record.tool_call_id));
    metadata.insert("reason".to_owned(), json!(record.reason));
    InboundMessage::new("system", "browser", record.chat_id.clone(), "")
        .with_session_key_override(record.session_id.clone())
        .with_metadata(metadata)
}

fn resume(record: &BrowserPendingResume) -> InboundMessage {
    unmarked(record).with_owner_accepted_browser_resume(OwnerAcceptedBrowserResume {
        episode: record.episode.clone(),
        tool_call_id: record.tool_call_id.clone(),
        reason: record.reason.clone(),
    })
}

fn config(setup: &Setup, desk: &Desk, mode: PermissionMode) -> AgentLoopConfig {
    let mut config = agent_loop_config(setup, true);
    if mode != PermissionMode::Auto {
        let context = setup.context(&Options::mode(mode, true));
        config.permission_mode_snapshot = context.permission_mode_snapshot;
        config.permission_ceiling_snapshot = context.permission_ceiling_snapshot;
        config.permission_auto_approval = context.permission_auto_approval;
    }
    config.browser_host = Some(Arc::new(desk.clone()));
    config
}

fn tools(setup: &Setup, desk: &Desk) -> ToolRegistry {
    let (mut registry, _) = setup.registry(Exec::Gated);
    registry.register(BrowserTool::new(Arc::new(desk.clone())));
    registry
}

fn handoff(id: &str) -> LlmResponse {
    browser(id, json!({"action":"handoff","reason":REASON}))
}

fn question(label: &str) -> String {
    format!(
        "브라우저를 넘깁니다: {REASON}\n화면 보기: {VIEW}\n화면 보기 비밀번호로 연결하세요. 한글이나 긴 글은 화면 아래 붙여넣기 칸을 쓰세요.\n끝나면 이 메시지에 답해 주세요(예: \"다 했어\"). 지금 화면을 볼 수 없으면 \"나중에\"라고 답해 이 단계를 넘길 수 있습니다.\n다른 대화방에서는 /browser return {label}로 돌려줄 수 있습니다.\n비밀번호나 인증 코드는 대화로 보내지 마세요.\n{MINUTES}분이 지나면 자동으로 돌려받습니다."
    )
}

fn session(setup: &Setup, key: &str) -> Option<Value> {
    SessionManager::new(&setup.workspace)
        .ok()?
        .read_session_file(key)
}

fn session_messages(setup: &Setup, key: &str) -> Vec<Value> {
    session(setup, key)
        .and_then(|session| session["messages"].as_array().cloned())
        .unwrap_or_default()
}

fn stored_result(setup: &Setup, call: &str) -> Option<Value> {
    session_messages(setup, OWNER)
        .into_iter()
        .find(|message| message["role"] == "tool" && message["tool_call_id"] == call)
}

fn seen(setup: &Setup, desk: &Desk, provider: &RoutingProvider) -> Value {
    let control = desk.control();
    json!({
        "session": session(setup, OWNER),
        "requests": provider.assistant_requests().len(),
        "classified": provider.payloads().len(),
        "turns": turn_edges(&desk.page),
        "log": control.log,
        "ended": control.ended,
        "open": control.open.as_ref().map(|episode| episode.id.clone()),
        "waiting": control
            .waiting
            .iter()
            .map(|record| record.episode.clone())
            .collect::<Vec<_>>(),
    })
}

/// A refused answer is still the owner's message: the host hears one turn
/// start and finish, which it does not count while the user has control.
fn with_idle_turn(turns: &Value) -> Value {
    let mut turns = turns.as_array().cloned().unwrap_or_default();
    turns.push(json!(format!("turn_started {OWNER}")));
    turns.push(json!(format!("turn_finished {OWNER}")));
    Value::Array(turns)
}

fn classifier_users(provider: &RoutingProvider, from: usize) -> Vec<String> {
    provider.payloads()[from..]
        .iter()
        .filter_map(|payload| payload["transcript"].as_array())
        .flatten()
        .filter_map(|entry| entry["user"].as_str())
        .map(str::to_owned)
        .collect()
}

fn answered_handoff(mode: PermissionMode, text: &str, replied: bool) -> TestResult {
    let label = format!("{mode:?} {text} replied {replied}");
    let auto = mode == PermissionMode::Auto;
    let setup = Setup::new()?;
    let desk = Desk::default();
    let registry = tools(&setup, &desk);
    let provider = if auto {
        RoutingProvider::new(
            vec![
                open("open"),
                handoff("hand"),
                browser("look", json!({"action":"look"})),
                browser("pay", json!({"action":"click","element":2,"revision":"r2"})),
                answer("결제 단계입니다."),
            ],
            vec![Ok("no"); 3],
        )
    } else {
        RoutingProvider::new(
            vec![
                handoff("hand"),
                browser("pay", json!({"action":"click","element":2,"revision":"r0"})),
            ],
            Vec::new(),
        )
    };
    let mut room = Room::new(&desk);
    let mut agent = agent(
        &setup,
        &room.bus,
        &registry,
        &provider,
        config(&setup, &desk, mode),
    )?;

    let paused = agent.process_message(user("chat-1", ASKED, None))?;

    assert_eq!(paused.stop_reason, "ask_user", "{label}: {paused:?}");
    let episode = desk.open_episode().ok_or("open episode")?;
    let notice = room.deliver_one()?;
    assert_eq!(notice.message.content, question("9f2c-1"), "{label}");
    assert!(notice.message.buttons.is_empty(), "{label}");
    assert!(paused.ask_user_options.is_empty(), "{label}");
    assert_eq!(notice.room(), ("telegram", "chat-1"), "{label}");
    assert_eq!(notice.episode(), Some(episode.as_str()), "{label}");
    assert_eq!(provider.payloads().len(), usize::from(auto), "{label}");
    assert!(stored_result(&setup, "hand").is_none(), "{label}");
    let metadata = session(&setup, OWNER).ok_or("session")?["metadata"].clone();
    assert!(metadata["pending_permission_approval"].is_null(), "{label}");
    assert!(
        metadata["session_permission_approvals"].is_null(),
        "{label}"
    );
    let classified = provider.payloads().len();

    let reply_to = replied.then_some(notice.id.as_str());
    agent.process_message(user("chat-1", text, reply_to))?;

    assert_eq!(
        desk.log(),
        [format!("finish_handoff {OWNER} hand {episode}")],
        "{label}"
    );
    assert_eq!(desk.ended(), [(episode, "answered".to_owned())], "{label}");
    let result = stored_result(&setup, "hand").ok_or("tool message")?;
    assert_eq!(result["name"], "browser", "{label}");
    assert_eq!(content(&result), format!("{ANSWERED}\n{text}"), "{label}");
    assert_eq!(tool_result(&provider, "hand"), content(&result), "{label}");
    let stored = session_messages(&setup, OWNER);
    assert!(
        stored
            .iter()
            .filter(|message| message["role"] == "user")
            .all(|message| !content(message).contains(text)),
        "{label}: {stored:?}"
    );
    let requests = provider.assistant_requests();
    let last = requests.last().ok_or("assistant request")?;
    assert!(
        last.messages
            .iter()
            .filter(|message| content(message).contains(text))
            .all(|message| message["role"] == "tool"),
        "{label}"
    );
    let users = classifier_users(&provider, classified);
    assert!(
        users.iter().all(|user| !user.contains(text)),
        "{label}: {users:?}"
    );
    let after = room.deliver();
    if auto {
        assert_eq!(provider.payloads().len(), 3, "{label}");
        assert_eq!(desk.page.state().clicks, ["결제하기"], "{label}");
    } else {
        let asked = after.last().ok_or("approval question")?;
        assert_eq!(asked.stop_reason(), Some("permission_approval"), "{label}");
        assert!(desk.page.state().clicks.is_empty(), "{label}");
    }
    Ok(())
}

#[test]
fn handoff_pauses_turn_and_any_answer_returns_control() -> TestResult {
    for mode in [PermissionMode::Auto, PermissionMode::Default] {
        for (text, replied) in [("다 했어", false), ("나중에", false), ("다 했어", true)] {
            answered_handoff(mode, text, replied)?;
        }
    }

    // Without a screen viewer the call is an ordinary result and the turn goes on.
    let setup = Setup::new()?;
    let desk = Desk::default();
    desk.control().view_off = Some("password_missing".to_owned());
    let registry = tools(&setup, &desk);
    let provider = RoutingProvider::new(
        vec![handoff("hand"), answer("화면 보기를 먼저 켜 주세요.")],
        Vec::new(),
    );
    let room = Room::new(&desk);
    let turn = agent(
        &setup,
        &room.bus,
        &registry,
        &provider,
        config(&setup, &desk, PermissionMode::Default),
    )?
    .process_message(user("chat-1", ASKED, None))?;

    assert_eq!(
        turn.final_content.as_deref(),
        Some("화면 보기를 먼저 켜 주세요.")
    );
    let refused = tool_result(&provider, "hand");
    assert!(
        refused
            .contains("browser_handoff_unavailable: 화면 보기를 쓸 수 없습니다(password_missing)"),
        "{refused}"
    );
    assert_eq!(desk.control().opened, 0);
    assert!(desk.log().is_empty() && desk.ended().is_empty());
    Ok(())
}

fn forgeries(record: &BrowserPendingResume) -> Vec<(&'static str, InboundMessage)> {
    let with = |change: fn(&mut BrowserPendingResume)| {
        let mut forged = record.clone();
        change(&mut forged);
        resume(&forged)
    };
    let mut sender = resume(record);
    sender.sender_id = "user-1".to_owned();
    let mut from_channel = unmarked(record);
    from_channel.channel = "telegram".to_owned();
    from_channel.sender_id = "user-1".to_owned();
    let other_reason = if record.reason == "limit" {
        "returned"
    } else {
        "limit"
    };
    let mut reason = record.clone();
    reason.reason = other_reason.to_owned();
    vec![
        ("no mark", unmarked(record)),
        ("no mark, from a channel", from_channel),
        (
            "other call",
            with(|forged| forged.tool_call_id = "other".to_owned()),
        ),
        (
            "unknown episode",
            with(|forged| forged.episode = format!("{NONCE}-99")),
        ),
        (
            "other session",
            with(|forged| forged.session_id = "telegram:chat-9".to_owned()),
        ),
        ("other sender", sender),
        ("other reason", resume(&reason)),
        (
            "answered is not a continuation",
            with(|forged| forged.reason = "answered".to_owned()),
        ),
    ]
}

#[test]
fn handoff_resumes_from_other_room_return_and_limit() -> TestResult {
    for (reason, text) in ENDINGS {
        let setup = Setup::new()?;
        let desk = Desk::default();
        let registry = tools(&setup, &desk);
        let provider = RoutingProvider::new(
            vec![handoff("hand"), open("again"), answer("이어서 했습니다.")],
            vec![Ok("no")],
        );
        let mut room = Room::new(&desk);
        let mut agent = agent(
            &setup,
            &room.bus,
            &registry,
            &provider,
            config(&setup, &desk, PermissionMode::Auto),
        )?;
        // The turn that hands over answers in a topic and a thread of its room.
        let route = [
            ("message_thread_id", json!("7")),
            ("slack", json!({"thread_ts":"1700000000.000100"})),
        ];
        let mut asked = user("chat-1", ASKED, None);
        for (key, value) in &route {
            asked.metadata.insert((*key).to_owned(), value.clone());
        }
        agent.process_message(asked)?;
        room.deliver_one()?;
        let record = desk.end(reason).ok_or("waiting record")?;
        let before = seen(&setup, &desk, &provider);

        for (name, forged) in forgeries(&record) {
            let dropped = agent.process_message(room.through(forged)?)?;

            let label = format!("{reason} {name}: {dropped:?}");
            assert_eq!(dropped.stop_reason, "browser_resume_discarded", "{label}");
            assert_eq!(dropped.final_content, None, "{label}");
            assert!(room.deliver().is_empty(), "{label}");
            assert_eq!(seen(&setup, &desk, &provider), before, "{label}");
        }
        assert!(session(&setup, "telegram:chat-9").is_none(), "{reason}");

        // The inbound names another room, topic and thread everywhere it can;
        // the host's record wins.
        let mut real = resume(&record);
        real.chat_id = "chat-9".to_owned();
        for (key, value) in [
            ("message_id", json!("777")),
            ("message_thread_id", json!("55")),
            ("thread_id", json!("66")),
            ("slack", json!({"thread_ts":"9.9"})),
        ] {
            real.metadata.insert(key.to_owned(), value);
        }
        let resumed = agent.process_message(room.through(real)?)?;

        assert_eq!(
            resumed.final_content.as_deref(),
            Some("이어서 했습니다."),
            "{reason}"
        );
        assert_eq!(tool_result(&provider, "hand"), text, "{reason}");
        let stored = stored_result(&setup, "hand").ok_or("tool message")?;
        assert_eq!(stored["name"], "browser", "{reason}");
        let sent = room.deliver();
        assert!(!sent.is_empty(), "{reason}");
        for message in &sent {
            assert_eq!(message.room(), ("telegram", "chat-1"), "{reason}");
            assert_eq!(message.message.reply_to, None, "{reason}");
            for (key, value) in &route {
                assert_eq!(message.message.metadata.get(*key), Some(value), "{reason}");
            }
            assert!(
                !message.message.metadata.contains_key("thread_id"),
                "{reason}"
            );
        }
        let edge = |name: &str| format!("turn_{name} {OWNER}");
        assert_eq!(
            turn_edges(&desk.page),
            [
                edge("started"),
                edge("finished"),
                edge("started"),
                edge("finished")
            ],
            "{reason}"
        );
        assert_eq!(
            desk.log(),
            [format!("resume_settled {}", record.episode)],
            "{reason}"
        );
        assert!(desk.control().waiting.is_empty(), "{reason}");
        let users = classifier_users(&provider, 0);
        assert_eq!(users.len(), 1, "{reason}: {users:?}");
        assert!(users[0].contains(ASKED), "{reason}: {users:?}");
        let saved = session(&setup, OWNER).ok_or("session")?;
        assert!(saved["metadata"]["pending_permission_approval"].is_null());
        assert!(saved["metadata"]["session_permission_approvals"].is_null());
        let user_messages = session_messages(&setup, OWNER)
            .iter()
            .filter(|message| message["role"] == "user")
            .count();
        assert_eq!(user_messages, 1, "{reason}");

        let settled = seen(&setup, &desk, &provider);
        let again = agent.process_message(room.through(resume(&record))?)?;
        assert_eq!(again.stop_reason, "browser_resume_discarded", "{reason}");
        assert_eq!(seen(&setup, &desk, &provider), settled, "{reason}");
    }
    Ok(())
}

#[test]
fn stop_and_new_cancel_paused_handoff() -> TestResult {
    for command in ["/stop", "/new"] {
        let setup = Setup::new()?;
        let desk = Desk::default();
        let registry = tools(&setup, &desk);
        let provider = RoutingProvider::new(
            vec![open("open"), handoff("hand"), answer("네.")],
            vec![Ok("no")],
        );
        let mut room = Room::new(&desk);
        let mut agent = agent(
            &setup,
            &room.bus,
            &registry,
            &provider,
            config(&setup, &desk, PermissionMode::Auto),
        )?;
        agent.process_message(user("chat-1", ASKED, None))?;
        room.deliver_one()?;
        let episode = desk.open_episode().ok_or("open episode")?;
        let requests = provider.assistant_requests().len();

        agent.process_message(user("chat-1", command, None))?;

        let cancelled = [(episode.clone(), "cancelled".to_owned())];
        assert_eq!(desk.ended(), cancelled, "{command}");
        assert_eq!(
            desk.log(),
            [
                format!("cancel_takeover {OWNER}"),
                format!("close_for_session {OWNER}")
            ],
            "{command}"
        );
        assert_eq!(provider.assistant_requests().len(), requests, "{command}");
        if command == "/stop" {
            let closed = stored_result(&setup, "hand").ok_or("closed call")?;
            assert_eq!(content(&closed), STOPPED);
            assert_eq!(closed["name"], "browser");
        } else {
            assert!(session_messages(&setup, OWNER).is_empty());
        }
        room.deliver();
        let before = seen(&setup, &desk, &provider);

        for (reason, _) in ENDINGS {
            let late = resume(&BrowserPendingResume {
                episode: episode.clone(),
                reason: reason.to_owned(),
                session_id: OWNER.to_owned(),
                tool_call_id: "hand".to_owned(),
                channel: "telegram".to_owned(),
                chat_id: "chat-1".to_owned(),
                reply_route: Map::new(),
            });
            let dropped = agent.process_message(room.through(late)?)?;

            assert_eq!(
                dropped.stop_reason, "browser_resume_discarded",
                "{command} {reason}"
            );
            assert!(room.deliver().is_empty(), "{command} {reason}");
            assert_eq!(seen(&setup, &desk, &provider), before, "{command} {reason}");
        }

        // The answer comes after all: it is an ordinary message now.
        let turn = agent.process_message(user("chat-1", "다 했어", None))?;

        assert_eq!(turn.final_content.as_deref(), Some("네."), "{command}");
        assert_eq!(desk.ended(), cancelled, "{command}");
        assert!(
            desk.log()
                .iter()
                .all(|entry| !entry.starts_with("finish_handoff")
                    && !entry.starts_with("handoff_closed")),
            "{command}: {:?}",
            desk.log()
        );
        let requests = provider.assistant_requests();
        let last = requests.last().ok_or("assistant request")?;
        assert!(
            last.messages
                .iter()
                .any(|message| message["role"] == "user" && content(message).contains("다 했어")),
            "{command}"
        );
    }
    Ok(())
}

#[derive(Debug, Clone, Copy)]
enum Ending {
    ReplyToCurrentNotice,
    UnrepliedAfterNotice,
    /// SPEC decision (마): an unreplied message is bound to the episode open
    /// when it reached shacs-bot. One written during the first handoff and
    /// handed over by the channel only now therefore answers the second.
    HeldUnrepliedBoundToArrivalByDecisionMa,
}

fn stale_answers(others: usize, ending: Ending) -> TestResult {
    let label = format!("{others} other episodes, {ending:?}");
    let setup = Setup::new()?;
    let desk = Desk::default();
    let registry = tools(&setup, &desk);
    let provider = RoutingProvider::new(
        vec![handoff("T1"), handoff("T2"), answer("끝났습니다.")],
        Vec::new(),
    );
    let mut room = Room::new(&desk);
    let mut agent = agent(
        &setup,
        &room.bus,
        &registry,
        &provider,
        config(&setup, &desk, PermissionMode::Default),
    )?;
    agent.process_message(user("chat-1", ASKED, None))?;
    let first = desk.open_episode().ok_or("first episode")?;
    let first_notice = room.deliver_one()?;
    // Written while the first handoff was open; the channel holds them back.
    let held_reply = user("chat-1", "다 했어", Some(&first_notice.id));
    let held_plain = user("chat-1", "다 했어", None);
    let record = desk.end("limit").ok_or("waiting record")?;
    for _ in 0..others {
        agent.process_message(user("chat-2", "/browser takeover", None))?;
        room.deliver_one()?;
        let other = desk
            .control()
            .open
            .as_ref()
            .map(|episode| episode.label.clone())
            .ok_or("other episode")?;
        agent.process_message(user("chat-2", &format!("/browser return {other}"), None))?;
        room.deliver_one()?;
    }
    assert_eq!(desk.ended().len(), others + 1, "{label}");

    agent.process_message(room.through(resume(&record))?)?;

    let second = desk.open_episode().ok_or("second episode")?;
    assert_ne!(first, second, "{label}");
    assert_eq!(tool_result(&provider, "T1"), ENDINGS[1].1, "{label}");

    let unanswered = seen(&setup, &desk, &provider);
    // The second notice is still queued when this one arrives.
    let early = agent.process_message(user("chat-1", "다 했어", None))?;

    assert_eq!(early.stop_reason, "browser_handoff_waiting", "{label}");
    assert_eq!(
        early.final_content.as_deref(),
        Some(BEFORE_NOTICE),
        "{label}"
    );
    let sent = room.deliver();
    let [notice, told] = sent.as_slice() else {
        return Err(format!("{label}: {sent:?}").into());
    };
    assert_eq!(notice.episode(), Some(second.as_str()), "{label}");
    assert_eq!(told.message.content, BEFORE_NOTICE, "{label}");
    assert_eq!(told.room(), ("telegram", "chat-1"), "{label}");
    let mut before = seen(&setup, &desk, &provider);
    assert_eq!(
        before["turns"],
        with_idle_turn(&unanswered["turns"]),
        "{label}"
    );
    before["turns"] = unanswered["turns"].clone();
    assert_eq!(before, unanswered, "{label}");
    before["turns"] = with_idle_turn(&unanswered["turns"]);

    for (name, stale) in [
        ("reply to the first notice", held_reply),
        (
            "reply to a message that is no notice",
            user("chat-1", "다 했어", Some(&told.id)),
        ),
    ] {
        let kept = agent.process_message(stale)?;

        assert_eq!(
            kept.stop_reason, "browser_handoff_waiting",
            "{label} {name}"
        );
        let said = room.deliver_one()?;
        assert_eq!(said.message.content, NOT_THIS_NOTICE, "{label} {name}");
        assert_eq!(said.room(), ("telegram", "chat-1"), "{label} {name}");
        before["turns"] = with_idle_turn(&before["turns"]);
        assert_eq!(seen(&setup, &desk, &provider), before, "{label} {name}");
    }

    let last = match ending {
        Ending::ReplyToCurrentNotice => user("chat-1", "다 했어", Some(&notice.id)),
        Ending::UnrepliedAfterNotice => user("chat-1", "다 했어", None),
        Ending::HeldUnrepliedBoundToArrivalByDecisionMa => held_plain,
    };
    let done = agent.process_message(last)?;

    assert_eq!(
        done.final_content.as_deref(),
        Some("끝났습니다."),
        "{label}"
    );
    assert_eq!(
        desk.ended().last(),
        Some(&(second.clone(), "answered".to_owned())),
        "{label}"
    );
    assert_eq!(
        desk.log().last(),
        Some(&format!("finish_handoff {OWNER} T2 {second}")),
        "{label}"
    );
    let finished = desk
        .log()
        .iter()
        .filter(|entry| entry.starts_with("finish_handoff"))
        .count();
    assert_eq!(finished, 1, "{label}");
    assert_eq!(
        tool_result(&provider, "T2"),
        format!("{ANSWERED}\n다 했어"),
        "{label}"
    );
    Ok(())
}

#[test]
fn stale_handoff_answer_does_not_end_new_episode() -> TestResult {
    for others in [0, 9] {
        for ending in [
            Ending::ReplyToCurrentNotice,
            Ending::UnrepliedAfterNotice,
            Ending::HeldUnrepliedBoundToArrivalByDecisionMa,
        ] {
            stale_answers(others, ending)?;
        }
    }

    // A user message that finds its paused call's episode already over,
    // before the continuation arrives, while someone else's takeover is open.
    let setup = Setup::new()?;
    let desk = Desk::default();
    let registry = tools(&setup, &desk);
    let provider = RoutingProvider::new(vec![handoff("T1"), answer("확인했습니다.")], Vec::new());
    let mut room = Room::new(&desk);
    let mut agent = agent(
        &setup,
        &room.bus,
        &registry,
        &provider,
        config(&setup, &desk, PermissionMode::Default),
    )?;
    agent.process_message(user("chat-1", ASKED, None))?;
    let first = desk.open_episode().ok_or("first episode")?;
    room.deliver_one()?;
    let record = desk.end("limit").ok_or("waiting record")?;
    agent.process_message(user("chat-2", "/browser takeover", None))?;
    room.deliver_one()?;
    let current = desk.open_episode().ok_or("current episode")?;

    let turn = agent.process_message(user("chat-1", "어떻게 됐어?", None))?;

    assert_eq!(turn.final_content.as_deref(), Some("확인했습니다."));
    assert_eq!(
        tool_result(&provider, "T1"),
        format!("{TAKEOVER_OVER}\n어떻게 됐어?")
    );
    assert_eq!(desk.open_episode(), Some(current));
    assert_eq!(desk.ended(), [(first, "limit".to_owned())]);
    assert_eq!(desk.log(), [format!("handoff_closed {OWNER} T1")]);
    room.deliver();
    let before = seen(&setup, &desk, &provider);

    let late = agent.process_message(room.through(resume(&record))?)?;

    assert_eq!(late.stop_reason, "browser_resume_discarded");
    assert!(room.deliver().is_empty());
    assert_eq!(seen(&setup, &desk, &provider), before);
    Ok(())
}

fn runner_context(setup: &Setup, options: &Options) -> ToolExecutionContext {
    let mut context = setup.context(options);
    context.browser_context = Some(BrowserCallContext {
        effective_session_id: "cli:direct".to_owned(),
        permission_mode_snapshot: context.permission_mode_snapshot.clone(),
        permission_interactive: options.interactive,
        browser_screenshots: Some(false),
        native_image_input_supported: false,
        media_root: None,
    });
    context
}

struct Attempt {
    desk: Desk,
    registry: ToolRegistry,
    context: ToolExecutionContext,
    report: RuntimeToolExecutionReport,
}

impl Attempt {
    fn handed_over(&self) -> bool {
        matches!(
            self.report.interrupt,
            Some(RuntimeInterrupt::AskUser { .. })
        ) && self.desk.control().opened == 1
    }

    fn refused_with(&self, reason: &str) -> bool {
        self.report.interrupt.is_none()
            && self.desk.control().opened == 0
            && self
                .report
                .messages
                .first()
                .is_some_and(|message| message.content.contains(reason))
    }

    fn decided(&self, change: impl FnOnce(&mut PermissionPolicyInput)) -> PermissionPolicyDecision {
        let action = self.report.permissioned_actions[0].clone();
        let mut input = PermissionPolicyInput {
            static_rule_decision: evaluate_static_rules(
                &action,
                &self.context.permission_rule_input,
            ),
            action,
            evaluator: None,
            approval: None,
            inherited_context: None,
            remembered_rules: Vec::new(),
            remembered_store_unavailable: false,
            interactive: self.context.permission_interactive,
            materials: None,
            outside_access: shacs_core::runtime::OutsideAccess::default(),
        };
        change(&mut input);
        decide_permission(input)
    }
}

fn attempt(setup: &Setup, options: &Options, arguments: Value) -> Attempt {
    rerun(setup, runner_context(setup, options), arguments)
}

fn rerun(setup: &Setup, context: ToolExecutionContext, arguments: Value) -> Attempt {
    let desk = Desk::default();
    let mut registry = tools(setup, &desk);
    registry.register(AskUserTool::new());
    let report = support::run_report(&registry, &context, support::call("browser", arguments));
    Attempt {
        desk,
        registry,
        context,
        report,
    }
}

fn handoff_arguments() -> Value {
    json!({"action":"handoff","reason":REASON})
}

#[test]
fn handoff_permission_follows_deny_rules_and_interactivity() -> TestResult {
    let setup = Setup::new()?;
    for mode in ALL_MODES {
        for interactive in [true, false] {
            let label = format!("{mode:?} interactive {interactive}");

            // No rules: allowed where someone can answer, in every mode.
            let plain = attempt(
                &setup,
                &Options::mode(mode, interactive),
                handoff_arguments(),
            );
            assert!(
                plain.report.permissioned_actions[0].capabilities.is_empty(),
                "{label}"
            );
            let decision = plain.decided(|_| {});
            if interactive {
                assert!(plain.handed_over(), "{label}: {:?}", plain.report);
                assert_eq!(
                    (decision.kind, decision.reason),
                    (
                        PermissionPolicyDecisionKind::Allow,
                        PermissionPolicyReason::LocalUserInteraction
                    ),
                    "{label}"
                );
                assert_eq!(decision.approval_ref, None, "{label}");
            } else {
                assert!(
                    plain.refused_with("ModeBaselineDeny"),
                    "{label}: {:?}",
                    plain.report
                );
                assert_eq!(
                    (decision.kind, decision.reason),
                    (
                        PermissionPolicyDecisionKind::Deny,
                        PermissionPolicyReason::ModeBaselineDeny
                    ),
                    "{label}"
                );
            }

            let denied = attempt(
                &setup,
                &Options::mode(mode, interactive).rules(Rules::deny(&["browser"])),
                handoff_arguments(),
            );
            assert!(
                denied.refused_with("UserRuleDeny"),
                "{label}: {:?}",
                denied.report
            );

            let action = &plain.report.permissioned_actions[0];
            let mut remembering = plain.context.clone();
            remembering.permission_session_remembered_rules =
                vec![SessionRememberedPermissionRule {
                    session_key: "cli:direct".to_owned(),
                    approval_context_digest: session_remembered_context_digest(action),
                    effect: RememberedPermissionEffect::Deny,
                    matcher: RememberedPermissionMatcher::ExactAction {
                        action_digest: action.action_digest.clone(),
                    },
                    created_unix_ms: support::now_unix_ms(),
                    legacy_imported: false,
                }];
            let remembered = rerun(&setup, remembering, handoff_arguments());
            assert!(
                remembered.refused_with("RememberedDeny"),
                "{label}: {:?}",
                remembered.report
            );

            let fixed = plain.decided(|input| {
                input.static_rule_decision.kind = StaticRuleDecisionKind::Deny;
                input.static_rule_decision.reason = StaticRuleReason::SecretRead;
            });
            assert_eq!(fixed.kind, PermissionPolicyDecisionKind::Deny, "{label}");

            let asking = Options::mode(mode, interactive).rules(Rules::ask(&["browser"]));
            let asked = attempt(&setup, &asking, handoff_arguments());
            if interactive && mode != PermissionMode::DontAsk {
                let outcome = support::outcome(asked.report.clone());
                let question = outcome
                    .question()
                    .ok_or_else(|| format!("{label}: {:?}", asked.report))?;
                assert_eq!(asked.desk.control().opened, 0, "{label}");
                let mut approved = asked.context.clone();
                approved.permission_approval_cache =
                    Some(question.approval(ApprovalDecisionKind::Approved));
                let report = support::run_report(&asked.registry, &approved, question.call.clone());
                assert!(
                    matches!(report.interrupt, Some(RuntimeInterrupt::AskUser { .. })),
                    "{label}: {report:?}"
                );
                assert_eq!(asked.desk.control().opened, 1, "{label}");
            } else {
                let reason = if interactive {
                    "UserRuleAsk"
                } else {
                    "ModeBaselineDeny"
                };
                assert!(asked.refused_with(reason), "{label}: {:?}", asked.report);
            }
        }
    }

    // Auto mode: no classifier, and unattended it is the baseline that
    // refuses, not the rule that keeps browser input from running alone.
    for verdict in [None, Some(Verdict::Block), Some(Verdict::Allow)] {
        let mut options = Options::auto(true);
        options.verdict = verdict;
        let allowed = attempt(&setup, &options, handoff_arguments());
        assert!(allowed.handed_over(), "{verdict:?}: {:?}", allowed.report);
        assert!(allowed.report.recent_auto_mode_denials.is_empty());
        options.interactive = false;
        options.rules = Rules::ask(&["browser"]);
        let unattended = attempt(&setup, &options, handoff_arguments());
        assert!(
            unattended.refused_with("ModeBaselineDeny"),
            "{verdict:?}: {:?}",
            unattended.report
        );
        assert!(!unattended.refused_with("UserRuleAsk"), "{verdict:?}");
    }

    // The other ten actions keep their capability and their decisions.
    // `capture` needs an open page, so only its capability is compared.
    for mode in ALL_MODES {
        for interactive in [true, false] {
            for verdict in [None, Some(Verdict::Allow), Some(Verdict::Block)] {
                for action in [
                    "open", "look", "capture", "scroll", "back", "wait", "close", "click", "type",
                    "press",
                ] {
                    let label = format!("{mode:?} {interactive} {verdict:?} {action}");
                    let arguments = match action {
                        "open" => json!({"action":"open","url":"https://shop.example/cart"}),
                        "click" => json!({"action":"click","element":2,"revision":"r0"}),
                        "type" => {
                            json!({"action":"type","element":1,"revision":"r0","text":"hello"})
                        }
                        "press" => json!({"action":"press","key":"Enter"}),
                        "wait" => json!({"action":"wait","timeoutMs":1}),
                        _ => json!({"action":action}),
                    };
                    let mut options = Options::mode(mode, interactive);
                    options.verdict = verdict;
                    let other = attempt(&setup, &options, arguments);
                    let act = matches!(action, "click" | "type" | "press");
                    assert_eq!(
                        other.report.permissioned_actions[0].capabilities,
                        [if act {
                            SafetyCapability::BrowserAct
                        } else {
                            SafetyCapability::NetOutbound
                        }],
                        "{label}"
                    );
                    let allowed = mode == PermissionMode::BypassPermissions
                        || (!act && mode == PermissionMode::Plan)
                        || (mode == PermissionMode::Auto
                            && (!act || interactive)
                            && verdict == Some(Verdict::Allow));
                    let asked = interactive
                        && matches!(
                            mode,
                            PermissionMode::Default
                                | PermissionMode::AcceptEdits
                                | PermissionMode::Plan
                        )
                        && !(mode == PermissionMode::Plan && !act);
                    assert_eq!(other.desk.control().opened, 0, "{label}");
                    if action == "capture" {
                        continue;
                    }
                    assert_eq!(
                        !other.report.executed_call_ids.is_empty(),
                        allowed,
                        "{label}: {:?}",
                        other.report
                    );
                    assert_eq!(other.report.interrupt.is_some(), asked, "{label}");
                }
            }
        }
    }

    // `ask_user` still pauses whoever is listening, and an unknown action is
    // still refused before any decision.
    for mode in ALL_MODES {
        for interactive in [true, false] {
            let plain = attempt(
                &setup,
                &Options::mode(mode, interactive),
                handoff_arguments(),
            );
            let report = support::run_report(
                &plain.registry,
                &plain.context,
                RuntimeToolCall::new("ask", "ask_user", json!({"question":"어느 쪽인가요?"})),
            );
            assert!(
                matches!(report.interrupt, Some(RuntimeInterrupt::AskUser { .. })),
                "{mode:?} {interactive}: {report:?}"
            );
            let unknown = attempt(
                &setup,
                &Options::mode(mode, interactive),
                json!({"action":"teleport","reason":REASON}),
            );
            assert!(
                unknown.refused_with("browser_invalid_arguments: unknown action"),
                "{mode:?} {interactive}: {:?}",
                unknown.report
            );
        }
    }
    Ok(())
}

fn command(
    agent: &mut AgentLoop<'_>,
    message: InboundMessage,
) -> Result<String, Box<dyn std::error::Error>> {
    let result = agent.process_message(message)?;
    assert_eq!(result.command, Some(AgentLoopCommandResult::Browser));
    assert_eq!(result.stop_reason, "browser");
    assert_eq!(result.outbound_count, 1);
    Ok(result.final_content.unwrap_or_default())
}

fn unreplied(chat: &str, episode: Option<&str>, asked: bool) -> Option<BrowserReturnTarget> {
    Some(BrowserReturnTarget::Arrival {
        channel: "telegram".to_owned(),
        chat_id: chat.to_owned(),
        reply_to: None,
        arrival: BrowserArrival {
            episode: episode.map(str::to_owned),
            asked,
        },
    })
}

#[test]
fn takeover_commands_and_guidance() -> TestResult {
    let setup = Setup::new()?;
    let desk = Desk::default();
    let registry = tools(&setup, &desk);
    let provider = RoutingProvider::new(vec![answer("안녕하세요.")], Vec::new());
    let mut room = Room::new(&desk);
    let mut agent = agent(
        &setup,
        &room.bus,
        &registry,
        &provider,
        config(&setup, &desk, PermissionMode::Default),
    )?;
    agent.process_message(user("chat-1", "안녕", None))?;
    let stored = session(&setup, OWNER).ok_or("session")?;
    let requests = provider.assistant_requests().len();
    let give_back = |chat: &str, reply_to: Option<&str>| user(chat, "/browser return", reply_to);
    // Reached shacs-bot while nothing was taken over; processed much later.
    let idle = queued(&desk, give_back("chat-1", None));

    // Row 5: nothing was taken over when the message arrived.
    assert_eq!(command(&mut agent, give_back("chat-1", None))?, NOT_TAKEN);
    assert_eq!(desk.last_target(), unreplied("chat-1", None, false));

    let taken = command(&mut agent, user("chat-1", "/browser takeover", None))?;
    assert_eq!(
        taken,
        format!(
            "브라우저를 넘겨받았습니다(9f2c-1).\n화면 보기: {VIEW}\n끝나면 /browser return으로 돌려주세요. {MINUTES}분이 지나면 자동으로 돌려받습니다."
        )
    );
    let first = desk.open_episode().ok_or("first episode")?;

    // Row 4: the notice has not been delivered yet.
    assert_eq!(
        command(&mut agent, give_back("chat-1", None))?,
        RETURN_NOT_ASKED
    );
    assert_eq!(desk.last_target(), unreplied("chat-1", Some(&first), false));
    assert_eq!(desk.open_episode(), Some(first.clone()));

    let sent = room.deliver();
    let notice = sent
        .iter()
        .find(|sent| sent.episode() == Some(first.as_str()))
        .ok_or("takeover notice")?;
    assert_eq!(notice.message.content, taken);
    assert_eq!(notice.stop_reason(), Some("browser"));
    let other = sent
        .iter()
        .find(|sent| sent.episode().is_none())
        .ok_or("an answer that is no notice")?;

    assert_eq!(
        command(&mut agent, user("chat-1", "/browser status", None))?,
        format!(
            "브라우저: 열림\n쓰는 세션: {OWNER}\n답 기다리기: 멈춤\n조작: 사용자\n넘겨받은 곳: {OWNER} 9f2c-1 (남은 {MINUTES}분)\n화면 보기: {VIEW}"
        )
    );
    assert_eq!(
        command(&mut agent, user("chat-2", "/browser takeover", None))?,
        "이미 넘겨받은 상태입니다."
    );

    // Row 2: a reply to some other message ends nothing.
    assert_eq!(
        command(&mut agent, give_back("chat-1", Some(&other.id)))?,
        RETURN_REPLY_MISMATCH
    );
    assert_eq!(
        desk.last_target(),
        Some(BrowserReturnTarget::Arrival {
            channel: "telegram".to_owned(),
            chat_id: "chat-1".to_owned(),
            reply_to: Some(other.id.clone()),
            arrival: BrowserArrival {
                episode: Some(first.clone()),
                asked: true,
            },
        })
    );
    // A label names its own episode only.
    assert_eq!(
        command(&mut agent, user("chat-2", "/browser return 9f2c-9", None))?,
        OVER
    );
    assert_eq!(
        desk.last_target(),
        Some(BrowserReturnTarget::Label("9f2c-9".to_owned()))
    );
    let targets = desk.control().targets.len();
    for wrong in [
        "/browser return 9f2c",
        "/browser return 9f2c-01",
        "/browser return zzzz-1",
        "/browser return 9f2c-1 now",
        "/browser steal",
    ] {
        assert_eq!(
            command(&mut agent, user("chat-1", wrong, None))?,
            USAGE,
            "{wrong}"
        );
    }
    assert_eq!(desk.control().targets.len(), targets);
    assert_eq!(desk.open_episode(), Some(first.clone()));
    assert!(desk.ended().is_empty());
    // Reached shacs-bot during the first takeover; processed after it ended.
    let late = queued(&desk, give_back("chat-1", None));

    // Row 1: a reply to the open episode's notice.
    assert_eq!(
        command(&mut agent, give_back("chat-1", Some(&notice.id)))?,
        RETURNED
    );
    assert_eq!(desk.ended(), [(first.clone(), "returned".to_owned())]);

    assert!(command(&mut agent, user("chat-2", "/browser takeover", None))?.contains("(9f2c-2)"));
    room.deliver();
    let second = desk.open_episode().ok_or("second episode")?;

    // Row 6: the episode open at arrival is over; the one open now stays.
    assert_eq!(command(&mut agent, late)?, OVER);
    assert_eq!(desk.last_target(), unreplied("chat-1", Some(&first), true));
    // Row 5 again, for a message that waited through two takeovers.
    assert_eq!(command(&mut agent, idle)?, NOT_TAKEN);
    assert_eq!(desk.open_episode(), Some(second.clone()));

    // Row 3: no reply, arrived after the notice was delivered, any room.
    assert_eq!(command(&mut agent, give_back("chat-1", None))?, RETURNED);
    assert_eq!(desk.last_target(), unreplied("chat-1", Some(&second), true));
    assert_eq!(desk.ended().last(), Some(&(second, "returned".to_owned())));

    // A label works from another room and before the notice is delivered.
    assert!(command(&mut agent, user("chat-1", "/browser takeover", None))?.contains("(9f2c-3)"));
    let third = desk.open_episode().ok_or("third episode")?;
    assert_eq!(
        command(&mut agent, user("chat-2", "/browser return 9F2C-3", None))?,
        RETURNED
    );
    assert_eq!(
        desk.last_target(),
        Some(BrowserReturnTarget::Label("9f2c-3".to_owned()))
    );
    assert_eq!(desk.ended().last(), Some(&(third, "returned".to_owned())));

    let status = command(&mut agent, user("chat-2", "/browser status", None))?;
    assert!(
        status.ends_with(&format!(
            "조작: 어시스턴트\n넘겨받은 곳: 없음\n화면 보기: {VIEW}"
        )),
        "{status}"
    );
    desk.control().view_off = Some("disabled".to_owned());
    let status = command(&mut agent, user("chat-2", "/browser status", None))?;
    assert!(status.ends_with("화면 보기: 꺼짐(disabled)"), "{status}");
    assert_eq!(
        command(&mut agent, user("chat-1", "/browser takeover", None))?,
        "화면 보기를 쓸 수 없습니다(disabled)"
    );

    assert_eq!(provider.assistant_requests().len(), requests);
    assert_eq!(session(&setup, OWNER), Some(stored));
    assert!(session(&setup, "telegram:chat-2").is_none());
    assert!(desk.log().is_empty());

    let help = agent.process_message(user("chat-1", "/help", None))?;
    let help = help.final_content.unwrap_or_default();
    assert!(
        help.contains("/browser status|takeover|return [label]|close"),
        "{help}"
    );

    let tool = BrowserTool::new(Arc::new(Desk::default()));
    assert_eq!(tool.description(), DESCRIPTION);
    let stale = "There is no way to enter them directly yet";
    assert!(!tool.description().contains(stale));
    assert!(tool
        .description()
        .contains("call handoff with a short reason"));
    let guided = RoutingProvider::new(vec![answer("ok")], Vec::new());
    crate::spec050_conversational_choice::agent(
        &setup,
        &MessageBus::new(),
        &registry,
        &guided,
        config(&setup, &desk, PermissionMode::Auto),
    )?
    .process_direct("hi", Some("cli:guide"))?;
    let guided = guided.assistant_requests();
    let system = guided
        .first()
        .and_then(|request| request.messages.first())
        .map(content)
        .unwrap_or_default();
    assert!(
        system.ends_with(&format!("{GUIDANCE}\n\n{BROWSER_GUIDANCE}")),
        "{system}"
    );
    assert!(system.contains("use the browser tool's handoff action"));
    assert!(!system.contains(stale));
    Ok(())
}
