use std::path::Path;

use shacs_session::{Session, SessionMutationGuard};

use super::entrusted_work_access::bound_current_work;
use super::entrusted_work_surface::load_owner_session;
use super::web_watch_store::{
    store_web_deadline_settings, web_deadline_settings, web_notify_settings, web_watches,
};
use super::{
    ConfigureWebDeadlineRequest, EntrustedWorkError, WebDeadline, WebDeadlineInput,
    WebDeadlineSettings, WebDeadlineView, WebWatchError,
    WEB_DEADLINE_DEFAULT_REMIND_BEFORE_SECONDS, WEB_DEADLINE_MAX_REMIND_BEFORE_SECONDS,
    WEB_DEADLINE_MIN_REMIND_BEFORE_SECONDS,
};

const MAX_AT_TEXT_BYTES: usize = 64;

/// Sets or clears the decision deadline of a confirmed web watch, compare-and-set on its
/// deadline revision. The observation revision and the baseline stay as they are.
pub fn configure_web_watch_deadline(
    workspace: &Path,
    request: ConfigureWebDeadlineRequest,
) -> Result<WebDeadlineView, WebWatchError> {
    let owner = &request.work.owner_session_id;
    let _guard = SessionMutationGuard::acquire(workspace, owner)?;
    let (mut manager, mut session) = load_owner_session(workspace, owner)?;
    let (works, index, _) = bound_current_work(
        &session,
        &request.work.requester_target,
        &request.work.work_id,
    )?;
    let work_id = works[index].work_id.clone();
    let settings = next_deadline_settings(
        &session,
        work_id,
        request.expected_revision,
        request.deadline,
        request.now_ms,
    )?;
    store_web_deadline_settings(&mut session, settings.clone())
        .map_err(EntrustedWorkError::Metadata)?;
    manager.save_with_fsync(&session)?;
    Ok(deadline_view(&settings))
}

/// The next deadline settings revision of `work_id` in `session`, checked as above.
pub(super) fn next_deadline_settings(
    session: &Session,
    work_id: String,
    expected_revision: u64,
    input: Option<WebDeadlineInput>,
    now_ms: u64,
) -> Result<WebDeadlineSettings, WebWatchError> {
    if !web_watches(session)
        .map_err(EntrustedWorkError::Metadata)?
        .iter()
        .any(|watch| watch.work_id == work_id)
    {
        return Err(invalid("confirm a web watch before setting a deadline"));
    }
    let actual = web_deadline_settings(session, &work_id)
        .map_err(EntrustedWorkError::Metadata)?
        .map_or(0, |settings| settings.revision);
    if expected_revision != actual {
        return Err(EntrustedWorkError::RevisionConflict {
            expected: expected_revision,
            actual,
        }
        .into());
    }
    let deadline = input.map(|input| deadline(input, now_ms)).transpose()?;
    let has_target = web_notify_settings(session, &work_id)
        .map_err(EntrustedWorkError::Metadata)?
        .is_some_and(|settings| settings.target.is_some());
    if deadline.is_some() && !has_target {
        return Err(invalid(
            "set an email notification target (web-notify) before a deadline",
        ));
    }
    Ok(WebDeadlineSettings {
        work_id,
        revision: actual
            .checked_add(1)
            .ok_or_else(|| invalid("deadline revision overflow"))?,
        deadline,
        changed_at_ms: now_ms,
    })
}

pub(super) fn deadline_view(settings: &WebDeadlineSettings) -> WebDeadlineView {
    WebDeadlineView {
        revision: settings.revision,
        deadline: settings.deadline.clone(),
        changed_at_ms: settings.changed_at_ms,
    }
}

pub(super) fn deadline(input: WebDeadlineInput, now_ms: u64) -> Result<WebDeadline, WebWatchError> {
    let at_text = input.at.trim().to_owned();
    let at_ms = (at_text.len() <= MAX_AT_TEXT_BYTES)
        .then(|| chrono::DateTime::parse_from_rfc3339(&at_text).ok())
        .flatten()
        .and_then(|time| u64::try_from(time.timestamp_millis()).ok())
        .ok_or_else(|| {
            invalid(
                "deadline must be RFC 3339 with an offset, like 2026-11-01T18:00:00+09:00, \
                 at most 64 characters",
            )
        })?;
    if at_ms <= now_ms {
        return Err(invalid("the deadline has already passed"));
    }
    let before = input
        .remind_before_seconds
        .unwrap_or(WEB_DEADLINE_DEFAULT_REMIND_BEFORE_SECONDS);
    if !(WEB_DEADLINE_MIN_REMIND_BEFORE_SECONDS..=WEB_DEADLINE_MAX_REMIND_BEFORE_SECONDS)
        .contains(&before)
    {
        return Err(WebWatchError::InvalidInput(format!(
            "remind before must be {WEB_DEADLINE_MIN_REMIND_BEFORE_SECONDS}..\
             {WEB_DEADLINE_MAX_REMIND_BEFORE_SECONDS} seconds"
        )));
    }
    let planned = at_ms.saturating_sub(before.saturating_mul(1000));
    let remind_at_ms = match (planned >= now_ms, input.remind_now) {
        (true, false) => planned,
        (false, true) => now_ms,
        (false, false) => {
            return Err(invalid(
                "the reminder time has already passed: choose an immediate reminder \
                 (remind now) or a shorter remind-before",
            ))
        }
        (true, true) => {
            return Err(invalid(
                "an immediate reminder is only for a reminder time that has already passed",
            ))
        }
    };
    Ok(WebDeadline {
        at_ms,
        at_text,
        remind_at_ms,
        remind_before_seconds: input.remind_before_seconds,
        remind_now: input.remind_now,
    })
}

fn invalid(reason: &str) -> WebWatchError {
    WebWatchError::InvalidInput(reason.to_owned())
}
