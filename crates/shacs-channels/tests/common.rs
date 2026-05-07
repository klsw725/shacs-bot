use serde_json::{json, Map};
use shacs_channels::{
    builtin_channel_default_configs, builtin_channel_descriptors, discord_thread_session_key,
    email_session_key, normalize_websocket_frame, normalize_whatsapp_bridge_message,
    route_channel_command, slack_thread_session_key, telegram_topic_session_key,
    websocket_event_from_outbound, whatsapp_auth_frame, whatsapp_outbound_frames, ChannelAdapter,
    ChannelAllowlist, ChannelCommandAction, ChannelCommandRequest, ChannelDescriptor, ChannelError,
    ChannelManager, ChannelRegistry, ChannelRetryPolicy, DiscordInbound, EmailInbound,
    InboundMessage, LiveChannelWorkerKind, OutboundMessage, RecentMessageIds, SlackInbound,
    TelegramInbound, WebSocketInboundAction, WebSocketServerEvent, WhatsAppBridgeMessage,
    WhatsAppChannelConfig, WhatsAppGroupPolicy, WhatsAppOutboundFrame, DISCORD_CHANNEL,
    EMAIL_CHANNEL, SLACK_CHANNEL, TELEGRAM_CHANNEL, WEBSOCKET_CHANNEL, WHATSAPP_CHANNEL,
};
use std::cell::RefCell;
use std::error::Error;
use std::rc::Rc;

type RecordedDeltas = Rc<RefCell<Vec<(String, String, Map<String, serde_json::Value>)>>>;

#[test]
fn message_shapes_preserve_session_key_and_defaults() {
    let inbound = InboundMessage::new("telegram", "user-1", "chat-1", "hello");
    assert_eq!(inbound.session_key(), "telegram:chat-1");
    assert!(inbound.media.is_empty());
    assert!(inbound.metadata.is_empty());

    let overridden = inbound.clone().with_session_key_override("shared");
    assert_eq!(overridden.session_key(), "shared");

    let outbound = OutboundMessage::new("telegram", "chat-1", "hi").with_reply_to("msg-1");
    assert_eq!(outbound.reply_to.as_deref(), Some("msg-1"));
    assert!(outbound.buttons.is_empty());
}

#[test]
fn allowlist_denies_empty_allows_star_and_exact_sender() {
    assert!(!ChannelAllowlist::deny_all().is_allowed("user-1"));
    assert!(ChannelAllowlist::allow_all().is_allowed("user-1"));
    assert!(ChannelAllowlist::new(["user-1".to_owned()]).is_allowed("user-1"));
    assert!(!ChannelAllowlist::new(["user-2".to_owned()]).is_allowed("user-1"));
}

#[test]
fn telegram_style_command_forwarding_strips_suffix_and_preserves_args() -> Result<(), Box<dyn Error>>
{
    let action = route_channel_command(
        ChannelCommandRequest::new("telegram", "user-1", "chat-1", "/status@MyBot now")
            .with_bot_name("mybot"),
    );
    let ChannelCommandAction::Forward(inbound) = action else {
        return Err(format!("status command should forward: {action:?}").into());
    };
    assert_eq!(inbound.content, "/status now");
    assert_eq!(inbound.session_key(), "telegram:chat-1");

    let alias = route_channel_command(ChannelCommandRequest::new(
        "telegram",
        "user-1",
        "chat-1",
        "/dream_log AbC123",
    ));
    let ChannelCommandAction::Forward(inbound) = alias else {
        return Err(format!("dream alias should forward: {alias:?}").into());
    };
    assert_eq!(inbound.content, "/dream-log AbC123");
    Ok(())
}

#[test]
fn help_is_direct_response_and_metadata_is_preserved_for_forwarded_commands(
) -> Result<(), Box<dyn Error>> {
    let help = route_channel_command(ChannelCommandRequest::new(
        "discord", "user-1", "chat-1", "/help",
    ));
    let ChannelCommandAction::DirectHelp(outbound) = help else {
        return Err(format!("help should be direct response: {help:?}").into());
    };
    assert_eq!(outbound.channel, "discord");
    assert!(outbound.content.contains("/status"));

    let mut metadata = Map::new();
    metadata.insert("is_slash_command".to_owned(), json!(true));
    let forward = route_channel_command(
        ChannelCommandRequest::new("discord", "user-1", "chat-1", "/history 5")
            .with_metadata(metadata),
    );
    let ChannelCommandAction::Forward(inbound) = forward else {
        return Err(format!("history should forward: {forward:?}").into());
    };
    assert_eq!(inbound.content, "/history 5");
    assert_eq!(inbound.metadata["is_slash_command"], true);
    Ok(())
}

#[test]
fn forwarded_commands_preserve_thread_session_override() -> Result<(), Box<dyn Error>> {
    let action = route_channel_command(
        ChannelCommandRequest::new("discord", "user-1", "thread-1", "/status")
            .with_session_key_override("discord:parent/thread-1"),
    );
    let ChannelCommandAction::Forward(inbound) = action else {
        return Err(format!("thread command should forward: {action:?}").into());
    };
    assert_eq!(inbound.content, "/status");
    assert_eq!(
        inbound.session_key_override.as_deref(),
        Some("discord:parent/thread-1")
    );
    assert_eq!(inbound.session_key(), "discord:parent/thread-1");
    Ok(())
}

#[test]
fn builtin_registry_exposes_selected_channels_and_rejects_unknowns() -> Result<(), Box<dyn Error>> {
    let registry = ChannelRegistry::with_builtin_channels();
    assert!(registry.names().contains(&WEBSOCKET_CHANNEL));
    assert!(registry.names().contains(&DISCORD_CHANNEL));
    assert!(registry.names().contains(&TELEGRAM_CHANNEL));
    assert!(registry.names().contains(&EMAIL_CHANNEL));
    assert!(registry.names().contains(&SLACK_CHANNEL));
    assert!(registry.names().contains(&WHATSAPP_CHANNEL));
    assert!(registry.require(WEBSOCKET_CHANNEL)?.capabilities.streaming);

    let selected =
        registry.selected(&[WEBSOCKET_CHANNEL.to_owned(), WHATSAPP_CHANNEL.to_owned()])?;
    assert_eq!(selected.len(), 2);

    let mut custom = ChannelRegistry::new();
    custom.register(ChannelDescriptor::new("custom", "Custom"))?;
    assert!(matches!(
        custom.register(ChannelDescriptor::new("custom", "Again")),
        Err(ChannelError::DuplicateChannel(name)) if name == "custom"
    ));
    assert!(matches!(
        registry.require("matrix"),
        Err(ChannelError::UnknownChannel(name)) if name == "matrix"
    ));
    Ok(())
}

#[test]
fn builtin_channel_default_configs_cover_all_builtin_channels() {
    let defaults = builtin_channel_default_configs();
    for descriptor in builtin_channel_descriptors() {
        assert!(
            defaults.contains_key(&descriptor.name),
            "missing default config for {}",
            descriptor.name
        );
    }
    assert_eq!(defaults[WEBSOCKET_CHANNEL]["enabled"], json!(true));
    assert_eq!(defaults[TELEGRAM_CHANNEL]["enabled"], json!(false));
    assert_eq!(defaults[EMAIL_CHANNEL]["consentGranted"], json!(false));
    assert_eq!(defaults[EMAIL_CHANNEL]["imap"]["security"], json!("tls"));
    assert_eq!(
        defaults[WHATSAPP_CHANNEL]["allowlist"]["allowedSenders"],
        json!([])
    );
}

#[test]
fn builtin_live_worker_descriptors_mark_websocket_ready_and_external_workers_gated() {
    let workers = shacs_channels::builtin_live_worker_descriptors();
    assert!(workers.iter().any(|worker| {
        worker.channel == WEBSOCKET_CHANNEL
            && worker.kind == LiveChannelWorkerKind::WebSocketServer
            && !worker.requires_external_credentials
            && worker.ready_for_runtime
    }));
    assert!(workers.iter().any(|worker| {
        worker.channel == TELEGRAM_CHANNEL
            && worker.kind == LiveChannelWorkerKind::TelegramLongPolling
            && worker.requires_external_credentials
            && worker.ready_for_runtime
    }));
    assert!(workers.iter().any(|worker| {
        worker.channel == EMAIL_CHANNEL && worker.kind == LiveChannelWorkerKind::EmailSmtp
    }));
    assert!(workers.iter().any(|worker| {
        worker.channel == EMAIL_CHANNEL && worker.kind == LiveChannelWorkerKind::EmailImap
    }));
}

#[test]
fn manager_tracks_lifecycle_retries_and_stream_delta_dispatch() -> Result<(), Box<dyn Error>> {
    let sends = Rc::new(RefCell::new(Vec::new()));
    let deltas = Rc::new(RefCell::new(Vec::new()));
    let starts = Rc::new(RefCell::new(0));
    let stops = Rc::new(RefCell::new(0));
    let fail_once = Rc::new(RefCell::new(true));
    let adapter = RecordingAdapter {
        name: WEBSOCKET_CHANNEL.to_owned(),
        sends: sends.clone(),
        deltas: deltas.clone(),
        starts: starts.clone(),
        stops: stops.clone(),
        fail_once: fail_once.clone(),
    };
    let mut manager =
        ChannelManager::new().with_retry_policy(ChannelRetryPolicy { max_attempts: 2 });
    manager.register_adapter(Box::new(adapter), true)?;

    manager.start_all()?;
    assert_eq!(*starts.borrow(), 1);
    assert!(manager
        .status(WEBSOCKET_CHANNEL)
        .is_some_and(|status| status.running));

    manager.dispatch_outbound(OutboundMessage::new(WEBSOCKET_CHANNEL, "chat", "hello"))?;
    assert_eq!(sends.borrow().len(), 1);
    assert!(!*fail_once.borrow());

    let mut metadata = Map::new();
    metadata.insert("_stream_delta".to_owned(), json!(true));
    metadata.insert("_stream_id".to_owned(), json!("s1"));
    manager.dispatch_outbound(
        OutboundMessage::new(WEBSOCKET_CHANNEL, "chat", "chunk").with_metadata(metadata),
    )?;
    assert_eq!(deltas.borrow()[0].0, "chat");
    assert_eq!(deltas.borrow()[0].1, "chunk");
    assert_eq!(deltas.borrow()[0].2["stream_id"], "s1");

    manager.stop_all()?;
    assert_eq!(*stops.borrow(), 1);
    assert!(manager
        .status(WEBSOCKET_CHANNEL)
        .is_some_and(|status| !status.running));
    Ok(())
}

#[test]
fn manager_records_dispatch_error_and_clears_after_success() -> Result<(), Box<dyn Error>> {
    let sends = Rc::new(RefCell::new(Vec::new()));
    let deltas = Rc::new(RefCell::new(Vec::new()));
    let starts = Rc::new(RefCell::new(0));
    let stops = Rc::new(RefCell::new(0));
    let fail_once = Rc::new(RefCell::new(true));
    let adapter = RecordingAdapter {
        name: TELEGRAM_CHANNEL.to_owned(),
        sends: sends.clone(),
        deltas,
        starts,
        stops,
        fail_once: fail_once.clone(),
    };
    let mut manager =
        ChannelManager::new().with_retry_policy(ChannelRetryPolicy { max_attempts: 1 });
    manager.register_adapter(Box::new(adapter), true)?;

    let error = manager
        .dispatch_outbound(OutboundMessage::new(TELEGRAM_CHANNEL, "chat", "first"))
        .expect_err("first dispatch should fail");
    assert!(error.to_string().contains("transient"));
    let status = manager.status(TELEGRAM_CHANNEL).ok_or("missing status")?;
    assert!(status
        .last_error
        .as_deref()
        .is_some_and(|error| error.contains("transient")));

    manager.dispatch_outbound(OutboundMessage::new(TELEGRAM_CHANNEL, "chat", "second"))?;
    let status = manager.status(TELEGRAM_CHANNEL).ok_or("missing status")?;
    assert!(status.last_error.is_none());
    assert_eq!(sends.borrow().len(), 1);
    Ok(())
}

#[test]
fn websocket_frames_preserve_legacy_envelope_media_and_streaming_shapes(
) -> Result<(), Box<dyn Error>> {
    let legacy = normalize_websocket_frame(json!("hello"), "client-1", "chat-a")?;
    let WebSocketInboundAction::Message(inbound) = legacy else {
        return Err("legacy text should become inbound message".into());
    };
    assert_eq!(inbound.channel, WEBSOCKET_CHANNEL);
    assert_eq!(inbound.sender_id, "client-1");
    assert_eq!(inbound.chat_id, "chat-a");
    assert_eq!(inbound.content, "hello");

    let attach = normalize_websocket_frame(
        json!({ "type": "attach", "chat_id": "chat-b" }),
        "client-1",
        "chat-a",
    )?;
    assert_eq!(
        attach,
        WebSocketInboundAction::Attach {
            chat_id: "chat-b".to_owned()
        }
    );

    let message = normalize_websocket_frame(
        json!({
            "type": "message",
            "chat_id": "chat-b",
            "text": "with image",
            "media": [{ "data_url": "data:image/png;base64,AA==", "name": "a.png" }]
        }),
        "client-1",
        "chat-a",
    )?;
    let WebSocketInboundAction::Message(inbound) = message else {
        return Err("message envelope should become inbound message".into());
    };
    assert_eq!(inbound.media, vec!["data:image/png;base64,AA=="]);
    assert_eq!(inbound.metadata["media_names"], json!(["a.png"]));

    let malformed = normalize_websocket_frame(
        json!({
            "type": "message",
            "text": "bad media",
            "media": [{ "name": "missing-data-url.png" }]
        }),
        "client-1",
        "chat-a",
    )
    .expect_err("media item without data_url should fail");
    assert!(malformed.to_string().contains("media item needs data_url"));

    let malformed = normalize_websocket_frame(
        json!({
            "type": "message",
            "text": "bad media",
            "media": [false]
        }),
        "client-1",
        "chat-a",
    )
    .expect_err("non-string media item should fail");
    assert!(malformed
        .to_string()
        .contains("media item must be a string or object"));

    let mut metadata = Map::new();
    metadata.insert("_stream_delta".to_owned(), json!(true));
    metadata.insert("_stream_id".to_owned(), json!("stream-1"));
    let event = websocket_event_from_outbound(
        OutboundMessage::new(WEBSOCKET_CHANNEL, "chat-b", "part").with_metadata(metadata),
    );
    assert_eq!(
        event,
        WebSocketServerEvent::Delta {
            chat_id: "chat-b".to_owned(),
            text: "part".to_owned(),
            stream_id: Some("stream-1".to_owned())
        }
    );
    Ok(())
}

#[test]
fn platform_normalizers_preserve_session_metadata_and_content_contracts() {
    let discord = DiscordInbound {
        sender_id: "user".to_owned(),
        channel_id: "parent".to_owned(),
        content: "hello".to_owned(),
        message_id: Some("msg".to_owned()),
        guild_id: Some("guild".to_owned()),
        parent_channel_id: Some("parent".to_owned()),
        thread_id: Some("thread".to_owned()),
        attachments: vec!["/tmp/a.png".to_owned()],
    }
    .into_message();
    assert_eq!(
        discord.session_key(),
        discord_thread_session_key("parent", "thread")
    );
    assert_eq!(discord.metadata["guild_id"], "guild");

    let telegram = TelegramInbound {
        sender_id: "user".to_owned(),
        chat_id: "chat".to_owned(),
        content: "hello".to_owned(),
        message_id: Some("msg".to_owned()),
        username: Some("alice".to_owned()),
        message_thread_id: Some("topic".to_owned()),
        media: Vec::new(),
    }
    .into_message();
    assert_eq!(
        telegram.session_key(),
        telegram_topic_session_key("chat", "topic")
    );
    assert_eq!(telegram.metadata["username"], "alice");

    let email = EmailInbound {
        sender_email: "me@example.com".to_owned(),
        subject: "Subj".to_owned(),
        date: "Tue".to_owned(),
        body: "Body".to_owned(),
        message_id: "mid".to_owned(),
        uid: Some("42".to_owned()),
        attachments: Vec::new(),
    }
    .into_message();
    assert_eq!(email.chat_id, "me@example.com");
    assert_eq!(email.session_key(), email_session_key("me@example.com"));
    assert!(email.content.starts_with("[EMAIL-CONTEXT] Email received."));
    assert_eq!(email.metadata["message_id"], "mid");
    assert_eq!(email.metadata["sender_email"], "me@example.com");

    let slack = SlackInbound {
        user_id: "U1".to_owned(),
        channel_id: "C1".to_owned(),
        content: "hello".to_owned(),
        event_ts: Some("1.0".to_owned()),
        thread_ts: Some("0.9".to_owned()),
        channel_type: Some("im".to_owned()),
        files: vec!["/tmp/f.txt".to_owned()],
    }
    .into_message();
    assert_eq!(slack.session_key(), slack_thread_session_key("C1", "0.9"));
    assert_eq!(slack.metadata["slack"]["channel_type"], "im");
    assert_eq!(slack.metadata["slack"]["event"]["channel"], "C1");
    assert_eq!(slack.metadata["slack"]["event"]["user"], "U1");
    assert_eq!(slack.metadata["slack"]["event"]["ts"], "1.0");
}

#[test]
fn whatsapp_bridge_normalizes_auth_dedupe_group_policy_media_and_outbound_frames(
) -> Result<(), Box<dyn Error>> {
    let config = WhatsAppChannelConfig {
        bridge_url: "ws://127.0.0.1:7788".to_owned(),
        bridge_token: Some("secret".to_owned()),
        allowlist: ChannelAllowlist::new(["+8210".to_owned()]),
        group_policy: WhatsAppGroupPolicy::Mention,
    };
    let mut recent = RecentMessageIds::default();

    let ignored = normalize_whatsapp_bridge_message(
        WhatsAppBridgeMessage {
            kind: "message".to_owned(),
            pn: Some("+8210".to_owned()),
            sender: Some("120@g.us".to_owned()),
            content: Some("hi".to_owned()),
            id: Some("m0".to_owned()),
            is_group: true,
            was_mentioned: false,
            media: Vec::new(),
            timestamp: None,
        },
        &config,
        &mut recent,
    )?;
    assert!(ignored.is_none());

    let inbound = normalize_whatsapp_bridge_message(
        WhatsAppBridgeMessage {
            kind: "message".to_owned(),
            pn: Some("+8210".to_owned()),
            sender: Some("120@g.us".to_owned()),
            content: Some("hi".to_owned()),
            id: Some("m1".to_owned()),
            is_group: true,
            was_mentioned: true,
            media: vec!["/tmp/photo.jpg".to_owned(), "/tmp/doc.pdf".to_owned()],
            timestamp: Some("2026-05-05T00:00:00Z".to_owned()),
        },
        &config,
        &mut recent,
    )?
    .ok_or("mentioned group message should pass")?;
    assert_eq!(inbound.channel, WHATSAPP_CHANNEL);
    assert_eq!(inbound.sender_id, "+8210");
    assert_eq!(inbound.chat_id, "120@g.us");
    assert!(inbound.content.contains("[image: /tmp/photo.jpg]"));
    assert!(inbound.content.contains("[file: /tmp/doc.pdf]"));
    assert_eq!(inbound.metadata["phone"], "+8210");

    let duplicate = normalize_whatsapp_bridge_message(
        WhatsAppBridgeMessage {
            kind: "message".to_owned(),
            pn: Some("+8210".to_owned()),
            sender: Some("120@g.us".to_owned()),
            content: Some("again".to_owned()),
            id: Some("m1".to_owned()),
            is_group: false,
            was_mentioned: false,
            media: Vec::new(),
            timestamp: None,
        },
        &config,
        &mut recent,
    )?;
    assert!(duplicate.is_none());

    let direct_jid = normalize_whatsapp_bridge_message(
        WhatsAppBridgeMessage {
            kind: "message".to_owned(),
            pn: None,
            sender: Some("8210@s.whatsapp.net".to_owned()),
            content: Some("direct".to_owned()),
            id: Some("m2".to_owned()),
            is_group: false,
            was_mentioned: false,
            media: Vec::new(),
            timestamp: None,
        },
        &config,
        &mut recent,
    )?
    .ok_or("direct JID should normalize against phone allowlist")?;
    assert_eq!(direct_jid.sender_id, "8210");
    assert_eq!(direct_jid.chat_id, "8210@s.whatsapp.net");

    let lid_first = normalize_whatsapp_bridge_message(
        WhatsAppBridgeMessage {
            kind: "message".to_owned(),
            pn: Some("+8210".to_owned()),
            sender: Some("abc123@lid.whatsapp.net".to_owned()),
            content: Some("lid first".to_owned()),
            id: Some("m3".to_owned()),
            is_group: false,
            was_mentioned: false,
            media: Vec::new(),
            timestamp: None,
        },
        &config,
        &mut recent,
    )?
    .ok_or("first LID message should seed mapping")?;
    assert_eq!(lid_first.sender_id, "+8210");

    let lid_second = normalize_whatsapp_bridge_message(
        WhatsAppBridgeMessage {
            kind: "message".to_owned(),
            pn: None,
            sender: Some("abc123@lid.whatsapp.net".to_owned()),
            content: Some("lid second".to_owned()),
            id: Some("m4".to_owned()),
            is_group: false,
            was_mentioned: false,
            media: Vec::new(),
            timestamp: None,
        },
        &config,
        &mut recent,
    )?
    .ok_or("known LID should resolve to cached phone")?;
    assert_eq!(lid_second.sender_id, "+8210");
    assert_eq!(lid_second.chat_id, "abc123@lid.whatsapp.net");

    assert_eq!(
        whatsapp_auth_frame("secret"),
        WhatsAppOutboundFrame::Auth {
            token: "secret".to_owned()
        }
    );
    let frames = whatsapp_outbound_frames(
        OutboundMessage::new(WHATSAPP_CHANNEL, "120@g.us", "reply").with_metadata(Map::new()),
    );
    assert_eq!(
        frames,
        vec![WhatsAppOutboundFrame::Send {
            to: "120@g.us".to_owned(),
            text: "reply".to_owned()
        }]
    );

    let media_frames = whatsapp_outbound_frames(OutboundMessage {
        channel: WHATSAPP_CHANNEL.to_owned(),
        chat_id: "120@g.us".to_owned(),
        content: String::new(),
        reply_to: None,
        media: vec!["/tmp/photo.jpg".to_owned()],
        metadata: Map::new(),
        buttons: Vec::new(),
    });
    assert_eq!(
        media_frames,
        vec![WhatsAppOutboundFrame::SendMedia {
            to: "120@g.us".to_owned(),
            file_path: "/tmp/photo.jpg".to_owned(),
            mimetype: "image/jpeg".to_owned(),
            file_name: "photo.jpg".to_owned(),
        }]
    );
    Ok(())
}

#[test]
fn manager_lifecycle_continues_after_adapter_errors() -> Result<(), Box<dyn Error>> {
    let failing_starts = Rc::new(RefCell::new(0));
    let succeeding_starts = Rc::new(RefCell::new(0));
    let after_error_starts = Rc::new(RefCell::new(0));
    let failing_stops = Rc::new(RefCell::new(0));
    let succeeding_stops = Rc::new(RefCell::new(0));
    let after_error_stops = Rc::new(RefCell::new(0));

    let mut manager = ChannelManager::new();
    manager.register_adapter(
        Box::new(LifecycleAdapter {
            name: "discord".to_owned(),
            starts: failing_starts.clone(),
            stops: failing_stops.clone(),
            fail_start: true,
            fail_stop: false,
        }),
        true,
    )?;
    manager.register_adapter(
        Box::new(LifecycleAdapter {
            name: "telegram".to_owned(),
            starts: succeeding_starts.clone(),
            stops: succeeding_stops.clone(),
            fail_start: false,
            fail_stop: true,
        }),
        true,
    )?;
    manager.register_adapter(
        Box::new(LifecycleAdapter {
            name: "whatsapp".to_owned(),
            starts: after_error_starts.clone(),
            stops: after_error_stops.clone(),
            fail_start: false,
            fail_stop: false,
        }),
        true,
    )?;

    assert!(matches!(
        manager.start_all(),
        Err(ChannelError::Delivery(error)) if error == "start discord"
    ));
    assert_eq!(*failing_starts.borrow(), 1);
    assert_eq!(*succeeding_starts.borrow(), 1);
    assert_eq!(*after_error_starts.borrow(), 1);
    assert!(manager
        .status("telegram")
        .is_some_and(|status| status.running));
    assert!(manager
        .status("whatsapp")
        .is_some_and(|status| status.running));

    assert!(matches!(
        manager.stop_all(),
        Err(ChannelError::Delivery(error)) if error == "stop telegram"
    ));
    assert_eq!(*failing_stops.borrow(), 0);
    assert_eq!(*succeeding_stops.borrow(), 1);
    assert_eq!(*after_error_stops.borrow(), 1);
    assert!(manager
        .status("telegram")
        .is_some_and(|status| status.running));
    assert!(manager
        .status("whatsapp")
        .is_some_and(|status| !status.running));
    Ok(())
}

struct RecordingAdapter {
    name: String,
    sends: Rc<RefCell<Vec<OutboundMessage>>>,
    deltas: RecordedDeltas,
    starts: Rc<RefCell<usize>>,
    stops: Rc<RefCell<usize>>,
    fail_once: Rc<RefCell<bool>>,
}

struct LifecycleAdapter {
    name: String,
    starts: Rc<RefCell<usize>>,
    stops: Rc<RefCell<usize>>,
    fail_start: bool,
    fail_stop: bool,
}

impl ChannelAdapter for LifecycleAdapter {
    fn name(&self) -> &str {
        &self.name
    }

    fn start(&mut self) -> Result<(), ChannelError> {
        *self.starts.borrow_mut() += 1;
        if self.fail_start {
            Err(ChannelError::Delivery(format!("start {}", self.name)))
        } else {
            Ok(())
        }
    }

    fn stop(&mut self) -> Result<(), ChannelError> {
        *self.stops.borrow_mut() += 1;
        if self.fail_stop {
            Err(ChannelError::Delivery(format!("stop {}", self.name)))
        } else {
            Ok(())
        }
    }

    fn send(&self, _message: OutboundMessage) -> Result<(), ChannelError> {
        Ok(())
    }
}

impl ChannelAdapter for RecordingAdapter {
    fn name(&self) -> &str {
        &self.name
    }

    fn supports_streaming(&self) -> bool {
        true
    }

    fn start(&mut self) -> Result<(), ChannelError> {
        *self.starts.borrow_mut() += 1;
        Ok(())
    }

    fn stop(&mut self) -> Result<(), ChannelError> {
        *self.stops.borrow_mut() += 1;
        Ok(())
    }

    fn send(&self, message: OutboundMessage) -> Result<(), ChannelError> {
        if *self.fail_once.borrow() {
            *self.fail_once.borrow_mut() = false;
            return Err(ChannelError::Delivery("transient".to_owned()));
        }
        self.sends.borrow_mut().push(message);
        Ok(())
    }

    fn send_delta(
        &self,
        chat_id: &str,
        delta: &str,
        metadata: Map<String, serde_json::Value>,
    ) -> Result<(), ChannelError> {
        self.deltas
            .borrow_mut()
            .push((chat_id.to_owned(), delta.to_owned(), metadata));
        Ok(())
    }
}
