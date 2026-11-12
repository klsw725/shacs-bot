//! The browser around a session's turns (Spec 050): the reply wait the host
//! counts between the owner's turns, the `/browser` command, and the paused
//! `handoff` call a takeover leaves behind (PRD 010).

use super::{append_ask_user_resume, method, AgentLoopConfig, InboundMessage, OutboundMessage};
use crate::runtime::{
    reply_route, BrowserArrival, BrowserCloseOutcome, BrowserDescription, BrowserDescriptionState,
    BrowserError, BrowserHandoffEpisode, BrowserHost, BrowserPendingResume, BrowserReturnOutcome,
    BrowserReturnTarget, BrowserTakeoverError, BrowserTakeoverRequest, BrowserViewDescription,
    BROWSER_ARRIVAL_METADATA, BROWSER_EPISODE_NOTICE_METADATA, BROWSER_RESUME_EVENT,
    REPLY_ROUTE_METADATA_KEYS,
};
use crate::tools::{pending_paused_call, PausedCallKind};
use serde_json::{json, Value};
use shacs_command::BrowserCommandArgs;
use shacs_session::Session;
use std::sync::Arc;

const NOT_THIS_NOTICE: &str = "이 메시지는 다른 메시지에 단 답글이라 지금 넘겨받기의 답으로 쓰지 않았습니다. 끝나면 지금 넘겨받기 안내에 답글을 달거나 답글 없이 답해 주세요.";
const BEFORE_NOTICE: &str = "이 메시지는 넘겨받기 안내가 가기 전에 보낸 것이라 답으로 쓰지 않았습니다. 끝나면 다시 답해 주세요.";
const RETURN_REPLY_MISMATCH: &str = "답글로 보낸 /browser return은 답글이 가리키는 안내의 넘겨받기만 돌려줍니다. 그 넘겨받기는 지금 열려 있지 않습니다. 지금 넘겨받기를 돌려주려면 답글 없이 /browser return을 보내 주세요.";
const RETURN_NOT_ASKED: &str = "넘겨받기 안내가 전달되기 전에 보낸 /browser return이라 돌려주지 않았습니다. 다시 보내거나 /browser return <회차 표시>를 쓰세요. 회차 표시는 /browser status에 있습니다.";
/// The tool result `/stop` closes a paused `handoff` call with.
const HANDOFF_STOPPED: &str = "browser_cancelled: 실행이 중단되었습니다";

/// Whether this inbound starts a turn of the session's owner: a message the
/// user sent, or the runtime's own continuation of that user's paused
/// `handoff` call. Other runs the runtime starts (system, heartbeat, cron,
/// automation, goal continuation, routed automation results and
/// notifications) are not.
pub(super) fn browser_owner_turn(inbound: &InboundMessage) -> bool {
    method::user_turn(inbound) || inbound.owner_accepted_browser_resume().is_some()
}

/// Tells the host that the owner's turn started and, when dropped, that it
/// ended. Every way a turn ends passes this one drop.
pub(super) struct OwnerTurn {
    host: Arc<dyn BrowserHost>,
    session_id: String,
}

impl OwnerTurn {
    pub(super) fn start(
        config: &AgentLoopConfig,
        inbound: &InboundMessage,
        session_id: &str,
    ) -> Option<Self> {
        if !browser_owner_turn(inbound) {
            return None;
        }
        let host = config.browser_host.clone()?;
        host.turn_started(session_id);
        Some(Self {
            host,
            session_id: session_id.to_owned(),
        })
    }
}

impl Drop for OwnerTurn {
    fn drop(&mut self) {
        self.host.turn_finished(&self.session_id);
    }
}

/// `/new` and `/stop` close the browser the session is using. A takeover
/// of that session ends first, as cancelled rather than closed.
pub(super) fn close_for_session(config: &AgentLoopConfig, session_id: &str) {
    if let Some(host) = &config.browser_host {
        host.cancel_takeover(session_id);
        host.close_for_session(session_id);
    }
}

/// Writes what the browser looked like when `message` first arrived. It
/// replaces whatever the channel carried under the same name. `reply_to` is
/// kept only for the channels whose adapters report the replied-to message.
pub fn stamp_browser_arrival(host: Option<&dyn BrowserHost>, message: &mut InboundMessage) {
    let arrival = host.map(BrowserHost::arrival).unwrap_or_default();
    let reply_to = if matches!(message.channel.as_str(), "telegram" | "discord") {
        message
            .metadata
            .get("reply_to_message_id")
            .and_then(Value::as_str)
            .map(str::to_owned)
    } else {
        None
    };
    message.metadata.insert(
        BROWSER_ARRIVAL_METADATA.to_owned(),
        json!({
            "episode": arrival.episode,
            "asked": arrival.asked,
            "reply_to": reply_to,
        }),
    );
}

/// The arrival stamp of `message` and the message it replies to.
fn arrival_of(message: &InboundMessage) -> (BrowserArrival, Option<String>) {
    let stamp = message.metadata.get(BROWSER_ARRIVAL_METADATA);
    let text = |key: &str| {
        stamp
            .and_then(|stamp| stamp.get(key))
            .and_then(Value::as_str)
            .map(str::to_owned)
    };
    let asked = stamp
        .and_then(|stamp| stamp.get("asked"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    (
        BrowserArrival {
            episode: text("episode"),
            asked,
        },
        text("reply_to"),
    )
}

/// The answer to `/browser` and, for a takeover, the episode whose notice
/// that answer is.
pub(super) struct CommandAnswer {
    pub(super) content: String,
    pub(super) episode_notice: Option<String>,
}

impl CommandAnswer {
    fn text(content: &str) -> Self {
        Self {
            content: content.to_owned(),
            episode_notice: None,
        }
    }
}

/// Answers `/browser`. It reads and writes no session state.
pub(super) fn command_reply(
    config: &AgentLoopConfig,
    args: BrowserCommandArgs,
    message: &InboundMessage,
    session_id: &str,
) -> CommandAnswer {
    let host = config.browser_host.as_deref();
    match args {
        BrowserCommandArgs::Close => {
            let outcome = host.map_or(BrowserCloseOutcome::NothingOpen, |host| {
                host.close_by_user()
            });
            CommandAnswer::text(match outcome {
                BrowserCloseOutcome::Closed => "브라우저를 닫았습니다.",
                BrowserCloseOutcome::Scheduled => "실행 중인 동작이 끝나면 브라우저를 닫습니다.",
                BrowserCloseOutcome::NothingOpen => "열린 브라우저가 없습니다.",
            })
        }
        BrowserCommandArgs::Status => CommandAnswer {
            content: status_text(host),
            episode_notice: None,
        },
        BrowserCommandArgs::Takeover => takeover_reply(host, message, session_id),
        BrowserCommandArgs::Return { label } => {
            let target = label.map_or_else(
                || {
                    let (arrival, reply_to) = arrival_of(message);
                    BrowserReturnTarget::Arrival {
                        channel: message.channel.clone(),
                        chat_id: message.chat_id.clone(),
                        reply_to,
                        arrival,
                    }
                },
                BrowserReturnTarget::Label,
            );
            let outcome = host.map_or_else(
                || match &target {
                    BrowserReturnTarget::Label(_) => BrowserReturnOutcome::Over,
                    BrowserReturnTarget::Arrival { .. } => BrowserReturnOutcome::NotTaken,
                },
                |host| host.return_takeover(&target),
            );
            CommandAnswer::text(match outcome {
                BrowserReturnOutcome::Returned => "브라우저를 돌려줬습니다.",
                BrowserReturnOutcome::ReplyMismatch => RETURN_REPLY_MISMATCH,
                BrowserReturnOutcome::NotAsked => RETURN_NOT_ASKED,
                BrowserReturnOutcome::NotTaken => "넘겨받은 상태가 아닙니다.",
                BrowserReturnOutcome::Over => "그 넘겨받기는 이미 끝났습니다.",
            })
        }
        BrowserCommandArgs::Invalid => {
            CommandAnswer::text("사용법: /browser status | takeover | return [회차] | close")
        }
    }
}

fn takeover_reply(
    host: Option<&dyn BrowserHost>,
    message: &InboundMessage,
    session_id: &str,
) -> CommandAnswer {
    let request = BrowserTakeoverRequest {
        session_id: session_id.to_owned(),
        channel: message.channel.clone(),
        chat_id: message.chat_id.clone(),
        reply_route: reply_route(&message.metadata),
        tool_call_id: None,
    };
    let taken = host.map_or(
        Err(BrowserTakeoverError::Browser(BrowserError::Unavailable)),
        |host| host.takeover(&request),
    );
    match taken {
        Ok(takeover) => CommandAnswer {
            content: format!(
                "브라우저를 넘겨받았습니다({}).\n화면 보기: {}\n끝나면 /browser return으로 돌려주세요. {}분이 지나면 자동으로 돌려받습니다.",
                takeover.label, takeover.view_url, takeover.minutes
            ),
            episode_notice: Some(takeover.episode),
        },
        Err(BrowserTakeoverError::AlreadyTaken) => CommandAnswer::text("이미 넘겨받은 상태입니다."),
        Err(BrowserTakeoverError::ViewUnavailable(reason)) => CommandAnswer {
            content: format!("화면 보기를 쓸 수 없습니다({reason})"),
            episode_notice: None,
        },
        Err(BrowserTakeoverError::Failed(reason)) => CommandAnswer {
            content: format!("넘겨주지 못했습니다({reason})"),
            episode_notice: None,
        },
        Err(BrowserTakeoverError::Browser(error)) => CommandAnswer {
            content: error.to_string(),
            episode_notice: None,
        },
    }
}

fn status_text(host: Option<&dyn BrowserHost>) -> String {
    let description = host.map_or(
        BrowserDescription::without_browser(BrowserDescriptionState::Unavailable),
        |host| host.describe(),
    );
    let state = match description.state {
        BrowserDescriptionState::Open => "열림",
        BrowserDescriptionState::Closed => "닫힘",
        BrowserDescriptionState::Disabled => "꺼짐",
        BrowserDescriptionState::Unavailable => "쓸 수 없음",
    };
    let session = description.session.as_deref().unwrap_or("없음");
    let takeover = description.control.takeover.as_ref();
    let reply_wait = if takeover.is_some() {
        "멈춤".to_owned()
    } else {
        description
            .reply_wait_remaining_minutes
            .map_or_else(|| "아님".to_owned(), |minutes| format!("남은 {minutes}분"))
    };
    let control = if takeover.is_some() {
        "사용자"
    } else {
        "어시스턴트"
    };
    let taken = takeover.map_or_else(
        || "없음".to_owned(),
        |takeover| {
            format!(
                "{} {} (남은 {}분)",
                takeover.session, takeover.label, takeover.remaining_minutes
            )
        },
    );
    let view = match &description.control.view {
        BrowserViewDescription::Open(url) => url.clone(),
        BrowserViewDescription::Off(reason) => format!("꺼짐({reason})"),
    };
    format!(
        "브라우저: {state}\n쓰는 세션: {session}\n답 기다리기: {reply_wait}\n조작: {control}\n넘겨받은 곳: {taken}\n화면 보기: {view}"
    )
}

/// The episode a paused `handoff` call of this session opened, when it is
/// open now: the question going out for that call is its notice.
pub(super) fn handoff_notice(
    config: &AgentLoopConfig,
    session_id: &str,
    tool_call_id: &str,
) -> Option<String> {
    let host = config.browser_host.as_deref()?;
    match host.handoff_episode(session_id, tool_call_id) {
        BrowserHandoffEpisode::Open { episode, .. } => Some(episode),
        BrowserHandoffEpisode::Over => None,
    }
}

/// How an inbound continues the session's paused `handoff` call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum HandoffResume {
    /// The user answered while the call's episode is open.
    Answer {
        tool_call_id: String,
        episode: String,
    },
    /// The user wrote after the episode ended, or this process never knew it.
    Over { tool_call_id: String },
    /// The episode ended some other way and the runtime continues the call.
    Ended(BrowserPendingResume),
}

/// What the loop does with an inbound before it may touch the session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum HandoffGate {
    /// Nothing here concerns a paused `handoff` call.
    Pass,
    Resume(HandoffResume),
    /// A user message that is not an answer: say this in its room and change
    /// nothing.
    Reply(&'static str),
    /// A continuation that does not match what the host and the session
    /// hold: drop it and change nothing.
    Discard,
}

/// Whether the inbound claims to be the runtime's continuation of a paused
/// `handoff` call. A claim is not trust: [`handoff_gate`] verifies it.
pub(super) fn resume_candidate(inbound: &InboundMessage) -> bool {
    inbound.owner_accepted_browser_resume().is_some()
        || inbound
            .metadata
            .get("injected_event")
            .and_then(Value::as_str)
            == Some(BROWSER_RESUME_EVENT)
        || (inbound.channel == "system" && inbound.sender_id == "browser")
}

/// Decides how `inbound` meets the session's paused `handoff` call
/// (PRD 010 scopes 8 and 9). It changes nothing in the host.
pub(super) fn handoff_gate(
    config: &AgentLoopConfig,
    inbound: &InboundMessage,
    session_id: &str,
    history: &[Value],
) -> HandoffGate {
    let paused = pending_paused_call(history)
        .filter(|call| call.kind == PausedCallKind::BrowserHandoff)
        .map(|call| call.id);
    if resume_candidate(inbound) {
        return verified_resume(config, inbound, session_id, paused.as_deref())
            .map_or(HandoffGate::Discard, |record| {
                HandoffGate::Resume(HandoffResume::Ended(record))
            });
    }
    let Some(tool_call_id) = paused else {
        return HandoffGate::Pass;
    };
    if !method::user_turn(inbound) {
        return HandoffGate::Pass;
    }
    let open = config
        .browser_host
        .as_deref()
        .map_or(BrowserHandoffEpisode::Over, |host| {
            host.handoff_episode(session_id, &tool_call_id)
        });
    let BrowserHandoffEpisode::Open {
        episode,
        channel,
        chat_id,
        notice_ids,
    } = open
    else {
        return HandoffGate::Resume(HandoffResume::Over { tool_call_id });
    };
    let (arrival, reply_to) = arrival_of(inbound);
    let answers = match reply_to {
        Some(reply_to) => {
            if inbound.channel == channel
                && inbound.chat_id == chat_id
                && notice_ids.contains(&reply_to)
            {
                Ok(())
            } else {
                Err(NOT_THIS_NOTICE)
            }
        }
        None if arrival.asked && arrival.episode.as_deref() == Some(episode.as_str()) => Ok(()),
        None => Err(BEFORE_NOTICE),
    };
    match answers {
        Ok(()) => HandoffGate::Resume(HandoffResume::Answer {
            tool_call_id,
            episode,
        }),
        Err(notice) => HandoffGate::Reply(notice),
    }
}

/// Every check of scope 9: the in-memory mark (not metadata), the sender,
/// the session's paused call and the host's waiting record must all agree.
fn verified_resume(
    config: &AgentLoopConfig,
    inbound: &InboundMessage,
    session_id: &str,
    paused: Option<&str>,
) -> Option<BrowserPendingResume> {
    let resume = inbound.owner_accepted_browser_resume()?;
    if inbound.channel != "system" || inbound.sender_id != "browser" {
        return None;
    }
    if paused != Some(resume.tool_call_id.as_str()) || resume_text(&resume.reason).is_none() {
        return None;
    }
    let record = config
        .browser_host
        .as_deref()?
        .pending_resume(&resume.episode)?;
    (record.episode == resume.episode
        && record.reason == resume.reason
        && record.session_id == session_id
        && record.tool_call_id == resume.tool_call_id)
        .then_some(record)
}

fn resume_text(reason: &str) -> Option<&'static str> {
    Some(match reason {
        "returned" => "browser_returned: 사용자가 돌려줬습니다. 다시 look하세요.",
        "limit" => "browser_takeover_timeout: 넘겨받기 시간이 지났습니다. 다시 look하세요.",
        "closed" => "browser_closed: 넘겨받은 동안 브라우저가 닫혔습니다. 페이지를 다시 여세요.",
        "view_failed" => "browser_view_failed: 화면 보기가 꺼져 돌려받았습니다. 다시 look하세요.",
        _ => return None,
    })
}

/// Makes the tool result that closes the paused call, ending the episode
/// when the user answered it. `user_text` is the user's message; the
/// runtime does not interpret it.
pub(super) fn handoff_result(
    config: &AgentLoopConfig,
    session_id: &str,
    resume: &HandoffResume,
    user_text: &str,
) -> String {
    let host = config.browser_host.as_deref();
    let over = |tool_call_id: &str| {
        if let Some(host) = host {
            host.handoff_closed(session_id, tool_call_id);
        }
        format!("browser_takeover_over: 넘겨받기는 이미 끝났습니다. 다시 look하세요.\n{user_text}")
    };
    match resume {
        HandoffResume::Answer {
            tool_call_id,
            episode,
        } => {
            if host.is_some_and(|host| host.finish_handoff(session_id, tool_call_id, episode)) {
                format!(
                    "browser_handoff_answer: 사용자가 답했고 브라우저를 돌려받았습니다. 다시 look하세요.\n{user_text}"
                )
            } else {
                over(tool_call_id)
            }
        }
        HandoffResume::Over { tool_call_id } => over(tool_call_id),
        HandoffResume::Ended(record) => resume_text(&record.reason)
            .unwrap_or("browser_closed: 넘겨받은 동안 브라우저가 닫혔습니다. 페이지를 다시 여세요.")
            .to_owned(),
    }
}

/// The paused call was closed by something other than a handoff
/// continuation, so the host stops holding anything for it.
pub(super) fn handoff_closed(config: &AgentLoopConfig, session_id: &str, tool_call_id: &str) {
    if let Some(host) = &config.browser_host {
        host.handoff_closed(session_id, tool_call_id);
    }
}

/// The continuation was saved in the session.
pub(super) fn resume_settled(config: &AgentLoopConfig, resume: &HandoffResume) {
    if let (Some(host), HandoffResume::Ended(record)) = (&config.browser_host, resume) {
        host.resume_settled(&record.episode);
    }
}

/// Marks an outbound as the notice of `episode`, so the place that really
/// sends it can tell the host it was delivered.
pub(super) fn mark_episode_notice(outbound: &mut OutboundMessage, episode: Option<String>) {
    if let Some(episode) = episode {
        outbound.metadata.insert(
            BROWSER_EPISODE_NOTICE_METADATA.to_owned(),
            Value::String(episode),
        );
    }
}

/// The inbound the continued turn runs on. A continuation the runtime made
/// answers in the room the takeover started in, and in the same topic or
/// thread of it. Both come from the host's record and never from the
/// inbound's own metadata.
pub(super) fn resumed_inbound(
    mut inbound: InboundMessage,
    resume: Option<&HandoffResume>,
) -> InboundMessage {
    if let Some(HandoffResume::Ended(record)) = resume {
        inbound.channel.clone_from(&record.channel);
        inbound.chat_id.clone_from(&record.chat_id);
        inbound.metadata.remove("message_id");
        for key in REPLY_ROUTE_METADATA_KEYS {
            inbound.metadata.remove(key);
        }
        inbound.metadata.extend(reply_route(&record.reply_route));
    }
    inbound
}

/// `/stop` closes the session's paused `handoff` call without a model turn.
pub(super) fn close_paused_handoff(session: &mut Session) {
    if let Some(call) = pending_paused_call(&session.messages)
        .filter(|call| call.kind == PausedCallKind::BrowserHandoff)
    {
        append_ask_user_resume(session, &call.id, HANDOFF_STOPPED);
    }
}
