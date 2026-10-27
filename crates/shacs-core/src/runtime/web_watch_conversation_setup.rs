use shacs_session::Session;

use super::entrusted_work_surface::create_entrusted_work_in;
use super::web_watch_conversation::{
    PendingWebWatchSummary, SummaryRead, WebWatchChange, WebWatchConversationConfig, WebWatchSetup,
};
use super::web_watch_conversation_summary::revised_summary;
use super::web_watch_deadline_surface::next_deadline_settings;
use super::web_watch_fetch::display_url;
use super::web_watch_notify_surface::{email_sendable, next_notify_settings};
use super::web_watch_redact::safe_text;
use super::web_watch_store::{
    store_web_deadline_settings, store_web_notify_settings, web_deadline_settings,
    web_notify_settings,
};
use super::web_watch_surface::{
    candidate_extraction, candidate_token, confirmation_candidate, read_confirming,
    store_confirmed_watch, ConfirmingRead, ShownValue,
};
use super::{
    EntrustedWorkConditions, EntrustedWorkConsentKind, EntrustedWorkConsentProof,
    EntrustedWorkCreateRequest, EntrustedWorkError, EntrustedWorkReadRequest, WebWatchError,
    DEFAULT_GOAL_TURN_BUDGET,
};

const MAX_REASON_CHARS: usize = 240;

/// What a confirmation did.
#[derive(Debug)]
pub(super) enum SetupOutcome {
    /// Everything in the summary is set; the text says what.
    Done(String),
    /// The page shows another value now: this new revision has to be confirmed instead.
    Revised(Box<PendingWebWatchSummary>),
    /// Nothing was set.
    Failed(String),
}

/// Sets up everything `summary` shows in the loaded owner `session`, or nothing: the page is
/// read again first, and every setting is applied to a copy that replaces `session` only when
/// all of them succeed. The caller saves the session once.
pub(super) fn set_up(
    session: &mut Session,
    summary: &PendingWebWatchSummary,
    config: Option<&WebWatchConversationConfig>,
    now_ms: u64,
) -> SetupOutcome {
    let Some(config) = config else {
        return SetupOutcome::Failed(
            "web page watches cannot be set up from this conversation".to_owned(),
        );
    };
    let setup = &summary.setup;
    if let Some(change) = &setup.change {
        if !unchanged_since(session, change) {
            return SetupOutcome::Failed(
                "the watch was changed after this summary was shown, so the summary no longer \
                 matches it"
                    .to_owned(),
            );
        }
    }
    let confirmed = match &setup.read {
        Some(read) => match reread(setup, read, config) {
            Ok(confirmed) => Some(confirmed),
            Err(Reread::Changed(current)) => {
                return match revised_read(setup, read, current) {
                    Ok(read) => {
                        SetupOutcome::Revised(Box::new(revised_summary(summary, read, now_ms)))
                    }
                    Err(error) => SetupOutcome::Failed(reason(&error)),
                }
            }
            Err(Reread::Failed(error)) => return SetupOutcome::Failed(reason(&error)),
        },
        None => None,
    };
    let mut staged = session.clone();
    match apply(&mut staged, summary, confirmed, config, now_ms) {
        Ok(text) => {
            *session = staged;
            SetupOutcome::Done(text)
        }
        Err(error) => SetupOutcome::Failed(reason(&error)),
    }
}

/// Whether the observation, notification and deadline revisions are all still the ones the
/// change summary was made against, whichever of them it changes.
fn unchanged_since(session: &Session, change: &WebWatchChange) -> bool {
    let work = EntrustedWorkReadRequest {
        owner_session_id: session.key.clone(),
        requester_target: session.key.clone(),
        work_id: change.work_id.clone(),
    };
    let condition = super::web_watch_surface::writable_work(session, &work)
        .ok()
        .map(|(works, index, _)| works[index].current_revision);
    let notify = web_notify_settings(session, &change.work_id)
        .ok()
        .map(|settings| settings.map_or(0, |settings| settings.revision));
    let deadline = web_deadline_settings(session, &change.work_id)
        .ok()
        .map(|settings| settings.map_or(0, |settings| settings.revision));
    (condition, notify, deadline)
        == (
            Some(change.condition_revision),
            Some(change.notify_revision),
            Some(change.deadline_revision),
        )
}

enum Reread {
    Changed(ShownValue),
    Failed(WebWatchError),
}

fn reread(
    setup: &WebWatchSetup,
    read: &SummaryRead,
    config: &WebWatchConversationConfig,
) -> Result<super::web_watch_surface::ConfirmedRead, Reread> {
    let candidate = confirmation_candidate(
        &setup.url,
        &read.candidate_token,
        setup.condition.clone(),
        setup.cadence_seconds,
        &config.guard,
    )
    .map_err(Reread::Failed)?;
    match read_confirming(&setup.url, candidate, &config.guard, &config.limits) {
        Ok(ConfirmingRead::Same(confirmed)) => Ok(*confirmed),
        Ok(ConfirmingRead::Changed(current)) => Err(Reread::Changed(current)),
        Err(error) => Err(Reread::Failed(error)),
    }
}

fn revised_read(
    setup: &WebWatchSetup,
    read: &SummaryRead,
    current: ShownValue,
) -> Result<SummaryRead, WebWatchError> {
    let extraction = candidate_extraction(&read.candidate_token)?;
    Ok(SummaryRead {
        candidate_token: candidate_token(&setup.url, &extraction, &current.value)?,
        shown: current.value,
        shown_text: current.shown_text,
    })
}

fn apply(
    session: &mut Session,
    summary: &PendingWebWatchSummary,
    confirmed: Option<super::web_watch_surface::ConfirmedRead>,
    config: &WebWatchConversationConfig,
    now_ms: u64,
) -> Result<String, WebWatchError> {
    let setup = &summary.setup;
    let evidence_ref = format!(
        "web-watch-summary:{}:{}",
        summary.summary_id, summary.revision
    );
    let (work_id, expected, notify_revision, deadline_revision) = match &setup.change {
        Some(change) => (
            change.work_id.clone(),
            change.condition_revision,
            change.notify_revision,
            change.deadline_revision,
        ),
        None => {
            let (work, _) = create_entrusted_work_in(
                session,
                EntrustedWorkCreateRequest {
                    owner_session_id: session.key.clone(),
                    goal_text: setup.goal_text.clone(),
                    turn_budget: DEFAULT_GOAL_TURN_BUDGET,
                    consent: EntrustedWorkConsentProof {
                        kind: EntrustedWorkConsentKind::ConfirmedProposal,
                        evidence_ref: evidence_ref.clone(),
                        recorded_at: now_ms.to_string(),
                    },
                    conditions: EntrustedWorkConditions {
                        instructions: INSTRUCTIONS.to_owned(),
                        budget: None,
                        prior_decisions: Vec::new(),
                        remaining_questions: Vec::new(),
                    },
                    linked_targets: Vec::new(),
                    observed_at: now_ms.to_string(),
                },
            )?;
            (work.work_id, 1, 0, 0)
        }
    };
    let work = EntrustedWorkReadRequest {
        owner_session_id: session.key.clone(),
        requester_target: session.key.clone(),
        work_id: work_id.clone(),
    };
    let mut done = Vec::new();
    if let Some(confirmed) = confirmed {
        let configured = store_confirmed_watch(
            session,
            &work,
            expected,
            setup.url.clone(),
            confirmed,
            now_ms,
        )?;
        done.push(format!(
            "watching {} every {} from a baseline read now (condition revision {})",
            display_url(&setup.url),
            super::web_watch_notify_render::duration_text(
                setup.cadence_seconds.saturating_mul(1000)
            ),
            configured.condition_revision
        ));
    } else {
        let current = super::web_watch_surface::writable_work(session, &work)?;
        let actual = current.0[current.1].current_revision;
        if actual != expected {
            return Err(EntrustedWorkError::RevisionConflict { expected, actual }.into());
        }
    }
    let notify_changed = setup.change.as_ref().map_or(true, |change| change.notify);
    if !notify_changed && setup.notify_target.is_some() {
        email_sendable(config.email_ready)?;
    }
    if notify_changed {
        let floor = setup.cadence_seconds.saturating_mul(2);
        let (settings, _) = next_notify_settings(
            session,
            work_id.clone(),
            setup.notify_target.clone(),
            setup
                .failure_threshold_seconds
                .map(|seconds| seconds.max(floor)),
            config.email_ready,
            now_ms,
        )?;
        if settings.revision != notify_revision.saturating_add(1) {
            return Err(EntrustedWorkError::RevisionConflict {
                expected: notify_revision,
                actual: settings.revision.saturating_sub(1),
            }
            .into());
        }
        done.push(format!(
            "notification settings revision {}",
            settings.revision
        ));
        store_web_notify_settings(session, settings).map_err(EntrustedWorkError::Metadata)?;
    }
    let deadline_changed = setup
        .change
        .as_ref()
        .map_or(setup.deadline.is_some(), |change| change.deadline);
    if deadline_changed {
        let settings = next_deadline_settings(
            session,
            work_id.clone(),
            deadline_revision,
            setup.deadline.clone(),
            now_ms,
        )?;
        done.push(format!("deadline revision {}", settings.revision));
        store_web_deadline_settings(session, settings).map_err(EntrustedWorkError::Metadata)?;
    }
    let what = match &setup.change {
        Some(_) => "Changed",
        None => "Set up",
    };
    Ok(format!(
        "{what} web watch `{work_id}` as confirmed in summary `{}` (revision {}): {}. Ask me when \
         it was last checked at any time; `/work pause|resume|done|clear {work_id}` controls it.",
        summary.summary_id,
        summary.revision,
        done.join("; ")
    ))
}

/// The work's instructions name no setting a confirmed change can alter: the page, value,
/// condition, interval, notification and deadline stay only in the web watch stores.
const INSTRUCTIONS: &str = "Watch one web page value as this work's web watch is set up now (its \
                            page, value, condition, check interval, notification and deadline: \
                            see `work web-status`).";

fn reason(error: &WebWatchError) -> String {
    safe_text(&error.to_string(), MAX_REASON_CHARS)
}
