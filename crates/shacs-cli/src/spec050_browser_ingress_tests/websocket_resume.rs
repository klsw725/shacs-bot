//! Spec 050 PRD 010 over WebSocket: a `handoff` that paused on a connection goes on there
//! when its episode ends, through the real WebSocket server and the desk's adapter.

use super::super::stage::Timing;
use super::super::{eventually, lock, TestResult, PATIENCE};
use super::{
    assistant, resume_id, Desk, Model, ENDINGS, LIMIT_MS, QUESTION, RETURNED, ROOM_A, ROOM_B,
    SESSION_A,
};
use crate::browser::BrowserOpenEpisode;
use crate::tests::public_adapter_test_bundle;
use crate::{
    install_browser_resume_notifier, web_ui_adapter, AgentLoopChatCompletionAdapter,
    ExternalChannelSupervisor, RuntimeShutdownReason,
};
use serde_json::{json, Value};
use shacs_api::{ChatCompletionAdapter, WebSocketRuntimeTurn, WebSocketRuntimeTurnEnd};
use shacs_channels::{WebSocketServerEvent, WEBSOCKET_CHANNEL};
use shacs_core::runtime::{
    BrowserArrival, BrowserCallContext, BrowserCapture, BrowserCloseOutcome, BrowserClosureNotice,
    BrowserDescription, BrowserError, BrowserHandoffEpisode, BrowserHost, BrowserPendingResume,
    BrowserPreparedAction, BrowserResumeNotifier, BrowserReturnOutcome, BrowserReturnTarget,
    BrowserSession, BrowserTakeover, BrowserTakeoverError, BrowserTakeoverRequest,
    SessionTurnGuard, SessionTurnLock,
};
use shacs_session::durable_work::WorkTerminalKind;
use std::error::Error;
use std::io::ErrorKind;
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};
use tungstenite::stream::MaybeTlsStream;
use tungstenite::{Message, WebSocket};

/// The chat every connection speaks in unless its frame names another.
const CHAT: &str = "default";
const SESSION: &str = "websocket:default";
const OTHER_CHAT: &str = "other";

/// The gateway's WebSocket server over the desk's adapter.
struct Served {
    url: String,
    stop: mpsc::Sender<()>,
    server: thread::JoinHandle<Result<(), String>>,
}

impl Served {
    fn start(desk: &Desk) -> Result<Self, Box<dyn Error>> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        listener.set_nonblocking(true)?;
        let url = format!(
            "ws://{}{}",
            listener.local_addr()?,
            shacs_api::WEBSOCKET_PATH
        );
        let (stop, stopped) = mpsc::channel();
        let adapter = desk.adapter.clone();
        let server = thread::spawn(move || serve(listener, adapter, stopped));
        Ok(Self { url, stop, server })
    }

    fn connect(&self) -> Result<Socket, Box<dyn Error>> {
        let (stream, _) = tungstenite::connect(self.url.as_str())?;
        if let MaybeTlsStream::Plain(tcp) = stream.get_ref() {
            tcp.set_read_timeout(Some(Duration::from_millis(50)))?;
        }
        Ok(Socket { stream, sent: 0 })
    }

    /// Stops the server, which waits until every connection closed.
    fn stop(self) -> TestResult {
        let _ = self.stop.send(());
        self.server
            .join()
            .map_err(|_| "the WebSocket server panicked")??;
        Ok(())
    }
}

fn serve(
    listener: TcpListener,
    adapter: Arc<AgentLoopChatCompletionAdapter>,
    stopped: mpsc::Receiver<()>,
) -> Result<(), String> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|error| error.to_string())?;
    runtime.block_on(async move {
        let listener =
            tokio::net::TcpListener::from_std(listener).map_err(|error| error.to_string())?;
        shacs_api::serve_websocket_listener_with_timeout_and_path(
            listener,
            adapter,
            Duration::from_secs(30),
            shacs_api::WEBSOCKET_PATH,
            async move {
                let _ = tokio::task::spawn_blocking(move || stopped.recv()).await;
            },
        )
        .await
        .map_err(|error| error.to_string())
    })
}

/// One client connection, and how many frames it sent.
struct Socket {
    stream: WebSocket<MaybeTlsStream<TcpStream>>,
    sent: usize,
}

impl Socket {
    /// Sends a message, in the connection's own chat unless `chat` names another.
    fn say(&mut self, chat: Option<&str>, text: &str) -> TestResult {
        let mut frame = json!({"type":"message","content":text});
        if let Some(chat) = chat {
            frame["chat_id"] = json!(chat);
        }
        self.stream.send(Message::text(frame.to_string()))?;
        self.sent += 1;
        Ok(())
    }

    /// The next message of `chat` that begins with `start`.
    fn hear(&mut self, chat: &str, start: &str) -> Result<String, Box<dyn Error>> {
        let deadline = Instant::now() + PATIENCE;
        let mut heard = Vec::new();
        while Instant::now() < deadline {
            match self.event()? {
                Some(WebSocketServerEvent::Message { chat_id, text, .. })
                    if chat_id == chat && text.starts_with(start) =>
                {
                    return Ok(text);
                }
                Some(WebSocketServerEvent::Error { detail, .. }) => {
                    return Err(format!("{chat} was sent an error: {detail:?}").into());
                }
                Some(event) => heard.push(event),
                None => {}
            }
        }
        Err(format!("{chat} never heard {start:?}; heard {heard:#?}").into())
    }

    /// The next event, when one comes within the read timeout. A frame of the channel's
    /// own state has none.
    fn event(&mut self) -> Result<Option<WebSocketServerEvent>, Box<dyn Error>> {
        let frame = match self.stream.read() {
            Ok(frame) => frame,
            Err(tungstenite::Error::Io(error)) if idle(&error) => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        if !frame.is_text() {
            return Ok(None);
        }
        let frame: Value = serde_json::from_str(frame.into_text()?.as_str())?;
        if frame.get("event").is_none() {
            return Ok(None);
        }
        Ok(Some(serde_json::from_value(frame)?))
    }

    /// Closes the connection and waits until the server let go of it.
    fn close(mut self) -> TestResult {
        self.stream.close(None)?;
        let deadline = Instant::now() + PATIENCE;
        loop {
            match self.stream.read() {
                Ok(_) => {}
                Err(tungstenite::Error::Io(error)) if idle(&error) => {}
                Err(_) => return Ok(()),
            }
            if Instant::now() >= deadline {
                return Err("the server kept the connection".into());
            }
        }
    }
}

/// Whether a read only ran into the socket's read timeout.
fn idle(error: &std::io::Error) -> bool {
    matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut)
}

/// Asks the assistant of the connection's own chat to hand over, and waits for the
/// question to be written to the connection.
fn hand_over(
    desk: &Desk,
    socket: &mut Socket,
    text: &str,
) -> Result<BrowserOpenEpisode, Box<dyn Error>> {
    socket.say(None, text)?;
    socket.hear(CHAT, QUESTION)?;
    let open = desk.asked()?;
    assert!(open.paused, "{open:?}");
    Ok(open)
}

fn turn(chat: &str, episode: &str) -> WebSocketRuntimeTurn {
    WebSocketRuntimeTurn {
        chat_id: chat.to_owned(),
        token: episode.to_owned(),
    }
}

/// The adapter's browser host, with another turn in it. Once aimed at a session, that
/// turn takes the session where the next turn stamps its message: after its caller saw
/// the session free, and before the turn itself starts.
struct Rival {
    host: Arc<dyn BrowserHost>,
    turns: SessionTurnLock,
    aimed: Mutex<Option<String>>,
    /// The turn it took, and how many continuations had been looked up by then.
    taken: Mutex<Option<(SessionTurnGuard, usize)>>,
    /// How often the continuation of an episode was looked up.
    looked: AtomicUsize,
}

impl Rival {
    /// Puts itself between the desk's adapter and its browser host.
    fn over(desk: &mut Desk) -> Result<Arc<Self>, Box<dyn Error>> {
        let adapter = Arc::get_mut(&mut desk.adapter).ok_or("the adapter is already shared")?;
        let rival = Arc::new(Self {
            host: desk.ear.host.clone(),
            turns: adapter.session_turn_lock.clone(),
            aimed: Mutex::new(None),
            taken: Mutex::new(None),
            looked: AtomicUsize::new(0),
        });
        adapter.browser_host = Some(rival.clone());
        Ok(rival)
    }

    fn aim(&self, session: &str) {
        *lock(&self.aimed) = Some(session.to_owned());
    }

    /// Whether it has the session, and a continuation was looked up since it took it.
    fn outlasted_a_try(&self) -> bool {
        lock(&self.taken)
            .as_ref()
            .is_some_and(|(_, looked)| self.looked.load(Ordering::SeqCst) > *looked)
    }

    /// Ends its turn.
    fn leave(&self) {
        let taken = lock(&self.taken).take();
        drop(taken);
    }
}

impl BrowserHost for Rival {
    fn close_prepared(
        &self,
        context: &BrowserCallContext,
        digest: &str,
    ) -> Result<(), BrowserError> {
        self.host.close_prepared(context, digest)
    }

    fn prepare(
        &self,
        session_id: &str,
        arguments: &Value,
    ) -> Result<BrowserPreparedAction, BrowserError> {
        self.host.prepare(session_id, arguments)
    }

    fn acquire(
        &self,
        context: &BrowserCallContext,
    ) -> Result<Box<dyn BrowserSession>, BrowserError> {
        self.host.acquire(context)
    }

    fn capture_prepared(
        &self,
        context: &BrowserCallContext,
        digest: &str,
    ) -> Result<BrowserCapture, BrowserError> {
        self.host.capture_prepared(context, digest)
    }

    fn turn_started(&self, session_id: &str) {
        self.host.turn_started(session_id);
    }

    fn turn_finished(&self, session_id: &str) {
        self.host.turn_finished(session_id);
    }

    fn closure_notice(&self, session_id: &str) -> Option<BrowserClosureNotice> {
        self.host.closure_notice(session_id)
    }

    fn acknowledge_closure_notice(&self, session_id: &str, token: &str) {
        self.host.acknowledge_closure_notice(session_id, token);
    }

    fn close_for_session(&self, session_id: &str) -> bool {
        self.host.close_for_session(session_id)
    }

    fn close_by_user(&self) -> BrowserCloseOutcome {
        self.host.close_by_user()
    }

    fn describe(&self) -> BrowserDescription {
        self.host.describe()
    }

    fn takeover(
        &self,
        request: &BrowserTakeoverRequest,
    ) -> Result<BrowserTakeover, BrowserTakeoverError> {
        self.host.takeover(request)
    }

    fn episode_notice_delivered(&self, episode: &str, message_id: Option<&str>) {
        self.host.episode_notice_delivered(episode, message_id);
    }

    fn arrival(&self) -> BrowserArrival {
        let aimed = lock(&self.aimed).take();
        if let Some(session) = aimed {
            let looked = self.looked.load(Ordering::SeqCst);
            let turn = self.turns.acquire(session).ok();
            *lock(&self.taken) = turn.map(|turn| (turn, looked));
        }
        self.host.arrival()
    }

    fn handoff_episode(&self, session_id: &str, tool_call_id: &str) -> BrowserHandoffEpisode {
        self.host.handoff_episode(session_id, tool_call_id)
    }

    fn finish_handoff(&self, session_id: &str, tool_call_id: &str, episode: &str) -> bool {
        self.host.finish_handoff(session_id, tool_call_id, episode)
    }

    fn return_takeover(&self, target: &BrowserReturnTarget) -> BrowserReturnOutcome {
        self.host.return_takeover(target)
    }

    fn cancel_takeover(&self, session_id: &str) {
        self.host.cancel_takeover(session_id);
    }

    fn pending_resume(&self, episode: &str) -> Option<BrowserPendingResume> {
        self.looked.fetch_add(1, Ordering::SeqCst);
        self.host.pending_resume(episode)
    }

    fn set_resume_notifier(&self, notifier: BrowserResumeNotifier) {
        self.host.set_resume_notifier(notifier);
    }

    fn resume_settled(&self, episode: &str) {
        self.host.resume_settled(episode);
    }

    fn handoff_closed(&self, session_id: &str, tool_call_id: &str) {
        self.host.handoff_closed(session_id, tool_call_id);
    }
}

#[test]
fn websocket_only_gateway_continues_handoff_without_a_user_message() -> TestResult {
    // Given: a gateway with no external channel, and a `handoff` paused on a connection.
    let desk = Desk::idle(Model::new(assistant), Timing::default())?;
    let channels = ExternalChannelSupervisor::start(
        desk.adapter.clone(),
        Vec::new(),
        1,
        desk.context.clone(),
        &desk.gateway,
    )?;
    let served = Served::start(&desk)?;
    let mut owner = served.connect()?;
    let mut other = served.connect()?;
    let first = hand_over(&desk, &mut owner, "넘겨줘")?;
    assert_eq!(desk.paused_call(SESSION).as_deref(), Some("handoff-0"));

    // When: another chat returns the browser by the episode's label.
    other.say(
        Some(OTHER_CHAT),
        &format!("/browser return {}", first.label),
    )?;

    // Then: the paused call goes on, on the connection that handed over.
    other.hear(OTHER_CHAT, RETURNED)?;
    owner.hear(CHAT, "이어서: browser_returned")?;
    assert!(
        desk.last_asked().contains(ENDINGS[0]),
        "{}",
        desk.last_asked()
    );
    assert_eq!(desk.paused_call(SESSION), None);
    assert_eq!(desk.records().waiting, []);
    assert_eq!(
        desk.settled(&first.episode).map(|record| record.reason),
        Some("returned".to_owned())
    );

    // Given: a second handoff of the same connection.
    let second = hand_over(&desk, &mut owner, "또 넘겨줘")?;
    let requests = desk.requests();

    // When: its limit passes.
    desk.stage.advance(LIMIT_MS);

    // Then: that call goes on too, and the connection sent nothing but the two requests.
    owner.hear(CHAT, "이어서: browser_takeover_timeout")?;
    assert!(
        desk.last_asked().contains(ENDINGS[1]),
        "{}",
        desk.last_asked()
    );
    assert_eq!(desk.requests(), requests + 1);
    assert_eq!(desk.records().waiting, []);
    assert!(desk.settled(&second.episode).is_some());
    assert_eq!(owner.sent, 2);

    owner.close()?;
    other.close()?;
    served.stop()?;
    channels.stop(RuntimeShutdownReason::Stop)?;
    Ok(())
}

#[test]
fn websocket_handoff_beside_external_channels_continues_on_its_connection() -> TestResult {
    // Given: a gateway with Telegram and WebSocket, and a `handoff` paused on a
    // connection.
    let desk = Desk::start(Model::new(assistant))?;
    install_browser_resume_notifier(&desk.adapter, Some((&desk.context, &desk.bus)));
    let served = Served::start(&desk)?;
    let mut owner = served.connect()?;
    let first = hand_over(&desk, &mut owner, "넘겨줘")?;

    // When: a Telegram room returns the browser by the episode's label.
    desk.say(ROOM_B, &format!("/browser return {}", first.label));

    // Then: the call goes on, on its connection and not through the external channels.
    desk.hear(ROOM_B, RETURNED)?;
    owner.hear(CHAT, "이어서: browser_returned")?;
    assert_eq!(desk.records().waiting, []);
    assert!(desk.settled(&first.episode).is_some());

    // Given: a second handoff of the same connection.
    let second = hand_over(&desk, &mut owner, "또 넘겨줘")?;
    let requests = desk.requests();

    // When: its limit passes.
    desk.stage.advance(LIMIT_MS);

    // Then: that call goes on the same way, and the connection sent only the two requests.
    owner.hear(CHAT, "이어서: browser_takeover_timeout")?;
    assert_eq!(desk.requests(), requests + 1);
    assert!(desk.settled(&second.episode).is_some());
    assert_eq!(owner.sent, 2);
    assert_eq!(desk.resumes()?.len(), 0, "{:#?}", desk.resumes()?);
    let stray = lock(&desk.wire.line)
        .tried
        .iter()
        .filter(|message| message.channel == WEBSOCKET_CHANNEL)
        .cloned()
        .collect::<Vec<_>>();
    assert_eq!(stray, []);

    owner.close()?;
    served.stop()
}

#[test]
fn websocket_turn_leaves_an_external_room_handoff_to_the_durable_line() -> TestResult {
    // Given: a gateway with Telegram and WebSocket, and a handoff of a Telegram room whose
    // continuation waits on the durable line behind a busy session.
    let desk = Desk::start(Model::new(assistant))?;
    install_browser_resume_notifier(&desk.adapter, Some((&desk.context, &desk.bus)));
    let (open, _) = desk.hand_over("넘겨줘")?;
    let held = desk.hold(SESSION_A)?;
    desk.say(ROOM_B, &format!("/browser return {}", open.label));
    desk.hear(ROOM_B, RETURNED)?;
    eventually("the continuation to be put on the line", || {
        desk.item(&resume_id(&open.episode)).is_some()
    })?;
    let requests = desk.requests();

    // When: a WebSocket turn names that episode.
    let end = desk
        .adapter
        .process_websocket_browser_resume(&turn(ROOM_A, &open.episode), &mut |_| true)?;

    // Then: it does nothing, and the call goes on through the durable line.
    assert_eq!(end, WebSocketRuntimeTurnEnd::Done);
    assert_eq!(desk.requests(), requests);
    assert!(
        desk.waiting(&open.episode).is_some(),
        "{:#?}",
        desk.records()
    );
    drop(held);
    desk.hear(ROOM_A, "이어서: browser_returned")?;
    desk.settles(&resume_id(&open.episode), WorkTerminalKind::Succeeded)
}

#[test]
fn websocket_handoff_waits_for_a_connection_and_a_free_session() -> TestResult {
    // Given: a `handoff` paused on a connection that closed since.
    let desk = Desk::idle(Model::new(assistant), Timing::default())?;
    install_browser_resume_notifier(&desk.adapter, None);
    let served = Served::start(&desk)?;
    let mut owner = served.connect()?;
    let open = hand_over(&desk, &mut owner, "넘겨줘")?;
    owner.close()?;
    let requests = desk.requests();

    // When: the limit passes while no connection serves the chat.
    desk.stage.advance(LIMIT_MS);
    desk.stage.upkeep();

    // Then: the call still waits, and nothing ran.
    assert!(
        desk.waiting(&open.episode).is_some(),
        "{:#?}",
        desk.records()
    );
    assert_eq!(desk.paused_call(SESSION).as_deref(), Some("handoff-0"));
    assert_eq!(desk.requests(), requests);

    // Given: another turn of the session runs.
    let held = desk.hold(SESSION)?;

    // When: the turn of that call is run, and turns that name another chat or no episode.
    let adapter = &desk.adapter;
    let busy =
        adapter.process_websocket_browser_resume(&turn(CHAT, &open.episode), &mut |_| true)?;
    let elsewhere = adapter
        .process_websocket_browser_resume(&turn(OTHER_CHAT, &open.episode), &mut |_| true)?;
    let unknown =
        adapter.process_websocket_browser_resume(&turn(CHAT, "no-episode"), &mut |_| true)?;

    // Then: the first is to be tried again, the others are over, and the call still waits.
    assert_eq!(busy, WebSocketRuntimeTurnEnd::Retry);
    assert_eq!(elsewhere, WebSocketRuntimeTurnEnd::Done);
    assert_eq!(unknown, WebSocketRuntimeTurnEnd::Done);
    assert!(
        desk.waiting(&open.episode).is_some(),
        "{:#?}",
        desk.records()
    );
    assert_eq!(desk.requests(), requests);

    // When: a connection opens while the session is busy, and the session then frees.
    let mut next = served.connect()?;
    desk.stage.upkeep();
    assert_eq!(desk.requests(), requests);
    assert!(
        desk.waiting(&open.episode).is_some(),
        "{:#?}",
        desk.records()
    );
    drop(held);

    // Then: the call goes on, on the new connection, which sent nothing.
    next.hear(CHAT, "이어서: browser_takeover_timeout")?;
    assert!(
        desk.last_asked().contains(ENDINGS[1]),
        "{}",
        desk.last_asked()
    );
    assert_eq!(desk.requests(), requests + 1);
    assert_eq!(desk.records().waiting, []);
    assert_eq!(next.sent, 0);

    next.close()?;
    served.stop()
}

#[test]
fn websocket_resume_that_loses_its_session_after_the_check_is_handed_out_again() -> TestResult {
    // Given: a `handoff` paused on a connection, and another turn that takes the session
    // right after the continuation saw it free.
    let mut desk = Desk::idle(Model::new(assistant), Timing::default())?;
    let rival = Rival::over(&mut desk)?;
    install_browser_resume_notifier(&desk.adapter, None);
    let served = Served::start(&desk)?;
    let mut owner = served.connect()?;
    let open = hand_over(&desk, &mut owner, "넘겨줘")?;
    let requests = desk.requests();
    rival.aim(SESSION);

    // When: the limit passes, and the continuation is offered until it was tried again
    // while the other turn runs.
    desk.stage.advance(LIMIT_MS);
    eventually("the continuation to be tried again", || {
        desk.stage.upkeep();
        rival.outlasted_a_try()
    })?;

    // Then: nothing ran, and the call still waits.
    assert_eq!(desk.requests(), requests);
    assert!(
        desk.waiting(&open.episode).is_some(),
        "{:#?}",
        desk.records()
    );
    assert_eq!(desk.paused_call(SESSION).as_deref(), Some("handoff-0"));

    // When: the other turn ends.
    rival.leave();

    // Then: the call goes on once, on its connection, which sent only the request.
    owner.hear(CHAT, "이어서: browser_takeover_timeout")?;
    desk.stage.upkeep();
    assert!(
        desk.last_asked().contains(ENDINGS[1]),
        "{}",
        desk.last_asked()
    );
    assert_eq!(desk.requests(), requests + 1);
    assert_eq!(desk.records().waiting, []);
    assert!(desk.settled(&open.episode).is_some());
    assert_eq!(desk.paused_call(SESSION), None);
    assert_eq!(owner.sent, 1);

    owner.close()?;
    served.stop()
}

#[test]
fn web_ui_adapter_has_no_browser_tool_and_no_browser_host() -> TestResult {
    // Given: a configuration whose browser is enabled.
    let root = tempfile::tempdir()?;
    let mut bundle = public_adapter_test_bundle(root.path(), false)?;
    bundle.config.tools.browser.enable = true;

    // When: `web` builds its adapter.
    let adapter = web_ui_adapter(bundle, false)?;

    // Then: no `handoff` can pause there, and its connections take no turn of the runtime.
    assert!(
        !adapter.tools.has("browser"),
        "{:?}",
        adapter.tools.tool_names()
    );
    assert!(adapter.browser_host.is_none());
    assert!(adapter.websocket_runtime_turns().is_none());
    Ok(())
}
