//! Spec 050 PRD 010: a `handoff` that paused in a Telegram topic or a Slack thread goes on
//! there, not in the parent chat, whether another room returned the browser or its limit
//! passed.

use super::super::TestResult;
use super::{assistant, Desk, Model, LIMIT_MS, QUESTION, RETURNED, ROOM_A, ROOM_B};
use crate::{
    slack_outbound_thread_ts, slack_socket_envelope_to_inbound, telegram_message_body,
    SlackRuntimeConfig,
};
use serde_json::{json, Value};
use shacs_channels::OutboundMessage;
use std::error::Error;

const TOPIC: u64 = 7;
const SLACK_ROOM: &str = "C0123";
const THREAD: &str = "1700000000.000100";

/// The `message_thread_id` Telegram is given with `message`.
fn topic(message: &OutboundMessage) -> Value {
    telegram_message_body(message, &message.content, None, false)["message_thread_id"].clone()
}

/// What goes on in `room` once another room returned the browser, and then once the limit
/// of a second handoff passed. `say` is how `room` asks its assistant.
fn continued(
    desk: &Desk,
    room: &str,
    say: impl Fn(&str),
) -> Result<[OutboundMessage; 2], Box<dyn Error>> {
    say("넘겨줘");
    desk.hear(room, QUESTION)?;
    let first = desk.asked()?;
    desk.say(ROOM_B, &format!("/browser return {}", first.label));
    desk.hear(ROOM_B, RETURNED)?;
    desk.hear(room, "이어서: browser_returned")?;
    let returned = desk.outbound("이어서: browser_returned")?;

    say("또 넘겨줘");
    desk.hear(room, QUESTION)?;
    desk.asked()?;
    desk.stage.advance(LIMIT_MS);
    desk.hear(room, "이어서: browser_takeover_timeout")?;
    let timed_out = desk.outbound("이어서: browser_takeover_timeout")?;
    Ok([returned, timed_out])
}

#[test]
fn telegram_topic_handoff_continues_in_its_topic() -> TestResult {
    // Given: a Telegram chat whose assistant is asked inside a topic.
    let desk = Desk::start(Model::new(assistant))?;
    let say = |text: &str| {
        let mut update = desk.update(ROOM_A, text, None);
        update["message"]["message_thread_id"] = json!(TOPIC);
        desk.arrive(&desk.bus, &update);
    };

    // When: its handoffs end by a return from another room and by the limit.
    let [returned, timed_out] = continued(&desk, ROOM_A, say)?;

    // Then: both continuations are sent to the topic.
    assert_eq!(topic(&returned), json!(TOPIC.to_string()), "{returned:#?}");
    assert_eq!(
        topic(&timed_out),
        json!(TOPIC.to_string()),
        "{timed_out:#?}"
    );
    Ok(())
}

#[test]
fn slack_thread_handoff_continues_in_its_thread() -> TestResult {
    // Given: a Slack channel whose assistant is asked inside a thread.
    let desk = Desk::start(Model::new(assistant))?;
    let slack = SlackRuntimeConfig {
        app_token: "slack-app-token".to_owned(),
        bot_token: "slack-bot-token".to_owned(),
        channel_ids: Vec::new(),
        allowed_senders: Vec::new(),
    };
    let say = |text: &str| {
        let envelope = json!({"payload":{"event":{"type":"message","user":"U0123",
            "channel":SLACK_ROOM,"text":text,"ts":"1700000000.000200","thread_ts":THREAD,
            "channel_type":"channel"}}});
        let inbound =
            slack_socket_envelope_to_inbound(&slack, &envelope).expect("a message with text");
        desk.take_in(&desk.bus, inbound);
    };

    // When: its handoffs end by a return from another room and by the limit.
    let [returned, timed_out] = continued(&desk, SLACK_ROOM, say)?;

    // Then: both continuations are sent to the thread.
    assert_eq!(
        slack_outbound_thread_ts(&returned).as_deref(),
        Some(THREAD),
        "{returned:#?}"
    );
    assert_eq!(
        slack_outbound_thread_ts(&timed_out).as_deref(),
        Some(THREAD),
        "{timed_out:#?}"
    );
    Ok(())
}
