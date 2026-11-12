//! Turns the runtime starts on a WebSocket connection by itself: the adapter
//! has work for a chat that no frame of the client asked for, such as going
//! on with a paused call (Spec 050 PRD 010 scope 9).

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

/// How many chats one connection is remembered to serve.
const SERVED_CHATS: usize = 32;
/// How many ended turns are remembered so they are not handed out again.
const CLOSED_TURNS: usize = 64;

/// A turn the adapter wants to run on a connection that serves `chat_id`.
/// `token` is the adapter's own name for the turn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WebSocketRuntimeTurn {
    pub chat_id: String,
    pub token: String,
}

/// How a connection's run of a turn ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WebSocketRuntimeTurnEnd {
    /// Nothing is left to do for the turn. It is not handed out again.
    Done,
    /// The turn could not start now. The next offer hands it out again.
    Retry,
}

struct Connection {
    id: u64,
    /// The chats it serves, each with when it last wrote an event of the chat.
    chats: Vec<(String, u64)>,
    turns: mpsc::UnboundedSender<WebSocketRuntimeTurn>,
}

#[derive(Default)]
struct Connections {
    /// Counts connections and written events: what came later has the larger number.
    clock: u64,
    open: Vec<Connection>,
    /// The turns a connection was handed and did not end yet, each with its connection.
    handed: Vec<(String, u64)>,
    closed: VecDeque<String>,
}

/// The WebSocket connections of one adapter, as the places its own turns can
/// run. The adapter offers a turn as often as it likes; a turn runs on one
/// connection at a time, and one that ended as done never runs again. The
/// lock is a leaf: nothing is called while it is held.
#[derive(Default)]
pub struct WebSocketRuntimeTurns {
    connections: Mutex<Connections>,
}

impl WebSocketRuntimeTurns {
    /// Hands `turn` to the connection that last wrote an event of its chat,
    /// unless a connection has it already or it ended as done. When no
    /// connection serves the chat nothing happens: the caller offers again.
    ///
    /// # Panics
    ///
    /// Panics when the lock on the connections is poisoned.
    pub fn offer(&self, turn: &WebSocketRuntimeTurn) {
        let mut connections = self
            .connections
            .lock()
            .expect("websocket runtime turns lock poisoned");
        let connections = &mut *connections;
        if connections.closed.contains(&turn.token)
            || connections
                .handed
                .iter()
                .any(|(token, _)| *token == turn.token)
        {
            return;
        }
        let serving = connections
            .open
            .iter()
            .filter_map(|connection| {
                let (_, wrote) = connection
                    .chats
                    .iter()
                    .find(|(chat, _)| *chat == turn.chat_id)?;
                Some((*wrote, connection))
            })
            .max_by_key(|(wrote, _)| *wrote);
        let Some((_, connection)) = serving else {
            return;
        };
        if connection.turns.send(turn.clone()).is_ok() {
            connections.handed.push((turn.token.clone(), connection.id));
        }
    }

    /// A connection opens. It serves `default_chat_id` from now on, behind
    /// every connection that wrote an event of that chat.
    pub(crate) fn attach(self: &Arc<Self>, default_chat_id: &str) -> WebSocketRuntimeTurnSeat {
        let (turns, offered) = mpsc::unbounded_channel();
        let mut connections = self
            .connections
            .lock()
            .expect("websocket runtime turns lock poisoned");
        connections.clock += 1;
        let id = connections.clock;
        connections.open.push(Connection {
            id,
            chats: vec![(default_chat_id.to_owned(), 0)],
            turns,
        });
        WebSocketRuntimeTurnSeat {
            turns: self.clone(),
            id,
            offered,
        }
    }

    /// The run of `turn` ended. Its guard calls this while it is dropped.
    fn finish(&self, turn: &WebSocketRuntimeTurn, end: WebSocketRuntimeTurnEnd) {
        if let Ok(mut connections) = self.connections.lock() {
            connections.handed.retain(|(token, _)| *token != turn.token);
            if end == WebSocketRuntimeTurnEnd::Done {
                connections.closed.push_back(turn.token.clone());
                while connections.closed.len() > CLOSED_TURNS {
                    connections.closed.pop_front();
                }
            }
        }
    }
}

/// One connection's place among them. Dropping it takes the connection out,
/// and a turn it was handed and did not end can be handed to another.
pub(crate) struct WebSocketRuntimeTurnSeat {
    turns: Arc<WebSocketRuntimeTurns>,
    id: u64,
    offered: mpsc::UnboundedReceiver<WebSocketRuntimeTurn>,
}

impl WebSocketRuntimeTurnSeat {
    /// The next turn handed to this connection.
    pub(crate) async fn next(&mut self) -> Option<WebSocketRuntimeTurn> {
        self.offered.recv().await
    }

    /// The connection wrote an event of `chat_id`, so it is the one that
    /// serves the chat now.
    pub(crate) fn wrote(&self, chat_id: &str) {
        let mut connections = self
            .turns
            .connections
            .lock()
            .expect("websocket runtime turns lock poisoned");
        let connections = &mut *connections;
        connections.clock += 1;
        let now = connections.clock;
        let Some(connection) = connections
            .open
            .iter_mut()
            .find(|connection| connection.id == self.id)
        else {
            return;
        };
        if let Some((_, wrote)) = connection
            .chats
            .iter_mut()
            .find(|(chat, _)| chat == chat_id)
        {
            *wrote = now;
            return;
        }
        if connection.chats.len() >= SERVED_CHATS {
            let longest_ago = connection
                .chats
                .iter()
                .enumerate()
                .min_by_key(|(_, (_, wrote))| *wrote)
                .map(|(index, _)| index);
            if let Some(index) = longest_ago {
                connection.chats.swap_remove(index);
            }
        }
        connection.chats.push((chat_id.to_owned(), now));
    }

    /// The connection does not run `turn` now. The next offer hands it out again.
    pub(crate) fn give_back(&self, turn: WebSocketRuntimeTurn) {
        self.run(turn).ended(WebSocketRuntimeTurnEnd::Retry);
    }

    /// Starts this connection's run of `turn`. Dropping the guard ends it.
    pub(crate) fn run(&self, turn: WebSocketRuntimeTurn) -> WebSocketRuntimeTurnRun {
        WebSocketRuntimeTurnRun {
            turns: self.turns.clone(),
            turn,
            end: WebSocketRuntimeTurnEnd::Done,
        }
    }
}

impl Drop for WebSocketRuntimeTurnSeat {
    fn drop(&mut self) {
        if let Ok(mut connections) = self.turns.connections.lock() {
            connections
                .open
                .retain(|connection| connection.id != self.id);
            connections.handed.retain(|(_, id)| *id != self.id);
        }
    }
}

/// One run of a turn. It ends as done however the work stops, a panic
/// included, unless the work said the turn could not start.
pub(crate) struct WebSocketRuntimeTurnRun {
    turns: Arc<WebSocketRuntimeTurns>,
    turn: WebSocketRuntimeTurn,
    end: WebSocketRuntimeTurnEnd,
}

impl WebSocketRuntimeTurnRun {
    pub(crate) fn turn(&self) -> &WebSocketRuntimeTurn {
        &self.turn
    }

    pub(crate) fn ended(&mut self, end: WebSocketRuntimeTurnEnd) {
        self.end = end;
    }
}

impl Drop for WebSocketRuntimeTurnRun {
    fn drop(&mut self) {
        self.turns.finish(&self.turn, self.end);
    }
}

#[cfg(test)]
mod tests {
    use super::{
        WebSocketRuntimeTurn, WebSocketRuntimeTurnEnd, WebSocketRuntimeTurnSeat,
        WebSocketRuntimeTurns, CLOSED_TURNS, SERVED_CHATS,
    };
    use crate::{
        serve_websocket_listener_with_timeout_and_path, ApiError, ChatCompletionAdapter,
        ChatCompletionInvocation, WEBSOCKET_PATH,
    };
    use futures_util::{SinkExt, StreamExt};
    use serde_json::{json, Value};
    use shacs_channels::WebSocketServerEvent;
    use shacs_providers::types::text_response;
    use shacs_providers::LlmResponse;
    use std::collections::VecDeque;
    use std::error::Error;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;
    use tokio::net::{TcpListener, TcpStream};
    use tokio::sync::oneshot;
    use tokio::task::JoinHandle;
    use tokio_tungstenite::tungstenite::Message;
    use tokio_tungstenite::{connect_async, MaybeTlsStream, WebSocketStream};

    type TestResult = Result<(), Box<dyn Error>>;
    type Client = WebSocketStream<MaybeTlsStream<TcpStream>>;

    const PATIENCE: Duration = Duration::from_secs(20);

    /// How often a frame and a turn are made ready together. Without the rule the turn
    /// would go first about every other time.
    const TIES: usize = 16;

    /// An adapter whose turns each write their token to the chat they are for. The work of
    /// a frame that names a token under `offer` offers that turn for the frame's chat.
    #[derive(Default)]
    struct TurnAdapter {
        turns: Arc<WebSocketRuntimeTurns>,
        /// How its next runs end. A run nothing is scripted for ends as done.
        ends: Mutex<VecDeque<WebSocketRuntimeTurnEnd>>,
    }

    impl ChatCompletionAdapter for TurnAdapter {
        fn configured_model(&self) -> &str {
            "gpt-5"
        }

        fn complete_chat(
            &self,
            _invocation: ChatCompletionInvocation,
        ) -> Result<LlmResponse, ApiError> {
            Ok(text_response("unused"))
        }

        fn process_websocket_frame_streaming(
            &self,
            frame: Value,
            _client_id: &str,
            default_chat_id: &str,
            on_event: &mut dyn FnMut(WebSocketServerEvent) -> bool,
        ) -> Result<(), ApiError> {
            let chat_id = frame
                .get("chat_id")
                .and_then(Value::as_str)
                .unwrap_or(default_chat_id);
            if let Some(token) = frame.get("offer").and_then(Value::as_str) {
                self.turns.offer(&turn(chat_id, token));
            }
            on_event(WebSocketServerEvent::Attached {
                chat_id: chat_id.to_owned(),
            });
            Ok(())
        }

        fn websocket_runtime_turns(&self) -> Option<Arc<WebSocketRuntimeTurns>> {
            Some(self.turns.clone())
        }

        fn process_websocket_runtime_turn_streaming(
            &self,
            turn: &WebSocketRuntimeTurn,
            on_event: &mut dyn FnMut(WebSocketServerEvent) -> bool,
        ) -> Result<WebSocketRuntimeTurnEnd, ApiError> {
            on_event(said(&turn.chat_id, &turn.token));
            Ok(self
                .ends
                .lock()
                .expect("test lock poisoned")
                .pop_front()
                .unwrap_or(WebSocketRuntimeTurnEnd::Done))
        }
    }

    fn said(chat_id: &str, text: &str) -> WebSocketServerEvent {
        WebSocketServerEvent::Message {
            chat_id: chat_id.to_owned(),
            text: text.to_owned(),
            buttons: Vec::new(),
            button_prompt: None,
            media: Vec::new(),
            reply_to: None,
            kind: None,
        }
    }

    /// A WebSocket server over the adapter, and how to stop it.
    struct Served {
        url: String,
        stop: oneshot::Sender<()>,
        server: JoinHandle<std::io::Result<()>>,
    }

    impl Served {
        async fn start(adapter: Arc<TurnAdapter>) -> Result<Self, Box<dyn Error>> {
            let listener = TcpListener::bind("127.0.0.1:0").await?;
            let url = format!("ws://{}{WEBSOCKET_PATH}", listener.local_addr()?);
            let (stop, stopped) = oneshot::channel::<()>();
            let server = tokio::spawn(serve_websocket_listener_with_timeout_and_path(
                listener,
                adapter,
                Duration::from_secs(30),
                WEBSOCKET_PATH,
                async {
                    let _ = stopped.await;
                },
            ));
            Ok(Self { url, stop, server })
        }

        /// A connection that wrote an event of `chat_id`.
        async fn attached(&self, chat_id: &str) -> Result<Client, Box<dyn Error>> {
            let (mut client, _) = connect_async(self.url.as_str()).await?;
            client
                .send(Message::Text(
                    json!({"type":"attach","chat_id":chat_id})
                        .to_string()
                        .into(),
                ))
                .await?;
            assert_eq!(
                heard(&mut client).await?,
                WebSocketServerEvent::Attached {
                    chat_id: chat_id.to_owned()
                }
            );
            Ok(client)
        }

        async fn stop(self) -> TestResult {
            let _ = self.stop.send(());
            self.server.await??;
            Ok(())
        }
    }

    /// The event in a frame the connection was sent. A frame of the channel's own state
    /// has none.
    fn event(frame: Message) -> Result<Option<WebSocketServerEvent>, Box<dyn Error>> {
        let frame: Value = serde_json::from_str(&frame.into_text()?)?;
        if frame.get("event").is_none() {
            return Ok(None);
        }
        Ok(Some(serde_json::from_value(frame)?))
    }

    /// The next event the connection was sent.
    async fn heard(client: &mut Client) -> Result<WebSocketServerEvent, Box<dyn Error>> {
        tokio::time::timeout(PATIENCE, async {
            loop {
                let frame = client.next().await.ok_or("the connection closed")??;
                if let Some(event) = event(frame)? {
                    return Ok(event);
                }
            }
        })
        .await?
    }

    /// Offers `turn` again and again until the connection is sent an event, and gives
    /// that event.
    async fn heard_while_offered(
        turns: &WebSocketRuntimeTurns,
        turn: &WebSocketRuntimeTurn,
        client: &mut Client,
    ) -> Result<WebSocketServerEvent, Box<dyn Error>> {
        tokio::time::timeout(PATIENCE, async {
            loop {
                turns.offer(turn);
                let next = tokio::time::timeout(Duration::from_millis(20), client.next()).await;
                let Ok(frame) = next else {
                    continue;
                };
                if let Some(event) = event(frame.ok_or("the connection closed")??)? {
                    return Ok(event);
                }
            }
        })
        .await?
    }

    /// Closes the connection and waits until the server let go of it.
    async fn closed(mut client: Client) -> TestResult {
        client.close(None).await?;
        tokio::time::timeout(PATIENCE, async {
            while let Some(Ok(_)) = client.next().await {}
        })
        .await?;
        Ok(())
    }

    #[tokio::test]
    async fn offered_turn_runs_on_its_connection_without_a_client_frame() -> TestResult {
        // Given: a connection that wrote to "room", and a turn whose first run cannot start.
        let adapter = Arc::new(TurnAdapter::default());
        let turns = adapter.turns.clone();
        let served = Served::start(adapter.clone()).await?;
        let mut client = served.attached("room").await?;
        adapter
            .ends
            .lock()
            .expect("test lock poisoned")
            .push_back(WebSocketRuntimeTurnEnd::Retry);
        let offered = turn("room", "a");

        // When: the turn is offered, and offered again until it ran once more.
        turns.offer(&offered);
        assert_eq!(heard(&mut client).await?, said("room", "a"));
        let again = heard_while_offered(&turns, &offered, &mut client).await?;

        // Then: the client sent no frame for either run.
        assert_eq!(again, said("room", "a"));

        // When: the turn that ended as done is offered again, and then another turn.
        turns.offer(&offered);
        turns.offer(&turn("room", "b"));

        // Then: only the other turn runs.
        assert_eq!(heard(&mut client).await?, said("room", "b"));
        closed(client).await?;
        served.stop().await
    }

    #[tokio::test]
    async fn frame_the_client_sent_goes_before_a_turn_handed_meanwhile() -> TestResult {
        // Given: a connection that wrote to "room".
        let adapter = Arc::new(TurnAdapter::default());
        let turns = adapter.turns.clone();
        let served = Served::start(adapter).await?;
        let mut client = served.attached("room").await?;
        let attached = WebSocketServerEvent::Attached {
            chat_id: "room".to_owned(),
        };

        for tie in 0..TIES {
            // When: two frames reach the server in one write, and the work of the first
            // hands the connection a turn. The second frame and the turn are both ready
            // when that work ends.
            let offered = turn("room", &format!("turn-{tie}"));
            for frame in [
                json!({"type":"attach","chat_id":"room","offer":offered.token}),
                json!({"type":"attach","chat_id":"room"}),
            ] {
                client.feed(Message::Text(frame.to_string().into())).await?;
            }
            client.flush().await?;

            // Then: the second frame is worked on before the turn, which runs once it is
            // offered again.
            assert_eq!(heard(&mut client).await?, attached, "tie {tie}");
            assert_eq!(heard(&mut client).await?, attached, "tie {tie}");
            let ran = heard_while_offered(&turns, &offered, &mut client).await?;
            assert_eq!(ran, said("room", &offered.token), "tie {tie}");
        }
        closed(client).await?;
        served.stop().await
    }

    #[tokio::test]
    async fn turn_waits_until_a_connection_serves_its_chat() -> TestResult {
        // Given: the connection of "room" closed, and one that serves another chat is open.
        let adapter = Arc::new(TurnAdapter::default());
        let turns = adapter.turns.clone();
        let served = Served::start(adapter).await?;
        closed(served.attached("room").await?).await?;
        let mut elsewhere = served.attached("elsewhere").await?;
        let offered = turn("room", "a");

        // When: the turn is offered while no connection serves its chat.
        turns.offer(&offered);
        turns.offer(&turn("elsewhere", "b"));

        // Then: it ran nowhere.
        assert_eq!(heard(&mut elsewhere).await?, said("elsewhere", "b"));

        // When: a new connection writes to the chat and the turn is offered again.
        let mut client = served.attached("room").await?;
        turns.offer(&offered);

        // Then
        assert_eq!(heard(&mut client).await?, said("room", "a"));
        closed(client).await?;
        closed(elsewhere).await?;
        served.stop().await
    }

    fn turn(chat_id: &str, token: &str) -> WebSocketRuntimeTurn {
        WebSocketRuntimeTurn {
            chat_id: chat_id.to_owned(),
            token: token.to_owned(),
        }
    }

    /// The turns the seat was handed and did not take out yet.
    fn handed(seat: &mut WebSocketRuntimeTurnSeat) -> Vec<String> {
        let mut tokens = Vec::new();
        while let Ok(turn) = seat.offered.try_recv() {
            tokens.push(turn.token);
        }
        tokens
    }

    #[test]
    fn turn_goes_to_the_connection_that_last_wrote_its_chat() {
        // Given: two connections of the default chat; the first wrote to "room" and then
        // the second did.
        let turns = Arc::new(WebSocketRuntimeTurns::default());
        let mut first = turns.attach("default");
        let mut second = turns.attach("default");
        first.wrote("room");
        second.wrote("room");

        // When
        turns.offer(&turn("room", "a"));
        turns.offer(&turn("nowhere", "b"));
        first.wrote("default");
        turns.offer(&turn("default", "c"));

        // Then: a chat no connection serves is handed to no one.
        assert_eq!(handed(&mut first), ["c"]);
        assert_eq!(handed(&mut second), ["a"]);
    }

    #[test]
    fn turn_is_handed_out_once_until_its_run_ends() {
        // Given
        let turns = Arc::new(WebSocketRuntimeTurns::default());
        let mut seat = turns.attach("default");
        let offered = turn("default", "a");

        // When: the turn is offered while a connection has it.
        turns.offer(&offered);
        turns.offer(&offered);

        // Then
        assert_eq!(handed(&mut seat), ["a"]);

        // When: the run says the turn could not start.
        let mut run = seat.run(offered.clone());
        turns.offer(&offered);
        run.ended(WebSocketRuntimeTurnEnd::Retry);
        drop(run);
        turns.offer(&offered);

        // Then: the next offer hands it out again.
        assert_eq!(handed(&mut seat), ["a"]);

        // When: the connection gives the turn back instead of running it.
        seat.give_back(offered.clone());
        turns.offer(&offered);

        // Then
        assert_eq!(handed(&mut seat), ["a"]);

        // When: a run of it ends as done.
        drop(seat.run(offered.clone()));
        turns.offer(&offered);

        // Then
        assert_eq!(handed(&mut seat), [] as [&str; 0]);
    }

    #[test]
    fn turn_of_a_closed_connection_goes_to_another() {
        // Given: a connection that was handed a turn and went away with it.
        let turns = Arc::new(WebSocketRuntimeTurns::default());
        let gone = turns.attach("default");
        let offered = turn("default", "a");
        turns.offer(&offered);
        drop(gone);

        // When: it is offered with no connection, and again once one opened.
        turns.offer(&offered);
        let mut next = turns.attach("default");
        turns.offer(&offered);

        // Then
        assert_eq!(handed(&mut next), ["a"]);
    }

    #[test]
    fn run_that_panics_ends_its_turn_as_done() {
        // Given
        let turns = Arc::new(WebSocketRuntimeTurns::default());
        let mut seat = turns.attach("default");
        let offered = turn("default", "a");
        turns.offer(&offered);
        let run = seat.run(offered.clone());

        // When
        let work = std::thread::spawn(move || {
            let _run = run;
            panic!("the work of the turn panics");
        });
        assert!(work.join().is_err());
        assert_eq!(handed(&mut seat), ["a"]);
        turns.offer(&offered);

        // Then
        assert_eq!(handed(&mut seat), [] as [&str; 0]);
    }

    #[test]
    fn remembered_chats_and_closed_turns_are_bounded() {
        // Given: a connection that wrote to more chats than it is remembered to serve.
        let turns = Arc::new(WebSocketRuntimeTurns::default());
        let mut seat = turns.attach("default");
        for index in 0..SERVED_CHATS {
            seat.wrote(&format!("room-{index}"));
        }

        // When
        turns.offer(&turn("default", "forgotten"));
        turns.offer(&turn("room-0", "kept"));

        // Then: the chat written longest ago was forgotten.
        assert_eq!(handed(&mut seat), ["kept"]);

        // When: more turns ended as done than are remembered.
        for index in 0..=CLOSED_TURNS {
            drop(seat.run(turn("room-0", &format!("done-{index}"))));
        }
        turns.offer(&turn("room-0", "done-0"));
        turns.offer(&turn("room-0", &format!("done-{CLOSED_TURNS}")));

        // Then: only the oldest of them can be handed out again.
        assert_eq!(handed(&mut seat), ["done-0"]);
    }
}
