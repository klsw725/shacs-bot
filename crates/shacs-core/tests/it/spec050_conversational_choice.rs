use crate::spec048_auto_mode::harness::{
    agent_loop_config, answer, calls, spec, tool_call, RoutingProvider,
};
use crate::spec048_auto_mode::support::{self, Exec, Options, Setup, TestResult};
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use shacs_core::runtime::{
    AgentLoop, AgentLoopCommandResult, AgentLoopConfig, AgentRunner, BrowserAction,
    BrowserCallContext, BrowserCapture, BrowserCloseOutcome, BrowserClosureNotice,
    BrowserControlDescription, BrowserDescription, BrowserDescriptionState, BrowserDocument,
    BrowserElement, BrowserError, BrowserHost, BrowserPreparedAction, BrowserRead, BrowserSession,
    ContextBuilder, DurableWorkDispatcher, GoalCompletionEvaluator, GoalCompletionVerdict,
    GoalEvaluationRequest, GoalEvaluatorOutcome, InboundMessage, MessageBus, PermissionMode,
    RuntimeToolCall, SessionManager, SessionTurnAcquireError, SessionTurnCancelOutcome,
    SessionTurnLock, ToolExecutionContext, ToolImageSettings,
};
use shacs_core::tools::{BrowserTool, MessageTool, Tool, ToolRegistry};
use shacs_eval::completion_boundary::{EvaluatorRoute, OwnerResultLocator};
use shacs_eval::evaluator::{
    EvaluatorVerdictEnvelope, RedactionStatus, SuggestedNextAction, VerdictKind,
};
use shacs_providers::{LlmResponse, ProviderRequest};
use shacs_session::durable_replay::evaluate_durable_recovery;
use shacs_session::durable_work::evaluate_durable_work_recovery;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Mutex, MutexGuard};

pub(super) const DESCRIPTION: &str = "Open and read http(s) pages in the official container. Use element numbers and the latest revision. Click/type/press require browser_act permission and a verified target. Passwords, file upload and downloads are unavailable; use web_fetch for downloads. Canvas controls may be inaccessible. When a step is the user's to decide (a choice among options, a purchase, payment, submission or other commitment they did not ask for, or a page that differs from the request), do not act: tell the user briefly what you did and what the page shows, list the choices by their exact element names from the latest result, ask them to reply with the name, and end your turn. When the reply arrives, look again and click only if an element with that name is on the page now; otherwise describe the changed page. Never ask the user to send passwords, verification codes or card numbers in chat. For logins, passwords, verification codes, CAPTCHAs or identity checks, and when the user asks to take over the browser, call handoff with a short reason; the user then works on the screen viewer. Any reply to handoff returns the browser: read it, and if the user postponed, do not hand off again; finish what you can and list what remains. While the user controls the browser every action returns browser_user_control. After control returns, look again; earlier element numbers are invalid. Use capture only when the user asks to see the screen; it saves a fresh screenshot and returns a path to send with the message tool. If capture fails, tell the user why and do not send an older image.";
pub(super) const GUIDANCE: &str = "# Auto Mode\n\nAuto mode is on: a classifier checks your tool calls instead of the user approving each one. Go ahead with routine steps without asking the user to confirm them. Ask only when the user's request or a skill you are following says to confirm first.";
pub(super) const BROWSER_GUIDANCE: &str = "In the browser, when a step is the user's to decide, tell them the progress and the choices in text, end the turn, and wait for their reply.\nFor login, passwords, CAPTCHAs or identity checks in the browser, use the browser tool's handoff action; use it too when the user asks in words to take over the browser. If they postpone, do not hand off again: do what you can and tell them what remains.";
const PNG: &[u8] = b"\x89PNG\r\n\x1a\n";
pub(super) const OWNER: &str = "telegram:chat-1";
const RUNNER_SESSION: &str = "cli:direct";
/// The lines `/browser status` adds while nobody has taken the browser over.
const ASSISTANT_CONTROL: &str = "조작: 어시스턴트\n넘겨받은 곳: 없음\n화면 보기: 꺼짐(disabled)";

#[derive(Default)]
pub(super) struct State {
    pub(super) owner: Option<String>,
    sequence: usize,
    generation: usize,
    pub(super) waiting: bool,
    action_running: bool,
    close_on_acquire: bool,
    capture_error: Option<BrowserError>,
    notices: BTreeMap<String, BrowserClosureNotice>,
    issued: usize,
    events: Vec<String>,
    pub(super) clicks: Vec<String>,
}

#[derive(Clone, Default)]
pub(super) struct Host(Arc<Mutex<State>>);

impl Host {
    pub(super) fn state(&self) -> MutexGuard<'_, State> {
        self.0.lock().expect("host state lock poisoned")
    }

    fn notify(&self, session: &str, error: BrowserError) -> String {
        put_notice(&mut self.state(), session, error)
    }

    pub(super) fn events(&self) -> Vec<String> {
        self.state().events.clone()
    }

    fn screen(sequence: usize) -> Vec<u8> {
        [PNG, format!("page {sequence}").as_bytes()].concat()
    }
}

fn put_notice(state: &mut State, session: &str, error: BrowserError) -> String {
    state.issued += 1;
    let token = format!("token-{}", state.issued);
    state.notices.insert(
        session.to_owned(),
        BrowserClosureNotice {
            error,
            token: token.clone(),
        },
    );
    token
}

fn code(error: BrowserError) -> String {
    let text = error.to_string();
    text.split(':').next().unwrap_or_default().to_owned()
}

fn element_name(element: &Value) -> &'static str {
    if element == 2 {
        "결제하기"
    } else {
        "계속 쇼핑"
    }
}

fn prepared(
    state: &State,
    session: &str,
    arguments: &Value,
) -> Result<BrowserPreparedAction, BrowserError> {
    if let Some(notice) = state.notices.get(session) {
        return Err(notice.error);
    }
    if state.owner.as_deref().is_some_and(|owner| owner != session) {
        return Err(BrowserError::Busy);
    }
    let action = arguments["action"].as_str().unwrap_or_default();
    if action == "capture" && state.owner.is_none() {
        return Err(BrowserError::NotOpen);
    }
    if action == "click" && arguments["revision"] != format!("r{}", state.sequence) {
        return Err(BrowserError::StaleRevision);
    }
    let description = json!({"action":action,
        "origin":if action == "open" {arguments["url"].clone()} else {json!("https://shop.example")},
        "role":"button","name":element_name(&arguments["element"]),
        "revision":format!("r{}", state.sequence),
        "frame":"frame","loader":"loader","form":null,"focus":null,
        "text":arguments["text"],"submit":arguments["submit"],"key":arguments["key"]});
    let raw = json!({"arguments":arguments,"description":description}).to_string();
    Ok(BrowserPreparedAction {
        description,
        digest: format!("{:x}", Sha256::digest(raw.as_bytes())),
    })
}

fn document() -> BrowserDocument {
    BrowserDocument {
        target_id: "page".to_owned(),
        session_id: "cdp".to_owned(),
        frame_id: "frame".to_owned(),
        loader_id: "loader".to_owned(),
        url: "https://shop.example/cart".to_owned(),
    }
}

impl BrowserHost for Host {
    fn prepare(
        &self,
        session_id: &str,
        arguments: &Value,
    ) -> Result<BrowserPreparedAction, BrowserError> {
        let mut state = self.state();
        let result = prepared(&state, session_id, arguments);
        let seen = result
            .as_ref()
            .map_or_else(|error| code(*error), |_| "ok".to_owned());
        let action = arguments["action"].as_str().unwrap_or("?").to_owned();
        state.events.push(format!("prepare {action} {seen}"));
        result
    }

    fn close_prepared(
        &self,
        context: &BrowserCallContext,
        digest: &str,
    ) -> Result<(), BrowserError> {
        let mut state = self.state();
        let close = json!({"action":"close"});
        if prepared(&state, &context.effective_session_id, &close)?.digest != digest {
            return Err(BrowserError::StaleRevision);
        }
        state.owner = None;
        Ok(())
    }

    fn acquire(
        &self,
        context: &BrowserCallContext,
    ) -> Result<Box<dyn BrowserSession>, BrowserError> {
        let mut state = self.state();
        let session = &context.effective_session_id;
        if state.close_on_acquire {
            state.close_on_acquire = false;
            state.owner = None;
            put_notice(&mut state, session, BrowserError::Closed);
        }
        if let Some(notice) = state.notices.get(session) {
            return Err(notice.error);
        }
        match state.owner.as_deref() {
            Some(owner) if owner != session => return Err(BrowserError::Busy),
            Some(_) => {}
            None => {
                state.generation += 1;
                state.owner = Some(session.clone());
            }
        }
        Ok(Box::new(self.clone()))
    }

    fn capture_prepared(
        &self,
        context: &BrowserCallContext,
        digest: &str,
    ) -> Result<BrowserCapture, BrowserError> {
        let state = self.state();
        let capture = json!({"action":"capture"});
        if prepared(&state, &context.effective_session_id, &capture)?.digest != digest {
            return Err(BrowserError::StaleRevision);
        }
        if let Some(error) = state.capture_error {
            return Err(error);
        }
        Ok(BrowserCapture {
            document: document(),
            png: Host::screen(state.sequence),
        })
    }

    fn turn_started(&self, session_id: &str) {
        let mut state = self.state();
        state.events.push(format!("turn_started {session_id}"));
        if state.owner.as_deref() == Some(session_id) {
            state.waiting = false;
        }
    }

    fn turn_finished(&self, session_id: &str) {
        let mut state = self.state();
        state.events.push(format!("turn_finished {session_id}"));
        if state.owner.as_deref() == Some(session_id) {
            state.waiting = true;
        }
    }

    fn closure_notice(&self, session_id: &str) -> Option<BrowserClosureNotice> {
        self.state().notices.get(session_id).cloned()
    }

    fn acknowledge_closure_notice(&self, session_id: &str, token: &str) {
        let mut state = self.state();
        state
            .events
            .push(format!("acknowledge {session_id} {token}"));
        if state
            .notices
            .get(session_id)
            .is_some_and(|notice| notice.token == token)
        {
            state.notices.remove(session_id);
        }
    }

    fn close_for_session(&self, session_id: &str) -> bool {
        let mut state = self.state();
        state.events.push(format!("close_for_session {session_id}"));
        if state.owner.as_deref() != Some(session_id) {
            return false;
        }
        state.owner = None;
        state.waiting = false;
        put_notice(&mut state, session_id, BrowserError::Closed);
        true
    }

    fn close_by_user(&self) -> BrowserCloseOutcome {
        let mut state = self.state();
        state.events.push("close_by_user".to_owned());
        let Some(owner) = state.owner.clone() else {
            return BrowserCloseOutcome::NothingOpen;
        };
        if state.action_running {
            return BrowserCloseOutcome::Scheduled;
        }
        state.owner = None;
        state.waiting = false;
        put_notice(&mut state, &owner, BrowserError::Closed);
        BrowserCloseOutcome::Closed
    }

    fn describe(&self) -> BrowserDescription {
        let state = self.state();
        BrowserDescription {
            state: if state.owner.is_some() {
                BrowserDescriptionState::Open
            } else {
                BrowserDescriptionState::Closed
            },
            session: state.owner.clone(),
            reply_wait_remaining_minutes: (state.owner.is_some() && state.waiting).then_some(30),
            control: BrowserControlDescription::default(),
        }
    }
}

impl BrowserSession for Host {
    fn execute_prepared(
        &self,
        context: &BrowserCallContext,
        arguments: &Value,
        digest: &str,
    ) -> Result<Option<BrowserRead>, BrowserError> {
        {
            let mut state = self.state();
            if prepared(&state, &context.effective_session_id, arguments)?.digest != digest {
                return Err(BrowserError::StaleRevision);
            }
            if arguments["action"] == "click" {
                state
                    .clicks
                    .push(element_name(&arguments["element"]).to_owned());
            }
        }
        self.read(context).map(Some)
    }

    fn current_document(
        &self,
        _context: &BrowserCallContext,
    ) -> Result<BrowserDocument, BrowserError> {
        Ok(document())
    }

    fn read(&self, context: &BrowserCallContext) -> Result<BrowserRead, BrowserError> {
        let mut state = self.state();
        state.sequence += 1;
        let screenshot = context
            .browser_screenshots
            .unwrap_or(context.native_image_input_supported)
            .then(|| STANDARD.encode([PNG, b"look".as_slice()].concat()));
        Ok(BrowserRead {
            document: document(),
            accessibility: json!({}),
            visible_text: "장바구니: 우유 1개".to_owned(),
            title: "Cart".to_owned(),
            boxes: Vec::new(),
            screenshot,
            revision: format!("r{}", state.sequence),
            elements: [1, 2]
                .map(|element| BrowserElement {
                    element,
                    role: "button".to_owned(),
                    name: element_name(&json!(element)).to_owned(),
                    value: String::new(),
                })
                .to_vec(),
            omitted_elements: 0,
            notice: None,
        })
    }

    fn act(
        &self,
        context: &BrowserCallContext,
        _action: &BrowserAction,
    ) -> Result<BrowserRead, BrowserError> {
        self.read(context)
    }

    fn close(&self, _context: &BrowserCallContext) -> Result<(), BrowserError> {
        self.state().owner = None;
        Ok(())
    }
}

fn bypass(setup: &Setup, host: &Host) -> AgentLoopConfig {
    let mut config = agent_loop_config(setup, true);
    config.permission_mode_snapshot.mode = PermissionMode::BypassPermissions;
    config.permission_ceiling_snapshot = setup
        .context(&Options::mode(PermissionMode::BypassPermissions, true))
        .permission_ceiling_snapshot;
    config.permission_auto_approval.enabled = false;
    config.browser_host = Some(Arc::new(host.clone()));
    config
}

pub(super) fn agent<'a>(
    setup: &Setup,
    bus: &MessageBus,
    registry: &'a ToolRegistry,
    provider: &'a RoutingProvider,
    config: AgentLoopConfig,
) -> Result<AgentLoop<'a>, Box<dyn std::error::Error>> {
    Ok(AgentLoop::new(
        bus.clone(),
        SessionManager::new(&setup.workspace)?,
        ContextBuilder::new(&setup.workspace),
        registry,
        provider,
        config,
    ))
}

pub(super) fn inbound(chat: &str, text: &str) -> InboundMessage {
    InboundMessage::new("telegram", "user-1", chat, text)
}

fn runtime_inbound(channel: &str, session: &str) -> InboundMessage {
    InboundMessage::new(channel, "system", channel, "맡은 일을 이어서 해")
        .with_session_key_override(session)
}

/// A goal evaluator that asks for one more turn after every turn.
fn keeps_going() -> Arc<dyn GoalCompletionEvaluator> {
    let evaluations = AtomicUsize::new(0);
    Arc::new(move |_request: &GoalEvaluationRequest| {
        let evaluation = evaluations.fetch_add(1, Ordering::SeqCst);
        Ok(GoalEvaluatorOutcome {
            output: EvaluatorVerdictEnvelope {
                verdict_kind: VerdictKind::Pass,
                reason: "keep going".to_owned(),
                confidence: 1.0,
                evidence_refs: Vec::new(),
                suggested_next_action: SuggestedNextAction::None,
                expires_at_ms: None,
                redaction_status: RedactionStatus::AlreadySafe,
                evaluator_version: "test-v1".to_owned(),
            },
            requested_route: EvaluatorRoute::Verify,
            owner_result_locator: OwnerResultLocator::new(format!(
                "session:{OWNER}:turn-{evaluation}"
            )),
            advisory_verdict: Some(GoalCompletionVerdict::Continue),
        })
    })
}

/// The follow-up turn the loop queued for the session's goal, as the durable
/// dispatcher hands it to the bus.
fn queued_goal_continuation(
    setup: &Setup,
    bus: &MessageBus,
) -> Result<InboundMessage, Box<dyn std::error::Error>> {
    let runtime = setup.workspace.join("runtime");
    let (event_root, payload_root) = (
        runtime.join("durable-events"),
        runtime.join("work-payloads"),
    );
    let pending = evaluate_durable_recovery(&event_root, runtime.join("durable-checkpoints"))
        .state
        .ok_or("durable state")?;
    let admission = evaluate_durable_work_recovery(&pending.work, &payload_root, 1);
    let mut dispatcher =
        DurableWorkDispatcher::open(&event_root, &payload_root, bus.clone(), "test-owner", 100)?;
    dispatcher.dispatch_due(&pending.work, &admission, 1)?;
    Ok(bus.consume_inbound().ok_or("goal continuation")?)
}

pub(super) fn turn_edges(host: &Host) -> Vec<String> {
    host.events()
        .into_iter()
        .filter(|event| event.starts_with("turn_"))
        .collect()
}

pub(super) fn browser(id: &str, arguments: Value) -> LlmResponse {
    calls(vec![tool_call(id, "browser", arguments)])
}

pub(super) fn open(id: &str) -> LlmResponse {
    browser(
        id,
        json!({"action":"open","url":"https://shop.example/cart"}),
    )
}

pub(super) fn content(message: &Value) -> String {
    match &message["content"] {
        Value::String(text) => text.clone(),
        other => other.to_string(),
    }
}

pub(super) fn tool_result(provider: &RoutingProvider, id: &str) -> String {
    provider
        .assistant_requests()
        .iter()
        .rev()
        .flat_map(|request| request.messages.iter())
        .find(|message| message["role"] == "tool" && message["tool_call_id"] == id)
        .map(content)
        .unwrap_or_default()
}

pub(super) fn mentions(request: &ProviderRequest, text: &str) -> usize {
    request
        .messages
        .iter()
        .filter(|message| content(message).contains(text))
        .count()
}

fn refused(error: BrowserError) -> String {
    format!("Permission denied: tool call was not executed ({error}).")
}

fn runner_context(setup: &Setup, options: &Options) -> ToolExecutionContext {
    let mut context = setup.context(options);
    context.browser_context = Some(BrowserCallContext {
        effective_session_id: RUNNER_SESSION.to_owned(),
        permission_mode_snapshot: context.permission_mode_snapshot.clone(),
        permission_interactive: options.interactive,
        browser_screenshots: Some(false),
        native_image_input_supported: false,
        media_root: None,
    });
    context
}

fn capture_path(setup: &Setup, sequence: usize) -> PathBuf {
    setup.media.join(format!(
        "tool-results/{:x}.png",
        Sha256::digest(Host::screen(sequence))
    ))
}

fn send(id: &str, path: &Path) -> LlmResponse {
    calls(vec![tool_call(
        id,
        "message",
        json!({"content":"지금 화면입니다","media":[path.to_string_lossy()]}),
    )])
}

struct CaptureRun {
    host: Host,
    provider: RoutingProvider,
    outbound: Vec<shacs_core::runtime::OutboundMessage>,
}

fn capture_turn(
    setup: &Setup,
    host: Host,
    script: Vec<LlmResponse>,
    screenshots: Option<bool>,
    attach: bool,
) -> Result<CaptureRun, Box<dyn std::error::Error>> {
    let bus = MessageBus::new();
    let message_tool = MessageTool::new(&setup.workspace);
    let mut registry = ToolRegistry::new();
    registry.register(message_tool.clone());
    registry.register(BrowserTool::new(Arc::new(host.clone())));
    let provider = RoutingProvider::new(script, Vec::new());
    let mut config = bypass(setup, &host);
    config.browser_screenshots = screenshots;
    config.tool_images = Some(ToolImageSettings {
        media_root: setup.media.clone(),
        attach,
    });
    config.media_roots = vec![setup.media.clone()];
    agent(setup, &bus, &registry, &provider, config)?
        .with_message_tool_delivery(message_tool)
        .process_message(inbound("chat-1", "지금 화면 보여줘"))?;
    let mut outbound = Vec::new();
    while let Some(message) = bus.consume_outbound() {
        outbound.push(message);
    }
    Ok(CaptureRun {
        host,
        provider,
        outbound,
    })
}

#[test]
fn tool_description_and_auto_guidance_carry_choice_rules() -> TestResult {
    let tool = BrowserTool::new(Arc::new(Host::default()));
    assert_eq!(tool.description(), DESCRIPTION);
    for sentence in [
        "Never ask the user to send passwords, verification codes or card numbers in chat.",
        "For logins, passwords, verification codes, CAPTCHAs or identity checks, and when the user asks to take over the browser, call handoff with a short reason; the user then works on the screen viewer.",
    ] {
        assert!(tool.description().contains(sentence), "{sentence}");
    }

    for (mode, with_browser) in [
        (PermissionMode::Auto, true),
        (PermissionMode::Auto, false),
        (PermissionMode::Default, true),
    ] {
        let setup = Setup::new()?;
        let (mut registry, _) = setup.registry(Exec::Gated);
        if with_browser {
            registry.register(BrowserTool::new(Arc::new(Host::default())));
        }
        let provider = RoutingProvider::new(vec![answer("ok")], Vec::new());
        let mut config = agent_loop_config(&setup, true);
        config.permission_mode_snapshot.mode = mode;
        agent(&setup, &MessageBus::new(), &registry, &provider, config)?
            .process_direct("hi", Some("cli:guide"))?;
        let requests = provider.assistant_requests();
        let system = requests
            .first()
            .and_then(|request| request.messages.first())
            .map(content)
            .unwrap_or_default();
        let label = format!("{mode:?} browser {with_browser}: {system}");
        let auto = mode == PermissionMode::Auto;
        assert_eq!(
            system.ends_with(&format!("{GUIDANCE}\n\n{BROWSER_GUIDANCE}")),
            auto && with_browser,
            "{label}"
        );
        assert_eq!(system.ends_with(GUIDANCE), auto && !with_browser, "{label}");
        assert_eq!(
            system.contains(BROWSER_GUIDANCE),
            auto && with_browser,
            "{label}"
        );
    }
    Ok(())
}

#[test]
fn reply_after_choices_reaches_classifier_as_user_message() -> TestResult {
    let setup = Setup::new()?;
    let (mut registry, _) = setup.registry(Exec::Gated);
    let host = Host::default();
    registry.register(BrowserTool::new(Arc::new(host.clone())));
    let choices = "장바구니에 우유 1개가 있습니다. 결제하기 / 계속 쇼핑 중 이름으로 답해 주세요.";
    let reply = "결제하기 눌러";
    let provider = RoutingProvider::new(
        vec![
            open("open"),
            answer(choices),
            browser("look", json!({"action":"look"})),
            browser("pay", json!({"action":"click","element":2,"revision":"r2"})),
            answer("결제 단계로 넘어갔습니다."),
        ],
        vec![Ok("no"), Ok("no"), Ok("no")],
    );
    let mut config = agent_loop_config(&setup, true);
    config.browser_host = Some(Arc::new(host.clone()));
    let bus = MessageBus::new();
    let mut agent = agent(&setup, &bus, &registry, &provider, config)?;

    let first = agent.process_message(inbound("chat-1", "장바구니 열어줘"))?;
    assert_eq!(first.final_content.as_deref(), Some(choices));
    assert!(host.state().clicks.is_empty());
    assert!(host.state().waiting);
    let before = provider.payloads().len();

    agent.process_message(inbound("chat-1", reply))?;

    assert_eq!(host.state().clicks, ["결제하기"]);
    let payloads = provider.payloads();
    assert!(payloads[..before]
        .iter()
        .all(|payload| !payload["transcript"].to_string().contains(reply)));
    let click = payloads.last().ok_or("classifier payload")?;
    let transcript = click["transcript"].as_array().ok_or("transcript")?;
    assert!(
        transcript.iter().any(|entry| entry["user"]
            .as_str()
            .is_some_and(|user| user.contains(reply))),
        "{click}"
    );
    assert!(
        transcript
            .iter()
            .filter(|entry| entry.to_string().contains(reply))
            .all(|entry| entry.get("user").is_some()),
        "{click}"
    );
    let requests = provider.assistant_requests();
    let last = requests.last().ok_or("assistant request")?;
    let carrying: Vec<&Value> = last
        .messages
        .iter()
        .filter(|message| content(message).contains(reply))
        .collect();
    assert_eq!(carrying.len(), 1, "{carrying:?}");
    assert_eq!(carrying[0]["role"], "user");
    let turns: Vec<String> = host
        .events()
        .into_iter()
        .filter(|event| event.starts_with("turn_"))
        .collect();
    assert_eq!(
        turns,
        [
            format!("turn_started {OWNER}"),
            format!("turn_finished {OWNER}"),
            format!("turn_started {OWNER}"),
            format!("turn_finished {OWNER}"),
        ]
    );
    Ok(())
}

#[test]
fn runtime_started_turns_leave_owner_reply_wait_untouched() -> TestResult {
    let setup = Setup::new()?;
    let (mut registry, _) = setup.registry(Exec::Gated);
    let host = Host::default();
    registry.register(BrowserTool::new(Arc::new(host.clone())));
    let channels = ["system", "automation", "cron", "heartbeat"];
    let mut responses = vec![open("open"), answer("골라 주세요.")];
    for channel in channels {
        responses.push(browser(
            &format!("{channel}-look"),
            json!({"action":"look"}),
        ));
        responses.push(answer("확인했습니다."));
    }
    responses.push(answer("네."));
    let provider = RoutingProvider::new(responses, Vec::new());
    let bus = MessageBus::new();
    let mut agent = agent(&setup, &bus, &registry, &provider, bypass(&setup, &host))?;

    agent.process_message(inbound("chat-1", "장바구니 열어줘"))?;

    let owner_turn = [
        format!("turn_started {OWNER}"),
        format!("turn_finished {OWNER}"),
    ];
    assert_eq!(turn_edges(&host), owner_turn);
    assert!(host.state().waiting);

    for channel in channels {
        let looked = host.state().sequence;

        agent.process_message(runtime_inbound(channel, OWNER))?;

        assert_eq!(host.state().sequence, looked + 1, "{channel}");
        assert_eq!(turn_edges(&host), owner_turn, "{channel}");
        assert!(host.state().waiting, "{channel}");
    }

    agent.process_message(inbound("chat-1", "고마워"))?;

    assert_eq!(turn_edges(&host), [owner_turn.clone(), owner_turn].concat());
    Ok(())
}

#[test]
fn tagged_turns_on_owner_channel_leave_owner_reply_wait_untouched() -> TestResult {
    let setup = Setup::new()?;
    let (mut registry, _) = setup.registry(Exec::Gated);
    let host = Host::default();
    registry.register(BrowserTool::new(Arc::new(host.clone())));
    let provider = RoutingProvider::new(
        vec![
            open("open"),
            answer("골라 주세요."),
            browser("goal-look", json!({"action":"look"})),
            answer("이어서 살펴봤습니다."),
            browser("route-look", json!({"action":"look"})),
            answer("결과를 확인했습니다."),
            answer("네."),
        ],
        Vec::new(),
    );
    let bus = MessageBus::new();
    let mut config = bypass(&setup, &host);
    config.durable_event_root = Some(setup.workspace.join("runtime/durable-events"));
    let mut agent = agent(&setup, &bus, &registry, &provider, config)?
        .with_goal_completion_evaluator(keeps_going());
    agent.process_message(inbound("chat-1", "/goal 장바구니를 끝까지 살펴봐"))?;

    agent.process_message(inbound("chat-1", "장바구니 열어줘"))?;

    let owner_turn = [
        format!("turn_started {OWNER}"),
        format!("turn_finished {OWNER}"),
    ];
    assert_eq!(turn_edges(&host), owner_turn);
    assert!(host.state().waiting);
    let continuation = queued_goal_continuation(&setup, &bus)?;
    let mut routed = InboundMessage::new(
        "telegram",
        "automation",
        "chat-1",
        "맡은 일의 결과를 확인해",
    );
    routed
        .metadata
        .insert("automation_route".to_owned(), json!("notify"));

    for (tag, message) in [
        ("goal_continuation", continuation),
        ("automation_route", routed),
    ] {
        assert_eq!(message.channel, "telegram", "{tag}");
        assert_eq!(message.session_key(), OWNER, "{tag}");
        assert!(message.metadata.contains_key(tag), "{tag}");
        let looked = host.state().sequence;

        agent.process_message(message)?;

        assert_eq!(host.state().sequence, looked + 1, "{tag}");
        assert_eq!(turn_edges(&host), owner_turn, "{tag}");
        assert!(host.state().waiting, "{tag}");
    }

    agent.process_message(inbound("chat-1", "고마워"))?;

    assert_eq!(turn_edges(&host), [owner_turn.clone(), owner_turn].concat());
    Ok(())
}

#[test]
fn automation_turn_opening_browser_starts_no_reply_wait() -> TestResult {
    let setup = Setup::new()?;
    let (mut registry, _) = setup.registry(Exec::Gated);
    let host = Host::default();
    registry.register(BrowserTool::new(Arc::new(host.clone())));
    let provider = RoutingProvider::new(vec![open("open"), answer("열었습니다.")], Vec::new());
    let bus = MessageBus::new();
    let mut agent = agent(&setup, &bus, &registry, &provider, bypass(&setup, &host))?;
    let session = "automation:job-1";

    agent.process_message(runtime_inbound("automation", session))?;

    assert_eq!(host.state().owner.as_deref(), Some(session));
    assert!(turn_edges(&host).is_empty(), "{:?}", turn_edges(&host));
    assert!(!host.state().waiting);
    Ok(())
}

#[test]
fn capture_sends_fresh_screen_without_model_image() -> TestResult {
    for (screenshots, attach) in [(Some(false), true), (None, false), (Some(true), true)] {
        let setup = Setup::new()?;
        let (first, second) = (capture_path(&setup, 1), capture_path(&setup, 2));
        let script = vec![
            open("open-1"),
            browser("capture-1", json!({"action":"capture"})),
            send("send-1", &first),
            open("open-2"),
            browser("capture-2", json!({"action":"capture"})),
            send("send-2", &second),
            answer("보냈습니다."),
        ];
        let run = capture_turn(&setup, Host::default(), script, screenshots, attach)?;
        let label = format!("screenshots {screenshots:?} attach {attach}");

        assert_eq!(run.outbound.len(), 2, "{label}: {:?}", run.outbound);
        for (index, (path, id)) in [(&first, "capture-1"), (&second, "capture-2")]
            .into_iter()
            .enumerate()
        {
            assert!(path.is_absolute(), "{label}");
            let result = tool_result(&run.provider, id);
            assert!(
                result.contains(&format!("browser_capture: {}", path.display())),
                "{label}: {result}"
            );
            assert!(!result.contains("image_url"), "{label}: {result}");
            let sent = &run.outbound[index];
            assert_eq!(
                (sent.channel.as_str(), sent.chat_id.as_str()),
                ("telegram", "chat-1")
            );
            assert_eq!(sent.media, [path.to_string_lossy()], "{label}");
            assert_eq!(
                fs::read(&sent.media[0])?,
                Host::screen(index + 1),
                "{label}"
            );
        }
        assert_ne!(fs::read(&first)?, fs::read(&second)?);
        for request in run.provider.assistant_requests() {
            let seen = serde_json::to_string(&request.messages)?;
            for sequence in [1, 2] {
                assert!(
                    !seen.contains(&STANDARD.encode(Host::screen(sequence))),
                    "{label}"
                );
            }
            if screenshots != Some(true) {
                assert!(!seen.contains("image_url"), "{label}: {seen}");
            }
        }
        assert_eq!(run.host.state().generation, 1, "{label}");
    }

    let setup = Setup::new()?;
    let host = Host::default();
    host.state().owner = Some("telegram:other".to_owned());
    let script = vec![
        browser("capture", json!({"action":"capture"})),
        answer("다른 세션이 브라우저를 쓰는 중입니다."),
    ];
    let run = capture_turn(&setup, host, script, Some(false), false)?;
    assert_eq!(
        tool_result(&run.provider, "capture"),
        refused(BrowserError::Busy)
    );
    assert!(run.outbound.iter().all(|message| message.media.is_empty()));
    assert!(!setup.media.join("tool-results").exists());

    for error in [BrowserError::DocumentChanged, BrowserError::StaleRevision] {
        let setup = Setup::new()?;
        let host = Host::default();
        host.state().capture_error = Some(error);
        let script = vec![
            open("open"),
            browser("capture", json!({"action":"capture"})),
            answer("화면을 보내지 못했습니다."),
        ];
        let run = capture_turn(&setup, host, script, Some(false), false)?;
        let result = tool_result(&run.provider, "capture");
        assert!(result.contains(&code(error)), "{result}");
        assert!(
            !result.contains("browser_capture") && !result.contains("tool-results"),
            "{result}"
        );
        assert!(run.outbound.iter().all(|message| message.media.is_empty()));
        assert!(!setup.media.join("tool-results").exists());
    }
    Ok(())
}

#[test]
fn new_and_stop_close_waiting_browser() -> TestResult {
    for command in ["/new", "/stop"] {
        let setup = Setup::new()?;
        let (mut registry, _) = setup.registry(Exec::Gated);
        let host = Host::default();
        registry.register(BrowserTool::new(Arc::new(host.clone())));
        let provider = RoutingProvider::new(
            vec![
                open("open"),
                answer("결제하기 / 계속 쇼핑 중 골라 주세요."),
                browser("first", json!({"action":"look"})),
                browser("second", json!({"action":"look"})),
                answer("페이지를 다시 열었습니다."),
            ],
            Vec::new(),
        );
        let bus = MessageBus::new();
        let mut agent = agent(&setup, &bus, &registry, &provider, bypass(&setup, &host))?;
        agent.process_message(inbound("chat-1", "장바구니 열어줘"))?;
        assert_eq!(host.state().owner.as_deref(), Some(OWNER), "{command}");
        assert!(host.state().waiting, "{command}");

        agent.process_message(inbound("chat-1", command))?;

        let closes: Vec<String> = host
            .events()
            .into_iter()
            .filter(|event| event.starts_with("close_"))
            .collect();
        assert_eq!(closes, [format!("close_for_session {OWNER}")], "{command}");
        assert!(host.state().owner.is_none(), "{command}");
        let token = host.closure_notice(OWNER).ok_or("closure notice")?.token;

        let result = agent.process_message(inbound("chat-1", "이어서 해줘"))?;

        assert_eq!(
            result.final_content.as_deref(),
            Some("페이지를 다시 열었습니다."),
            "{command}"
        );
        assert_eq!(
            tool_result(&provider, "first"),
            refused(BrowserError::Closed),
            "{command}"
        );
        assert!(
            tool_result(&provider, "second").contains("revision"),
            "{command}"
        );
        let requests = provider.assistant_requests();
        let last = requests.last().ok_or("assistant request")?;
        assert_eq!(mentions(last, "browser_closed:"), 1, "{command}");
        let acknowledged: Vec<String> = host
            .events()
            .into_iter()
            .filter(|event| event.starts_with("acknowledge"))
            .collect();
        assert_eq!(
            acknowledged,
            [format!("acknowledge {OWNER} {token}")],
            "{command}"
        );
        assert!(host.closure_notice(OWNER).is_none(), "{command}");
        assert_eq!(host.state().generation, 2, "{command}");
    }
    Ok(())
}

#[test]
fn browser_command_close_and_status_skip_model_turn() -> TestResult {
    let setup = Setup::new()?;
    let (mut registry, _) = setup.registry(Exec::Gated);
    let host = Host::default();
    registry.register(BrowserTool::new(Arc::new(host.clone())));
    let provider = RoutingProvider::new(
        vec![open("open"), answer("결제하기 / 계속 쇼핑 중 골라 주세요.")],
        Vec::new(),
    );
    let bus = MessageBus::new();
    let turn_lock = SessionTurnLock::new();
    let mut agent = agent(&setup, &bus, &registry, &provider, bypass(&setup, &host))?
        .with_session_turn_lock(turn_lock.clone());
    agent.process_message(inbound("chat-1", "장바구니 열어줘"))?;
    while bus.consume_outbound().is_some() {}
    let stored = || -> Result<Option<Value>, Box<dyn std::error::Error>> {
        Ok(SessionManager::new(&setup.workspace)?.read_session_file(OWNER))
    };
    let session_before = stored()?.ok_or("session file")?;
    let requests_before = provider.assistant_requests().len();

    let mut command = |chat: &str, text: &str| -> Result<String, Box<dyn std::error::Error>> {
        let result = agent.process_message(inbound(chat, text))?;
        assert_eq!(result.command, Some(AgentLoopCommandResult::Browser));
        assert_eq!(result.stop_reason, "browser");
        let sent = bus.consume_outbound().ok_or("command reply")?;
        assert!(bus.consume_outbound().is_none());
        assert_eq!(
            (sent.channel.as_str(), sent.chat_id.as_str()),
            ("telegram", chat)
        );
        assert_eq!(result.final_content.as_deref(), Some(sent.content.as_str()));
        Ok(sent.content)
    };

    assert_eq!(
        command("chat-1", "/browser status")?,
        format!("브라우저: 열림\n쓰는 세션: {OWNER}\n답 기다리기: 남은 30분\n{ASSISTANT_CONTROL}")
    );
    assert_eq!(
        command("chat-1", "/browser open")?,
        "사용법: /browser status | takeover | return [회차] | close"
    );
    assert_eq!(
        command("chat-1", "/browser")?,
        "사용법: /browser status | takeover | return [회차] | close"
    );

    let running = turn_lock
        .acquire(OWNER)
        .map_err(|error| format!("{error:?}"))?;
    host.state().action_running = true;
    assert_eq!(
        command("chat-1", "/browser close")?,
        "실행 중인 동작이 끝나면 브라우저를 닫습니다."
    );
    assert_eq!(
        command("chat-1", "/browser status")?,
        format!("브라우저: 열림\n쓰는 세션: {OWNER}\n답 기다리기: 남은 30분\n{ASSISTANT_CONTROL}")
    );
    assert!(turn_lock.acquire(OWNER).is_err());
    drop(running);
    host.state().action_running = false;

    let reservation = turn_lock.reserve(OWNER);
    assert_eq!(
        command("chat-2", "/browser close")?,
        "브라우저를 닫았습니다."
    );
    assert_eq!(
        host.closure_notice(OWNER).map(|notice| notice.error),
        Some(BrowserError::Closed)
    );
    assert_eq!(
        command("chat-2", "/browser close")?,
        "열린 브라우저가 없습니다."
    );
    assert_eq!(
        command("chat-2", "/browser status")?,
        format!("브라우저: 닫힘\n쓰는 세션: 없음\n답 기다리기: 아님\n{ASSISTANT_CONTROL}")
    );
    reservation.bind_to_current_thread();
    assert!(turn_lock.acquire(OWNER).is_ok());

    assert_eq!(stored()?.as_ref(), Some(&session_before));
    assert!(SessionManager::new(&setup.workspace)?
        .read_session_file("telegram:chat-2")
        .is_none());
    assert_eq!(provider.assistant_requests().len(), requests_before);
    assert!(provider.classifier_requests().is_empty());

    let mut hostless = bypass(&setup, &host);
    hostless.browser_host = None;
    let mut agent = self::agent(&setup, &bus, &registry, &provider, hostless)?;
    let status = agent.process_message(inbound("chat-3", "/browser status"))?;
    assert_eq!(
        status.final_content.as_deref(),
        Some(
            format!(
                "브라우저: 쓸 수 없음\n쓰는 세션: 없음\n답 기다리기: 아님\n{ASSISTANT_CONTROL}"
            )
            .as_str()
        )
    );
    let close = agent.process_message(inbound("chat-3", "/browser close"))?;
    assert_eq!(
        close.final_content.as_deref(),
        Some("열린 브라우저가 없습니다.")
    );
    Ok(())
}

#[test]
fn closure_notice_survives_auto_mode_prepares_until_execution() -> TestResult {
    for error in [BrowserError::Closed, BrowserError::ReplyWaitTimeout] {
        let setup = Setup::new()?;
        let (mut registry, _) = setup.registry(Exec::Gated);
        let host = Host::default();
        registry.register(BrowserTool::new(Arc::new(host.clone())));
        let token = host.notify(RUNNER_SESSION, error);
        let provider = RoutingProvider::new(
            vec![
                browser("invalid", json!({"action":"click"})),
                browser("first", json!({"action":"look"})),
                browser("second", json!({"action":"look"})),
                answer("done"),
            ],
            vec![Ok("no")],
        );
        let options = Options::auto(true);
        let mut run_spec = spec(&setup, &registry, &provider, &options, "show the page");
        run_spec.max_iterations = 6;
        run_spec.tool_context = runner_context(&setup, &options);

        let result = AgentRunner::new().run(run_spec)?;

        let code = code(error);
        assert_eq!(result.final_content.as_deref(), Some("done"), "{code}");
        let invalid = tool_result(&provider, "invalid");
        assert!(
            !invalid.is_empty() && !invalid.contains(&code),
            "{code}: {invalid}"
        );
        assert_eq!(tool_result(&provider, "first"), refused(error));
        assert!(
            tool_result(&provider, "second").contains("revision"),
            "{code}"
        );
        // The denied call reaches no prepare and leaves the notice; the next
        // call's classifier, diagnostic and execution prepares all see it.
        let notice = format!("prepare look {code}");
        assert_eq!(
            host.events(),
            [
                notice.clone(),
                notice.clone(),
                notice,
                format!("acknowledge {RUNNER_SESSION} {token}"),
                "prepare look ok".to_owned(),
                "prepare look ok".to_owned(),
                "prepare look ok".to_owned(),
            ]
        );
        let requests = provider.assistant_requests();
        let last = requests.last().ok_or("assistant request")?;
        assert_eq!(mentions(last, &format!("{code}:")), 1);
        assert_eq!(provider.classifier_requests().len(), 1, "{code}");
        assert!(host.closure_notice(RUNNER_SESSION).is_none());
    }

    let setup = Setup::new()?;
    let (mut registry, _) = setup.registry(Exec::Gated);
    let host = Host::default();
    registry.register(BrowserTool::new(Arc::new(host.clone())));
    let look = |id: &str, mode: PermissionMode| {
        support::run_report(
            &registry,
            &runner_context(&setup, &Options::mode(mode, true)),
            RuntimeToolCall::new(id, "browser", json!({"action":"look"})),
        )
    };

    let token = host.notify(RUNNER_SESSION, BrowserError::Closed);
    let report = look("default-1", PermissionMode::Default);
    assert_eq!(report.messages.len(), 1, "{report:?}");
    assert_eq!(report.messages[0].content, refused(BrowserError::Closed));
    assert!(report.interrupt.is_none() && report.executed_call_ids.is_empty());
    assert_eq!(
        host.events(),
        [
            "prepare look browser_closed".to_owned(),
            format!("acknowledge {RUNNER_SESSION} {token}"),
        ]
    );
    let report = look("default-2", PermissionMode::Default);
    assert!(report.interrupt.is_some(), "{report:?}");
    assert!(report.messages.is_empty(), "{report:?}");

    // The browser closes between the decision and the execution: the tool
    // returns the notice itself and acknowledges it.
    let report = look("bypass-1", PermissionMode::BypassPermissions);
    assert_eq!(report.executed_call_ids, ["bypass-1"]);
    host.state().events.clear();
    host.state().close_on_acquire = true;
    let report = look("bypass-2", PermissionMode::BypassPermissions);
    assert!(
        report.messages[0]
            .content
            .contains(&BrowserError::Closed.to_string()),
        "{report:?}"
    );
    let acknowledged: Vec<String> = host
        .events()
        .into_iter()
        .filter(|event| event.starts_with("acknowledge"))
        .collect();
    assert_eq!(acknowledged.len(), 1, "{acknowledged:?}");
    assert!(host.closure_notice(RUNNER_SESSION).is_none());
    let report = look("bypass-3", PermissionMode::BypassPermissions);
    assert!(
        report.messages[0].content.contains("revision"),
        "{report:?}"
    );
    Ok(())
}

#[test]
fn claimed_session_turn_refuses_other_turns_until_it_ends() -> TestResult {
    // Given: a session with no turn, which one thread claims for the turn it is about to start.
    let turn_lock = SessionTurnLock::new();
    let claim = turn_lock
        .try_reserve(OWNER)
        .ok_or("an idle session can be claimed")?;

    // When: another thread reserves the session for its own turn and tries to start it.
    let other = turn_lock.clone();
    let refused = std::thread::spawn(move || {
        let reservation = other.reserve(OWNER);
        reservation.bind_to_current_thread();
        other.acquire(OWNER).is_err()
    })
    .join()
    .map_err(|_| "the other thread panicked")?;

    // Then: it is refused, no second claim is given, and the claim is still the first thread's.
    assert!(refused);
    assert!(turn_lock.try_reserve(OWNER).is_none());
    let turn = turn_lock
        .acquire(OWNER)
        .map_err(|error| format!("the claiming thread was refused its turn: {error:?}"))?;

    // And: the session stays taken until that turn ends.
    assert!(turn_lock.try_reserve(OWNER).is_none());
    drop(turn);
    drop(claim);
    assert!(!turn_lock.is_busy(OWNER));
    assert!(turn_lock.try_reserve(OWNER).is_some());
    Ok(())
}

#[test]
fn stop_on_a_claimed_session_turn_reaches_only_its_claimer() -> TestResult {
    // Given: a session turn one thread claimed, and a stop that landed on the claim.
    let turn_lock = SessionTurnLock::new();
    let claim = turn_lock
        .try_reserve(OWNER)
        .ok_or("an idle session can be claimed")?;
    assert_eq!(
        turn_lock.cancel_active_or_reserved(OWNER),
        SessionTurnCancelOutcome::Reserved
    );

    // When: another thread reserves the session for its own turn and tries to start it.
    let other = turn_lock.clone();
    let refusal = std::thread::spawn(move || {
        let reservation = other.reserve(OWNER);
        reservation.bind_to_current_thread();
        other.acquire(OWNER).map(drop)
    })
    .join()
    .map_err(|_| "the other thread panicked")?;

    // Then: it is refused as a second turn, and the stop stays on the claim.
    assert_eq!(
        refusal,
        Err(SessionTurnAcquireError::AlreadyActive {
            session_key: OWNER.to_owned()
        })
    );
    assert!(turn_lock.is_busy(OWNER));

    // When: the claiming thread starts its turn.
    let started = turn_lock.acquire(OWNER).map(drop);

    // Then: the stop reaches it, so nothing runs and the session is free again.
    assert_eq!(
        started,
        Err(SessionTurnAcquireError::Cancelled {
            session_key: OWNER.to_owned()
        })
    );
    drop(claim);
    assert!(!turn_lock.is_busy(OWNER));
    Ok(())
}

#[test]
fn priority_commands_leave_a_claimed_session_turn_to_its_claimer() -> TestResult {
    let setup = Setup::new()?;
    let (registry, _) = setup.registry(Exec::Gated);
    let host = Host::default();
    let provider = RoutingProvider::new(Vec::new(), Vec::new());
    let bus = MessageBus::new();
    let turn_lock = SessionTurnLock::new();
    let mut agent = agent(&setup, &bus, &registry, &provider, bypass(&setup, &host))?
        .with_session_turn_lock(turn_lock.clone());
    let cancelled = Err(SessionTurnAcquireError::Cancelled {
        session_key: OWNER.to_owned(),
    });
    for (command, answered, started) in [
        ("/status", AgentLoopCommandResult::Status, Ok(())),
        ("/restart", AgentLoopCommandResult::RestartRequested, Ok(())),
        ("/stop", AgentLoopCommandResult::StopRequested, cancelled),
    ] {
        // Given: a session turn a worker thread claimed and has not started yet.
        let (claimed_tx, claimed_rx) = mpsc::channel();
        let (start_tx, start_rx) = mpsc::channel::<()>();
        let worker = std::thread::spawn({
            let turn_lock = turn_lock.clone();
            move || {
                let claim = turn_lock.try_reserve(OWNER);
                let _ = claimed_tx.send(claim.is_some());
                let _ = start_rx.recv();
                turn_lock.acquire(OWNER).map(drop)
            }
        });
        assert!(claimed_rx.recv()?, "{command}");

        // When: the priority command arrives on another thread.
        let result = agent.process_message(inbound("chat-1", command))?;

        // Then: it is answered as for a busy session. It neither takes the claimed turn nor
        // stores the session, and only a stop cancels the claim.
        assert_eq!(result.command, Some(answered), "{command}");
        assert!(bus.consume_outbound().is_some(), "{command}");
        assert_eq!(
            turn_lock.acquire_priority(OWNER).map(drop),
            Err(SessionTurnAcquireError::AlreadyActive {
                session_key: OWNER.to_owned()
            }),
            "{command}"
        );
        assert!(
            SessionManager::new(&setup.workspace)?
                .read_session_file(OWNER)
                .is_none(),
            "{command}"
        );
        start_tx.send(())?;
        let turn = worker.join().map_err(|_| "the worker thread panicked")?;
        assert_eq!(turn, started, "{command}");
        assert!(!turn_lock.is_busy(OWNER), "{command}");
    }
    assert!(provider.assistant_requests().is_empty());
    Ok(())
}
