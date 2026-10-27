use serde_json::Value;
use shacs_session::Session;

use super::goal::persistent_goal_result_from_session;
use super::learned_method_source::marked_owner_turn;
use super::web_watch_conversation::{
    PageNow, PendingWebWatchSummary, SummaryRead, WebWatchChange, WebWatchConversationConfig,
    WebWatchSetup, WEB_WATCH_CANCEL_REPLIES, WEB_WATCH_CONFIRM_REPLIES, WEB_WATCH_SUMMARY_TTL_MS,
};
use super::web_watch_deadline_surface::deadline;
use super::web_watch_decision::condition_matches;
use super::web_watch_fetch::{display_url, validate_watch_url};
use super::web_watch_notify_render::{condition_text, duration_text, time};
use super::web_watch_notify_surface::valid_address;
use super::web_watch_number::parse_plain_decimal;
use super::web_watch_redact::{safe_extraction, safe_text, MAX_PHRASE_CHARS};
use super::web_watch_store::{web_deadline_settings, web_notify_settings, web_watches};
use super::web_watch_surface::{
    candidate_extraction, candidate_token, checked_condition, checked_target, preview_page,
    read_value, writable_work,
};
use super::{
    EntrustedWorkReadRequest, InboundMessage, WebCondition, WebDeadlineInput, WebNotifySettings,
    WebObservedValue, WebPreviewTarget, WebTextArea, WebValueKind, WebWatchError,
    WEB_NOTIFY_MAX_THRESHOLD_SECONDS, WEB_WATCH_MAX_CADENCE_SECONDS, WEB_WATCH_MIN_CADENCE_SECONDS,
};
use crate::tools::WebWatchProposalDraft;

const DEFAULT_CADENCE_SECONDS: u64 = 3_600;
const MAX_GOAL_CHARS: usize = 240;
const MAX_REASON_CHARS: usize = 240;
const SUPPORTED_CONDITIONS: &str = "for a number: at most or at least a value, or below or above \
     the current value; for a phrase: shown or not shown";

/// The turn that proposed a watch: who asked, where, and with what the summary is checked.
pub(super) struct SummaryContext<'a> {
    pub session: &'a Session,
    pub message: &'a InboundMessage,
    pub config: Option<&'a WebWatchConversationConfig>,
    pub summary_id: String,
    pub evidence_ref: String,
    pub epoch: &'a str,
    pub now_ms: u64,
}

/// A proposal that cannot be set up: why, and what to try instead. Nothing is pending.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct SummaryProblem {
    pub reason: String,
    pub alternative: String,
}

fn problem(reason: impl Into<String>, alternative: impl Into<String>) -> SummaryProblem {
    SummaryProblem {
        reason: reason.into(),
        alternative: alternative.into(),
    }
}

/// A proposal from a turn that also used another tool: what that tool read (a page, a file, a
/// search) may have shaped the condition or the interval, so the proposal is not used.
pub(super) fn proposal_after_other_content() -> SummaryProblem {
    problem(
        "the proposal came from a turn that also read other content (a page, a file or another \
         tool), which could have shaped it, so it is not used",
        "ask again; the assistant proposes from your words and the runtime reads the page itself",
    )
}

/// A proposal from a turn the user did not send (a scheduled job, a sub-task's result, an
/// automation): only the user can ask for a watch, and no one could confirm it.
pub(super) fn proposal_not_from_user() -> SummaryProblem {
    problem(
        "the proposal came from a turn you did not send (a scheduled job, a task's result or an \
         automation), so it is not used",
        "ask for the watch yourself in this conversation",
    )
}

/// A proposal from a turn that someone else started or went on with: a summary waits for one
/// sender, and the turn was not all theirs.
pub(super) fn proposal_from_another_sender() -> SummaryProblem {
    problem(
        "the proposal came from a turn that someone else in this conversation started or \
         answered, so it is not used",
        "ask for the watch yourself and answer its questions yourself",
    )
}

fn error_text(error: &WebWatchError) -> String {
    safe_text(&error.to_string(), MAX_REASON_CHARS)
}

/// Checks a proposal against the conversation, the settings and the page, and makes the
/// summary for its sender. The page is only read for the value and its location; the address,
/// condition, interval and target come from the proposal and must pass the same checks as the
/// commands.
pub(super) fn build_summary(
    context: &SummaryContext<'_>,
    draft: &WebWatchProposalDraft,
) -> Result<PendingWebWatchSummary, SummaryProblem> {
    let config = context.config.ok_or_else(|| {
        problem(
            "web page watches cannot be set up from this conversation",
            "use the `work web-preview` and `work web-watch` commands",
        )
    })?;
    let setup = match draft.work_id.as_deref().map(str::trim) {
        Some(work_id) if !work_id.is_empty() => change_setup(context, config, draft, work_id)?,
        _ => new_setup(context, config, draft)?,
    };
    let mut summary = PendingWebWatchSummary {
        summary_id: context.summary_id.clone(),
        revision: 1,
        epoch: context.epoch.to_owned(),
        session_key: context.session.key.clone(),
        source_session_key: context.message.session_key(),
        channel: context.message.channel.clone(),
        chat_id: context.message.chat_id.clone(),
        sender_id: context.message.sender_id.clone(),
        shown_at_ms: context.now_ms,
        expires_at_ms: context.now_ms.saturating_add(WEB_WATCH_SUMMARY_TTL_MS),
        message_index: 0,
        other_replies: 0,
        evidence_ref: context.evidence_ref.clone(),
        setup,
        text: String::new(),
    };
    summary.text = render_summary(&summary, None);
    Ok(summary)
}

/// The next revision of `summary` showing the value read now; it has to be confirmed again.
pub(super) fn revised_summary(
    summary: &PendingWebWatchSummary,
    read: SummaryRead,
    now_ms: u64,
) -> PendingWebWatchSummary {
    let mut revised = summary.clone();
    revised.revision = summary.revision.saturating_add(1);
    revised.shown_at_ms = now_ms;
    revised.expires_at_ms = now_ms.saturating_add(WEB_WATCH_SUMMARY_TTL_MS);
    revised.setup.read = Some(read);
    revised.text = render_summary(
        &revised,
        Some("The value on the page changed since the last summary, so nothing was set up."),
    );
    revised
}

fn new_setup(
    context: &SummaryContext<'_>,
    config: &WebWatchConversationConfig,
    draft: &WebWatchProposalDraft,
) -> Result<WebWatchSetup, SummaryProblem> {
    let active = persistent_goal_result_from_session(context.session)
        .ok()
        .flatten()
        .is_some_and(|goal| !goal.is_terminal());
    if active {
        return Err(problem(
            "another entrusted work is already active in this conversation",
            "change that watch, or end it first with `/work done <work-id>` or `/work clear <work-id>`",
        ));
    }
    let url = required(draft.url.as_deref(), "the page address")?;
    let target = draft_target(draft)?.ok_or_else(|| {
        problem(
            "what to watch on the page is missing",
            "say the text shown right before the number, or the phrase to watch for",
        )
    })?;
    let ask = draft_condition(draft)?
        .ok_or_else(|| problem("the condition is missing", SUPPORTED_CONDITIONS))?;
    let email = required(draft.notify_email.as_deref(), "the email address to notify")?;
    let url = checked_url(context, config, url)?;
    let cadence_seconds =
        checked_cadence(draft.cadence_seconds.unwrap_or(DEFAULT_CADENCE_SECONDS))?;
    let notify_target = checked_email(context, config, email)?;
    let failure_threshold_seconds =
        checked_threshold(draft.failure_threshold_seconds, cadence_seconds)?;
    let deadline = checked_deadline(draft_deadline(draft, None)?, context.now_ms)?;
    let (read, extraction, target) = read_target(&url, &target, config)?;
    let condition = resolved_condition(ask, &read.shown, &extraction.value)?;
    let goal_text = draft
        .goal_text
        .as_deref()
        .map(str::trim)
        .filter(|goal| !goal.is_empty())
        .map_or_else(|| format!("Watch {}", display_url(&url)), str::to_owned);
    Ok(WebWatchSetup {
        goal_text: safe_text(&goal_text, MAX_GOAL_CHARS),
        value_text: value_text(&target, &extraction),
        phrase: phrase_of(&target),
        url,
        condition,
        cadence_seconds,
        notify_target: Some(notify_target),
        failure_threshold_seconds,
        deadline,
        read: Some(read),
        now: None,
        change: None,
    })
}

fn change_setup(
    context: &SummaryContext<'_>,
    config: &WebWatchConversationConfig,
    draft: &WebWatchProposalDraft,
    work_id: &str,
) -> Result<WebWatchSetup, SummaryProblem> {
    let session = context.session;
    let request = EntrustedWorkReadRequest {
        owner_session_id: session.key.clone(),
        requester_target: session.key.clone(),
        work_id: work_id.to_owned(),
    };
    let (works, index, goal) = writable_work(session, &request).map_err(|error| {
        problem(
            format!("that watch cannot be changed now: {}", error_text(&error)),
            "resume it with `/work resume <work-id>`, or ask for a new watch",
        )
    })?;
    let work = &works[index];
    let entry = web_watches(session)
        .ok()
        .into_iter()
        .flatten()
        .find(|entry| entry.work_id == work.work_id)
        .ok_or_else(|| problem("that work does not watch a web page", "ask for a new watch"))?;
    let notify = web_notify_settings(session, &work.work_id).ok().flatten();
    let deadline_settings = web_deadline_settings(session, &work.work_id).ok().flatten();
    let plan = &entry.plan;

    let url = match draft.url.as_deref().map(str::trim) {
        Some(url) if url != plan.url => checked_url(context, config, url)?,
        _ => plan.url.clone(),
    };
    let target = draft_target(draft)?;
    let ask = draft_condition(draft)?;
    let cadence_seconds = checked_cadence(draft.cadence_seconds.unwrap_or(plan.cadence_seconds))?;
    let observation_asked = url != plan.url
        || target.is_some()
        || ask.is_some()
        || cadence_seconds != plan.cadence_seconds;
    let mut value = (extraction_text(&plan.extraction), None);
    let mut now = None;
    let (read, condition) = if observation_asked {
        let (read, kind) = match &target {
            Some(target) => {
                let (read, extraction, target) = read_target(&url, target, config)?;
                value = (value_text(&target, &extraction), phrase_of(&target));
                (read, extraction.value)
            }
            None => {
                if let WebValueKind::Text { phrase } = &plan.extraction.value {
                    value.1 = Some(safe_text(phrase, MAX_PHRASE_CHARS));
                }
                (
                    current_read(&url, &plan.extraction, config)?,
                    plan.extraction.value.clone(),
                )
            }
        };
        let condition = match ask {
            Some(ask) => resolved_condition(ask, &read.shown, &kind)?,
            None => checked_condition(plan.condition.clone(), &kind)
                .map_err(|error| problem(error_text(&error), SUPPORTED_CONDITIONS))?,
        };
        let extraction_changed = candidate_extraction(&read.candidate_token)
            .map_or(true, |extraction| extraction != plan.extraction);
        let changed = url != plan.url
            || extraction_changed
            || condition != plan.condition
            || cadence_seconds != plan.cadence_seconds;
        if changed {
            (Some(read), condition)
        } else {
            now = Some(PageNow::Read {
                shown: read.shown,
                shown_text: read.shown_text,
            });
            (None, condition)
        }
    } else {
        if let WebValueKind::Text { phrase } = &plan.extraction.value {
            value.1 = Some(safe_text(phrase, MAX_PHRASE_CHARS));
        }
        (None, plan.condition.clone())
    };

    let current_target = notify.as_ref().and_then(|settings| settings.target.clone());
    let current_threshold = notify
        .as_ref()
        .and_then(|settings| settings.failure_threshold_seconds);
    let notify_target = match draft.notify_email.as_deref() {
        Some(email) => Some(checked_email(context, config, email)?),
        None => current_target.clone(),
    };
    if notify_target.is_some() {
        email_sendable(config)?;
    }
    let failure_threshold_seconds = match draft.failure_threshold_seconds {
        Some(seconds) => checked_threshold(Some(seconds), cadence_seconds)?,
        None => current_threshold,
    };
    let notify_changed =
        notify_target != current_target || failure_threshold_seconds != current_threshold;

    let current_deadline = deadline_settings
        .as_ref()
        .and_then(|settings| settings.deadline.as_ref())
        .map(|deadline| WebDeadlineInput {
            at: deadline.at_text.clone(),
            remind_before_seconds: deadline.remind_before_seconds,
            remind_now: deadline.remind_now,
        });
    let (deadline, deadline_changed) = if draft.clear_deadline {
        let changed = current_deadline.is_some();
        (None, changed)
    } else {
        match draft_deadline(draft, current_deadline.as_ref())? {
            Some(input) if Some(&input) != current_deadline.as_ref() => {
                (checked_deadline(Some(input), context.now_ms)?, true)
            }
            _ => (current_deadline, false),
        }
    };
    if deadline.is_some() && notify_target.is_none() {
        return Err(problem(
            "a decision deadline needs an email address to remind",
            "say the email address to notify",
        ));
    }
    if read.is_none() && !notify_changed && !deadline_changed {
        return Err(problem(
            "nothing would change",
            "say what to change: the page, the value, the condition, the check interval, the \
             email address, the failure time, the deadline or its reminder",
        ));
    }
    let now = match (&read, now) {
        (Some(_), _) => None,
        (None, Some(now)) => Some(now),
        (None, None) => Some(page_now(&url, &plan.extraction, config)),
    };
    Ok(WebWatchSetup {
        goal_text: safe_text(&goal.text, MAX_GOAL_CHARS),
        url,
        value_text: value.0,
        phrase: value.1,
        change: Some(WebWatchChange {
            work_id: work.work_id.clone(),
            condition_revision: work.current_revision,
            notify_revision: notify.as_ref().map_or(0, |settings| settings.revision),
            deadline_revision: deadline_settings
                .as_ref()
                .map_or(0, |settings| settings.revision),
            observation: read.is_some(),
            notify: notify_changed,
            deadline: deadline_changed,
        }),
        condition,
        cadence_seconds,
        notify_target,
        failure_threshold_seconds,
        deadline,
        read,
        now,
    })
}

fn required<'a>(field: Option<&'a str>, what: &str) -> Result<&'a str, SummaryProblem> {
    field
        .map(str::trim)
        .filter(|field| !field.is_empty())
        .ok_or_else(|| problem(format!("{what} is missing"), format!("say {what}")))
}

/// Whether `needle` appears in what the user wrote: this turn's message (proposals come only
/// from turns the user sent) or an earlier message the user typed. User-role messages the
/// runtime stored for others (a task's result, a scheduled job) do not count, so text read from
/// pages can never supply an address this way.
fn from_user(context: &SummaryContext<'_>, needle: &str, ignore_case: bool) -> bool {
    let fold = |text: &str| {
        if ignore_case {
            text.to_ascii_lowercase()
        } else {
            text.to_owned()
        }
    };
    let needle = fold(needle);
    std::iter::once(context.message.content.as_str())
        .chain(context.session.messages.iter().filter_map(|message| {
            (message.get("role").and_then(Value::as_str) == Some("user")
                && marked_owner_turn(message))
            .then(|| message.get("content").and_then(Value::as_str))
            .flatten()
        }))
        .any(|text| written_whole(&fold(text), &needle))
}

/// Whether `needle` appears in `text` as a whole address, not as the start or the end of a
/// longer one. Punctuation and non-ASCII text (a Korean particle such as `로`) may touch it.
fn written_whole(text: &str, needle: &str) -> bool {
    let touching = |character: char| !character.is_ascii() || character.is_whitespace();
    !needle.is_empty()
        && text.match_indices(needle).any(|(start, _)| {
            let starts = text[..start].chars().next_back().map_or(true, |before| {
                touching(before) || "([{<\"':".contains(before)
            });
            let mut after = text[start + needle.len()..].chars();
            let ends = match after.next() {
                None => true,
                Some('.') => after.next().map_or(true, touching),
                Some(next) => touching(next) || ",;!)]}>\"'".contains(next),
            };
            starts && ends
        })
}

fn checked_url(
    context: &SummaryContext<'_>,
    config: &WebWatchConversationConfig,
    url: &str,
) -> Result<String, SummaryProblem> {
    if !from_user(context, url, false) {
        return Err(problem(
            "the page address must be one you wrote in this conversation",
            "send the page address again",
        ));
    }
    validate_watch_url(url, &config.guard).map_err(|reason| {
        problem(
            format!(
                "this page address is not supported: {}",
                safe_text(&reason, MAX_REASON_CHARS)
            ),
            "use a public http or https page that needs no login, user name or password",
        )
    })?;
    Ok(url.to_owned())
}

fn checked_email(
    context: &SummaryContext<'_>,
    config: &WebWatchConversationConfig,
    email: &str,
) -> Result<String, SummaryProblem> {
    let email = email.trim().trim_start_matches("email:");
    if !valid_address(email) || !from_user(context, email, true) {
        return Err(problem(
            "notifications go to one email address you wrote in this conversation",
            "say the email address to notify",
        ));
    }
    email_sendable(config)?;
    Ok(format!("email:{email}"))
}

fn email_sendable(config: &WebWatchConversationConfig) -> Result<(), SummaryProblem> {
    if config.email_ready {
        Ok(())
    } else {
        Err(problem(
            "the Email channel cannot send",
            "enable `channels.email` with `consentGranted` and SMTP settings, then ask again",
        ))
    }
}

fn checked_cadence(seconds: u64) -> Result<u64, SummaryProblem> {
    if (WEB_WATCH_MIN_CADENCE_SECONDS..=WEB_WATCH_MAX_CADENCE_SECONDS).contains(&seconds) {
        Ok(seconds)
    } else {
        Err(problem(
            "the page can be checked every 10 minutes at most and every day at least",
            "choose a check interval from 10 minutes to 1 day",
        ))
    }
}

fn checked_threshold(
    seconds: Option<u64>,
    cadence_seconds: u64,
) -> Result<Option<u64>, SummaryProblem> {
    let floor = cadence_seconds.saturating_mul(2);
    match seconds {
        Some(seconds) if !(floor..=WEB_NOTIFY_MAX_THRESHOLD_SECONDS).contains(&seconds) => {
            Err(problem(
                format!(
                    "a check failure can be mailed after {} at the earliest and 7 days at the latest",
                    span(floor)
                ),
                "choose a failure time in that range, or leave it to the default",
            ))
        }
        seconds => Ok(seconds),
    }
}

/// The deadline a proposal gives: its deadline fields over the `current` deadline, so a change
/// can give only the reminder. `None` when it gives none of them. A deadline that reminds at the
/// same time as `current` (however its time is written, and whether the default reminder is
/// written out) is `current`: no change, and it keeps an immediate reminder, which is only for
/// a reminder time that has already passed.
fn draft_deadline(
    draft: &WebWatchProposalDraft,
    current: Option<&WebDeadlineInput>,
) -> Result<Option<WebDeadlineInput>, SummaryProblem> {
    let at = draft
        .deadline_at
        .as_deref()
        .map(str::trim)
        .filter(|at| !at.is_empty());
    if at.is_none() && draft.remind_before_seconds.is_none() && !draft.remind_now {
        return Ok(None);
    }
    let at = at
        .map(str::to_owned)
        .or_else(|| current.map(|current| current.at.clone()))
        .ok_or_else(|| {
            problem(
                "a reminder needs a decision deadline",
                "give the deadline with its offset, like 2026-11-01T18:00:00+09:00",
            )
        })?;
    let remind_before_seconds = draft
        .remind_before_seconds
        .or_else(|| current.and_then(|current| current.remind_before_seconds));
    let time = reminder_time(&at, remind_before_seconds);
    if let Some(current) = current.filter(|current| {
        time.is_some()
            && reminder_time(&current.at, current.remind_before_seconds) == time
            && (current.remind_now || !draft.remind_now)
    }) {
        return Ok(Some(current.clone()));
    }
    Ok(Some(WebDeadlineInput {
        at,
        remind_before_seconds,
        remind_now: draft.remind_now,
    }))
}

/// The instant of a deadline written as `at` and its reminder offset (a day when not given), or
/// `None` when `at` cannot be read.
fn reminder_time(at: &str, remind_before_seconds: Option<u64>) -> Option<(i64, u64)> {
    let at_ms = chrono::DateTime::parse_from_rfc3339(at.trim())
        .ok()?
        .timestamp_millis();
    Some((
        at_ms,
        remind_before_seconds.unwrap_or(super::WEB_DEADLINE_DEFAULT_REMIND_BEFORE_SECONDS),
    ))
}

fn checked_deadline(
    input: Option<WebDeadlineInput>,
    now_ms: u64,
) -> Result<Option<WebDeadlineInput>, SummaryProblem> {
    let Some(input) = input else {
        return Ok(None);
    };
    deadline(input.clone(), now_ms).map_err(|error| {
        problem(
            format!(
                "the decision deadline cannot be set: {}",
                error_text(&error)
            ),
            "give a future time with its offset, like 2026-11-01T18:00:00+09:00",
        )
    })?;
    Ok(Some(input))
}

fn draft_target(draft: &WebWatchProposalDraft) -> Result<Option<WebPreviewTarget>, SummaryProblem> {
    let text = |field: &Option<String>| {
        field
            .as_deref()
            .map(str::trim)
            .filter(|field| !field.is_empty())
            .map(str::to_owned)
    };
    match (text(&draft.value_label), text(&draft.value_phrase)) {
        (Some(_), Some(_)) => Err(problem(
            "a watch reads either one number or one phrase",
            "choose the number or the phrase",
        )),
        (Some(label), None) => Ok(Some(WebPreviewTarget::Number {
            label,
            decimal_comma: draft.decimal_comma,
        })),
        (None, Some(phrase)) => {
            let area = match (text(&draft.area_label), text(&draft.area_until)) {
                (Some(label), Some(until)) => Some(WebTextArea { label, until }),
                (None, None) => None,
                _ => {
                    return Err(problem(
                        "a phrase area needs the text that starts it and the text that ends it",
                        "give both, or leave the area out when the phrase is on the page now",
                    ))
                }
            };
            Ok(Some(WebPreviewTarget::Text { phrase, area }))
        }
        (None, None) => Ok(None),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Direction {
    Below,
    Above,
}

/// A condition as asked: fixed, or relative to the value the summary reads.
#[derive(Debug, Clone, PartialEq, Eq)]
enum ConditionAsk {
    Fixed(WebCondition),
    Current(Direction),
}

fn draft_condition(draft: &WebWatchProposalDraft) -> Result<Option<ConditionAsk>, SummaryProblem> {
    let Some(kind) = draft.condition.as_deref().map(str::trim) else {
        return Ok(None);
    };
    let value = || {
        draft
            .condition_value
            .as_deref()
            .and_then(parse_plain_decimal)
            .ok_or_else(|| {
                problem(
                    "the condition needs a plain number",
                    "give the number without separators, like 11000",
                )
            })
    };
    Ok(Some(match kind {
        "at_most" => ConditionAsk::Fixed(WebCondition::AtMost { value: value()? }),
        "at_least" => ConditionAsk::Fixed(WebCondition::AtLeast { value: value()? }),
        "below_current" => ConditionAsk::Current(Direction::Below),
        "above_current" => ConditionAsk::Current(Direction::Above),
        "text_present" => ConditionAsk::Fixed(WebCondition::TextPresent),
        "text_absent" => ConditionAsk::Fixed(WebCondition::TextAbsent),
        other => {
            return Err(problem(
                format!(
                    "the condition `{}` is not supported",
                    safe_text(other, MAX_PHRASE_CHARS)
                ),
                SUPPORTED_CONDITIONS,
            ))
        }
    }))
}

fn resolved_condition(
    ask: ConditionAsk,
    shown: &WebObservedValue,
    kind: &WebValueKind,
) -> Result<WebCondition, SummaryProblem> {
    let condition = match (ask, shown) {
        (ConditionAsk::Fixed(condition), _) => condition,
        (ConditionAsk::Current(direction), WebObservedValue::Number { value }) => {
            let stepped = step(value, direction).ok_or_else(|| {
                problem(
                    "there is no number below the current value",
                    "give a fixed number to compare with",
                )
            })?;
            match direction {
                Direction::Below => WebCondition::AtMost { value: stepped },
                Direction::Above => WebCondition::AtLeast { value: stepped },
            }
        }
        (ConditionAsk::Current(_), WebObservedValue::Text { .. }) => {
            return Err(problem(
                "below or above the current value fits a number only",
                SUPPORTED_CONDITIONS,
            ))
        }
    };
    checked_condition(condition, kind)
        .map_err(|error| problem(error_text(&error), SUPPORTED_CONDITIONS))
}

/// `value` one unit of its last decimal place lower or higher: `12900` -> `12899`, `12.99` ->
/// `12.98`. `None` below zero.
fn step(value: &str, direction: Direction) -> Option<String> {
    let (whole, fraction) = value.split_once('.').unwrap_or((value, ""));
    let number: u128 = format!("{whole}{fraction}").parse().ok()?;
    let number = match direction {
        Direction::Below => number.checked_sub(1)?,
        Direction::Above => number.checked_add(1)?,
    };
    let digits = format!("{number:0width$}", width = fraction.len() + 1);
    let (whole, fraction_digits) = digits.split_at(digits.len() - fraction.len());
    let stepped = if fraction.is_empty() {
        whole.to_owned()
    } else {
        format!("{whole}.{fraction_digits}")
    };
    parse_plain_decimal(&stepped)
}

fn read_target(
    url: &str,
    target: &WebPreviewTarget,
    config: &WebWatchConversationConfig,
) -> Result<(SummaryRead, super::WebExtraction, WebPreviewTarget), SummaryProblem> {
    let target = checked_target(target).map_err(|error| {
        problem(
            error_text(&error),
            "use a label or phrase of 1 to 64 characters as the page shows it",
        )
    })?;
    let preview = preview_page(url, &target, &config.guard, &config.limits).map_err(|error| {
        problem(
            format!("the page could not be read: {}", error_text(&error)),
            "check the address; pages that need a login or JavaScript are not supported",
        )
    })?;
    let candidate = preview.candidates.into_iter().next().ok_or_else(|| {
        problem(
            "the value was not found on the page",
            "use the label or phrase exactly as the page shows it; only text in the page itself \
             is read",
        )
    })?;
    Ok((
        SummaryRead {
            candidate_token: candidate.token,
            shown: candidate.value,
            shown_text: candidate.shown_text,
        },
        candidate.extraction,
        target,
    ))
}

fn current_read(
    url: &str,
    extraction: &super::WebExtraction,
    config: &WebWatchConversationConfig,
) -> Result<SummaryRead, SummaryProblem> {
    let unreadable = |error: WebWatchError| {
        problem(
            format!(
                "the watched value could not be read on the page: {}",
                error_text(&error)
            ),
            "say what to watch on the page again",
        )
    };
    let shown = read_value(url, extraction, &config.guard, &config.limits).map_err(unreadable)?;
    let candidate_token = candidate_token(url, extraction, &shown.value).map_err(unreadable)?;
    Ok(SummaryRead {
        candidate_token,
        shown: shown.value,
        shown_text: shown.shown_text,
    })
}

/// The watched value as the page shows it now, read with the stored criterion for a change that
/// keeps the observation. It is only shown, so a page that cannot be read now is said and does
/// not stop the change.
fn page_now(
    url: &str,
    extraction: &super::WebExtraction,
    config: &WebWatchConversationConfig,
) -> PageNow {
    match read_value(url, extraction, &config.guard, &config.limits) {
        Ok(shown) => PageNow::Read {
            shown: shown.value,
            shown_text: shown.shown_text,
        },
        Err(error) => PageNow::Unreadable {
            reason: error_text(&error),
        },
    }
}

/// What a new criterion watches, in the request's words, and where the criterion it is
/// confirmed with finds it.
fn value_text(target: &WebPreviewTarget, extraction: &super::WebExtraction) -> String {
    let safe = |text: &str| safe_text(text, MAX_PHRASE_CHARS);
    let place = place_text(extraction);
    match target {
        WebPreviewTarget::Number { label, .. } => {
            format!("the number after \"{}\"{place}", safe(label))
        }
        WebPreviewTarget::Text { phrase, area: None } => {
            format!("the phrase \"{}\"{place}", safe(phrase))
        }
        WebPreviewTarget::Text {
            phrase,
            area: Some(area),
        } => format!(
            "the phrase \"{}\" between \"{}\" and \"{}\"{place}",
            safe(phrase),
            safe(&area.label),
            safe(&area.until)
        ),
    }
}

/// What a stored criterion watches and where, as a summary shows it: its anchors made safe like
/// other page text.
fn extraction_text(extraction: &super::WebExtraction) -> String {
    let shown = safe_extraction(extraction);
    let place = place_text(extraction);
    match &shown.value {
        WebValueKind::Number { .. } => {
            format!("the number after \"{}\"{place}", shown.prefix.trim())
        }
        WebValueKind::Text { phrase } => format!(
            "the phrase \"{phrase}\" between \"{}\" and \"{}\"{place}",
            shown.prefix.trim(),
            shown.suffix.trim()
        ),
    }
}

fn place_text(extraction: &super::WebExtraction) -> String {
    if extraction.match_count > 1 {
        format!(
            " (place {} of {} on the page)",
            extraction.occurrence, extraction.match_count
        )
    } else {
        String::new()
    }
}

fn phrase_of(target: &WebPreviewTarget) -> Option<String> {
    match target {
        WebPreviewTarget::Text { phrase, .. } => Some(safe_text(phrase, MAX_PHRASE_CHARS)),
        WebPreviewTarget::Number { .. } => None,
    }
}

fn observed_text(value: &WebObservedValue) -> String {
    match value {
        WebObservedValue::Number { value } => value.clone(),
        WebObservedValue::Text { present: true } => "shown".to_owned(),
        WebObservedValue::Text { present: false } => "not shown".to_owned(),
    }
}

fn span(seconds: u64) -> String {
    duration_text(seconds.saturating_mul(1000))
}

/// The failure time a watch uses when none is given, as `web-notify` computes it.
fn effective_threshold(failure_threshold_seconds: Option<u64>, cadence_seconds: u64) -> u64 {
    WebNotifySettings {
        work_id: String::new(),
        revision: 0,
        target: None,
        failure_threshold_seconds,
        changed_at_ms: 0,
    }
    .threshold_seconds(cadence_seconds)
}

pub(super) fn render_summary(summary: &PendingWebWatchSummary, note: Option<&str>) -> String {
    let setup = &summary.setup;
    let mut lines = vec![format!(
        "Web watch summary `{}` (revision {}): nothing is set up until you confirm.",
        summary.summary_id, summary.revision
    )];
    lines.extend(note.map(str::to_owned));
    match &setup.change {
        Some(change) => lines.push(format!("Changes to watch `{}`:", change.work_id)),
        None => lines.push(format!("- Goal: {}", setup.goal_text)),
    }
    let shown_url = display_url(&setup.url);
    lines.push(if shown_url == setup.url {
        format!("- Page: {shown_url}")
    } else {
        format!(
            "- Page: {shown_url} (the address as you wrote it; its query and fragment are kept \
             but not shown)"
        )
    });
    let condition = condition_text(&setup.condition, setup.phrase.as_deref());
    let now = match (&setup.read, &setup.now) {
        (Some(read), _) => Some((&read.shown, read.shown_text.as_str())),
        (None, Some(PageNow::Read { shown, shown_text })) => Some((shown, shown_text.as_str())),
        (None, Some(PageNow::Unreadable { .. }) | None) => None,
    };
    match now {
        Some((shown, shown_text)) => {
            lines.push(format!(
                "- Value: {}, now {} (shown as \"{shown_text}\")",
                setup.value_text,
                observed_text(shown)
            ));
            let met = if condition_matches(&setup.condition, shown) {
                "met"
            } else {
                "not met"
            };
            lines.push(format!("- Condition: notify when {condition} (now: {met})"));
        }
        None => {
            if let Some(PageNow::Unreadable { reason }) = &setup.now {
                lines.push(format!(
                    "- Value: {}; the page could not be read now ({reason}), so its current \
                     value is not shown",
                    setup.value_text
                ));
            }
            lines.push(format!("- Condition: notify when {condition}"));
        }
    }
    lines.push(format!("- Checked every {}", span(setup.cadence_seconds)));
    lines.push(match &setup.notify_target {
        Some(target) => format!(
            "- Notification: email to {}; a check failure is mailed once it lasts {}",
            target.trim_start_matches("email:"),
            span(effective_threshold(
                setup.failure_threshold_seconds,
                setup.cadence_seconds
            ))
        ),
        None => "- Notification: none".to_owned(),
    });
    lines.push(match &setup.deadline {
        Some(input) => {
            let reminder = if input.remind_now {
                "a reminder right after setup".to_owned()
            } else {
                format!(
                    "a reminder {} before",
                    span(
                        input
                            .remind_before_seconds
                            .unwrap_or(super::WEB_DEADLINE_DEFAULT_REMIND_BEFORE_SECONDS)
                    )
                )
            };
            let at = deadline(input.clone(), summary.shown_at_ms)
                .map_or_else(|_| input.at.clone(), |deadline| time(deadline.at_ms));
            format!(
                "- Decision deadline: {at} (set as {}), {reminder}",
                safe_text(&input.at, MAX_PHRASE_CHARS)
            )
        }
        None => "- Decision deadline: none".to_owned(),
    });
    if let Some(change) = &setup.change {
        lines.push(if change.observation {
            "On confirmation the page is read again and that value becomes the new comparison \
             baseline."
                .to_owned()
        } else {
            "The comparison baseline is kept.".to_owned()
        });
    }
    lines.push(format!(
        "Reply {} to set it up exactly as shown (or send `/watch confirm {} {}`); reply {} to \
         drop it. It waits 30 minutes and closes when another summary is shown or the assistant \
         restarts. Confirming sets up this watch only: it grants no tool permission and \
         approves no booking, purchase or sending.",
        WEB_WATCH_CONFIRM_REPLIES.join(", "),
        summary.summary_id,
        summary.revision,
        WEB_WATCH_CANCEL_REPLIES.join(", ")
    ));
    lines.join("\n")
}

pub(super) fn render_problem(problem: &SummaryProblem) -> String {
    format!(
        "Web watch summary: this cannot be set up as asked, so nothing is pending.\n- Problem: \
         {}\n- Try: {}",
        problem.reason, problem.alternative
    )
}

#[cfg(test)]
mod tests {
    use super::{extraction_text, step, written_whole, Direction};
    use crate::runtime::{WebExtraction, WebValueKind};

    #[test]
    fn a_stored_criterion_is_shown_by_its_anchors_and_its_place() {
        let criterion = |prefix: &str, occurrence, match_count, value| WebExtraction {
            version: "normalized_static_text_v1".to_owned(),
            prefix: prefix.to_owned(),
            suffix: " 배송비 https://x.test/a?ref=secret".to_owned(),
            occurrence,
            match_count,
            value,
        };
        let number = WebValueKind::Number {
            decimal_separator: '.',
            thousands_separator: Some(','),
            unit_before: Some("₩".to_owned()),
            unit_after: None,
        };
        let phrase = WebValueKind::Text {
            phrase: "품절".to_owned(),
        };
        for (extraction, expected) in [
            (
                criterion("판매가 ", 1, 1, number.clone()),
                "the number after \"판매가\"",
            ),
            (
                criterion("판매가 ", 2, 3, number),
                "the number after \"판매가\" (place 2 of 3 on the page)",
            ),
            (
                criterion("상태:", 1, 1, phrase),
                "the phrase \"품절\" between \"상태:\" and \"배송비 https://x.test/a\"",
            ),
        ] {
            assert_eq!(extraction_text(&extraction), expected);
        }
    }

    #[test]
    fn only_a_whole_written_address_counts() {
        for (text, needle, expected) in [
            (
                "watch http://x.test/a?ref=1 please",
                "http://x.test/a?ref=1",
                true,
            ),
            ("watch http://x.test/a?ref=1", "http://x.test/a", false),
            ("watch http://x.test/items", "http://x.test/item", false),
            ("xhttp://x.test/a", "http://x.test/a", false),
            ("http://x.test/a.html", "http://x.test/a", false),
            ("see (http://x.test/a).", "http://x.test/a", true),
            ("user@example.test로 알려줘", "user@example.test", true),
            ("user@example.testing", "user@example.test", false),
        ] {
            assert_eq!(written_whole(text, needle), expected, "{needle} in {text}");
        }
    }

    #[test]
    fn the_current_value_steps_by_its_last_decimal_place() {
        for (value, direction, expected) in [
            ("12900", Direction::Below, Some("12899")),
            ("12.99", Direction::Below, Some("12.98")),
            ("0.5", Direction::Below, Some("0.4")),
            ("9", Direction::Above, Some("10")),
            ("0", Direction::Below, None),
        ] {
            assert_eq!(step(value, direction).as_deref(), expected, "{value}");
        }
    }
}
