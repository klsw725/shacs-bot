use std::path::Path;

use base64::Engine;
use serde::{Deserialize, Serialize};
use shacs_security::NetworkGuard;
use shacs_session::{Session, SessionMutationGuard};

use super::entrusted_work_access::{bound_current_work, latest_transition_id};
use super::entrusted_work_store::store_entrusted_works;
use super::entrusted_work_surface::load_owner_session;
use super::web_watch_decision::condition_matches;
use super::web_watch_extract::{
    candidates, continues_number, extract_shown, CandidateSearchResult, UNSUPPORTED_NUMBER_LEAD,
};
use super::web_watch_fetch::{
    display_url, sha256_ref, status_failure, validate_watch_url, ReadPage, WebFetchLimits,
};
use super::web_watch_number::{is_unit_char, parse_plain_decimal, ALL_SEPARATORS};
use super::web_watch_read::plan_view;
use super::web_watch_redact::{safe_extraction, safe_text, MAX_ANCHOR_CHARS};
use super::web_watch_store::{store_web_watches, web_watches};
use super::web_watch_text::normalize_fragment;
use super::{
    ConfirmWebWatchRequest, EntrustedWork, EntrustedWorkError, EntrustedWorkReadRequest,
    EntrustedWorkRevision, ObservationBlock, ObservationError, PersistentGoal,
    PersistentGoalStatus, RenderedWebReader, WebCandidate, WebCondition, WebEvidence,
    WebExtraction, WebObservedValue, WebPreview, WebPreviewRequest, WebPreviewTarget,
    WebReadingMode, WebResponseFacts, WebTextArea, WebValueKind, WebWatchConfigured, WebWatchEntry,
    WebWatchError, WebWatchPlan, WEB_WATCH_MAX_CADENCE_SECONDS, WEB_WATCH_MIN_CADENCE_SECONDS,
};

const MAX_NEEDLE_CHARS: usize = 64;

/// Opaque preview candidate handed back on confirmation; nothing is stored at preview.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct CandidateToken {
    version: u8,
    #[serde(default, skip_serializing_if = "WebReadingMode::is_fetch")]
    reading_mode: WebReadingMode,
    #[serde(default, skip_serializing_if = "super::web_watch::login_unused")]
    use_saved_login: bool,
    url_digest: String,
    extraction: WebExtraction,
    shown: WebObservedValue,
}

pub fn preview_web_watch(
    workspace: &Path,
    request: &WebPreviewRequest,
    guard: &NetworkGuard,
    limits: &WebFetchLimits,
    reader: Option<&dyn RenderedWebReader>,
) -> Result<WebPreview, WebWatchError> {
    validate_watch_url(&request.url, guard).map_err(WebWatchError::InvalidInput)?;
    let target = checked_target(&request.target)?;
    {
        let _guard = SessionMutationGuard::acquire(workspace, &request.work.owner_session_id)?;
        let (_, session) = load_owner_session(workspace, &request.work.owner_session_id)?;
        writable_work(&session, &request.work)?;
    }
    preview_page(
        &request.url,
        &target,
        request.reading_mode,
        request.use_saved_login,
        guard,
        limits,
        reader,
    )
}

pub(super) fn checked_target(target: &WebPreviewTarget) -> Result<WebPreviewTarget, WebWatchError> {
    Ok(match target {
        WebPreviewTarget::Number {
            label,
            decimal_comma,
        } => WebPreviewTarget::Number {
            label: needle(label)?,
            decimal_comma: *decimal_comma,
        },
        WebPreviewTarget::Text { phrase, area } => WebPreviewTarget::Text {
            phrase: needle(phrase)?,
            area: match area {
                Some(area) => Some(WebTextArea {
                    label: needle(&area.label)?,
                    until: needle(&area.until)?,
                }),
                None => None,
            },
        },
    })
}

pub(super) fn preview_page(
    url: &str,
    target: &WebPreviewTarget,
    mode: WebReadingMode,
    use_saved_login: bool,
    guard: &NetworkGuard,
    limits: &WebFetchLimits,
    reader: Option<&dyn RenderedWebReader>,
) -> Result<WebPreview, WebWatchError> {
    validate_watch_url(url, guard).map_err(WebWatchError::InvalidInput)?;
    checked_login(mode, use_saved_login)?;
    let page = successful_page(url, mode, use_saved_login, guard, limits, reader)?;
    let (found, no_candidate_reason) = match candidates(&page.text, target, page.response.truncated)
    {
        CandidateSearchResult::Candidates(found) => (found, None),
        CandidateSearchResult::Unavailable(reason) => (Vec::new(), Some(reason)),
    };
    let candidates = found
        .into_iter()
        .map(|mut candidate| {
            candidate.extraction.version = mode.text_version().to_owned();
            Ok(WebCandidate {
                token: candidate_token(
                    url,
                    mode,
                    use_saved_login,
                    &candidate.extraction,
                    &candidate.value,
                )?,
                shown_text: safe_text(&candidate.shown_text, MAX_ANCHOR_CHARS),
                value: candidate.value,
                extraction: safe_extraction(&candidate.extraction),
            })
        })
        .collect::<Result<Vec<_>, WebWatchError>>()?;
    Ok(WebPreview {
        url: display_url(url),
        reading_mode: mode,
        use_saved_login,
        final_url: page.response.final_url,
        http_status: page.response.http_status,
        truncated: page.response.truncated,
        text_digest: page.response.text_digest,
        candidates,
        no_candidate_reason,
    })
}

pub(super) fn candidate_token(
    url: &str,
    mode: WebReadingMode,
    use_saved_login: bool,
    extraction: &WebExtraction,
    shown: &WebObservedValue,
) -> Result<String, WebWatchError> {
    let token = CandidateToken {
        version: 3,
        reading_mode: mode,
        use_saved_login,
        url_digest: sha256_ref(url),
        extraction: extraction.clone(),
        shown: shown.clone(),
    };
    let bytes =
        serde_json::to_vec(&token).map_err(|error| ObservationError::Durable(error.into()))?;
    Ok(base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes))
}

pub(super) fn candidate_extraction(token: &str) -> Result<WebExtraction, WebWatchError> {
    decode_token(token).map(|token| token.extraction)
}

/// A value read with a confirmed criterion, with its text as the page shows it, made safe like a
/// preview candidate's.
pub(super) struct ShownValue {
    pub value: WebObservedValue,
    pub shown_text: String,
}

pub(super) fn read_value(
    url: &str,
    extraction: &WebExtraction,
    mode: WebReadingMode,
    use_saved_login: bool,
    guard: &NetworkGuard,
    limits: &WebFetchLimits,
    reader: Option<&dyn RenderedWebReader>,
) -> Result<ShownValue, WebWatchError> {
    validate_watch_url(url, guard).map_err(WebWatchError::InvalidInput)?;
    checked_login(mode, use_saved_login)?;
    let page = successful_page(url, mode, use_saved_login, guard, limits, reader)?;
    let (value, shown) =
        extract_shown(&page.text, extraction).map_err(|failure| WebWatchError::Unreadable {
            failure: mode.extraction_failure(failure),
            http_status: Some(page.response.http_status),
            browser_reason: None,
        })?;
    Ok(ShownValue {
        value,
        shown_text: safe_text(shown, MAX_ANCHOR_CHARS),
    })
}

pub(super) struct ConfirmationCandidate {
    token: CandidateToken,
    condition: WebCondition,
    cadence_seconds: u64,
}

/// The confirming read: the value is the one the candidate showed.
pub(super) struct ConfirmedRead {
    candidate: ConfirmationCandidate,
    current: WebObservedValue,
    response: WebResponseFacts,
}

pub(super) fn confirmation_candidate(
    url: &str,
    candidate_token: &str,
    mode: WebReadingMode,
    use_saved_login: bool,
    condition: WebCondition,
    cadence_seconds: u64,
    guard: &NetworkGuard,
) -> Result<ConfirmationCandidate, WebWatchError> {
    if !(WEB_WATCH_MIN_CADENCE_SECONDS..=WEB_WATCH_MAX_CADENCE_SECONDS).contains(&cadence_seconds) {
        return Err(WebWatchError::InvalidInput(
            "cadence must be 600..86400 seconds".to_owned(),
        ));
    }
    validate_watch_url(url, guard).map_err(WebWatchError::InvalidInput)?;
    checked_login(mode, use_saved_login)?;
    let token = decode_token(candidate_token)?;
    if token.url_digest != sha256_ref(url)
        || token.reading_mode != mode
        || token.use_saved_login != use_saved_login
    {
        return Err(WebWatchError::InvalidInput(
            "candidate was previewed for a different URL, reading mode or login choice; preview again".to_owned(),
        ));
    }
    let condition = checked_condition(condition, &token.extraction.value)?;
    Ok(ConfirmationCandidate {
        token,
        condition,
        cadence_seconds,
    })
}

/// What the confirming read found: the value the candidate showed, ready to become the
/// baseline, or another value, with its text as the page shows it.
pub(super) enum ConfirmingRead {
    Same(Box<ConfirmedRead>),
    Changed(ShownValue),
}

pub(super) fn read_confirming(
    url: &str,
    candidate: ConfirmationCandidate,
    guard: &NetworkGuard,
    limits: &WebFetchLimits,
    reader: Option<&dyn RenderedWebReader>,
) -> Result<ConfirmingRead, WebWatchError> {
    let mode = candidate.token.reading_mode;
    let page = successful_page(
        url,
        mode,
        candidate.token.use_saved_login,
        guard,
        limits,
        reader,
    )?;
    let (current, shown) =
        extract_shown(&page.text, &candidate.token.extraction).map_err(|failure| {
            WebWatchError::Unreadable {
                failure: mode.extraction_failure(failure),
                http_status: Some(page.response.http_status),
                browser_reason: None,
            }
        })?;
    if current != candidate.token.shown {
        return Ok(ConfirmingRead::Changed(ShownValue {
            value: current,
            shown_text: safe_text(shown, MAX_ANCHOR_CHARS),
        }));
    }
    Ok(ConfirmingRead::Same(Box::new(ConfirmedRead {
        candidate,
        current,
        response: page.response,
    })))
}

/// Re-reads the page with the candidate's criterion; a value other than the one shown is
/// `ValueChanged`, so it never becomes a baseline unseen.
pub(super) fn read_confirmation(
    url: &str,
    candidate: ConfirmationCandidate,
    guard: &NetworkGuard,
    limits: &WebFetchLimits,
    reader: Option<&dyn RenderedWebReader>,
) -> Result<ConfirmedRead, WebWatchError> {
    let shown = candidate.token.shown.clone();
    match read_confirming(url, candidate, guard, limits, reader)? {
        ConfirmingRead::Same(confirmed) => Ok(*confirmed),
        ConfirmingRead::Changed(current) => Err(WebWatchError::ValueChanged {
            shown,
            current: current.value,
        }),
    }
}

/// Re-reads the page with the previewed criterion. Only an unchanged value becomes the
/// first baseline; the plan, baseline and next wake are stored in one session write.
pub fn confirm_web_watch(
    workspace: &Path,
    request: ConfirmWebWatchRequest,
    guard: &NetworkGuard,
    limits: &WebFetchLimits,
    reader: Option<&dyn RenderedWebReader>,
) -> Result<WebWatchConfigured, WebWatchError> {
    let candidate = confirmation_candidate(
        &request.url,
        &request.candidate_token,
        request.reading_mode,
        request.use_saved_login,
        request.condition.clone(),
        request.cadence_seconds,
        guard,
    )?;
    let owner = request.work.owner_session_id.clone();
    {
        let _guard = SessionMutationGuard::acquire(workspace, &owner)?;
        let (_, session) = load_owner_session(workspace, &owner)?;
        let (works, index, _) = writable_work(&session, &request.work)?;
        expect_revision(&works[index], request.expected_revision)?;
    }
    let read = read_confirmation(&request.url, candidate, guard, limits, reader)?;
    let _guard = SessionMutationGuard::acquire(workspace, &owner)?;
    let (mut manager, mut session) = load_owner_session(workspace, &owner)?;
    let configured = store_confirmed_watch(
        &mut session,
        &request.work,
        request.expected_revision,
        request.url,
        read,
        request.now_ms,
    )?;
    manager.save_with_fsync(&session)?;
    Ok(configured)
}

/// Stores the confirmed plan and first baseline in the loaded owner `session` without saving
/// it: the work revision goes up by one and the next wake is one cadence later.
pub(super) fn store_confirmed_watch(
    session: &mut Session,
    work: &EntrustedWorkReadRequest,
    expected_revision: u64,
    url: String,
    read: ConfirmedRead,
    now_ms: u64,
) -> Result<WebWatchConfigured, WebWatchError> {
    let ConfirmedRead {
        candidate:
            ConfirmationCandidate {
                token,
                condition,
                cadence_seconds,
            },
        current,
        response,
    } = read;
    let (mut works, index, goal) = writable_work(session, work)?;
    let work = &mut works[index];
    expect_revision(work, expected_revision)?;
    let conditions = work
        .current_conditions()
        .cloned()
        .ok_or(ObservationError::Blocked(ObservationBlock::StaleLineage))?;
    let revision = work
        .current_revision
        .checked_add(1)
        .ok_or_else(|| WebWatchError::InvalidInput("revision overflow".to_owned()))?;
    work.current_revision = revision;
    work.revisions.push(EntrustedWorkRevision {
        revision,
        conditions,
        linked_targets: work.linked_targets.clone(),
        changed_at: now_ms.to_string(),
    });
    let first_wake_at_ms = now_ms.saturating_add(cadence_seconds.saturating_mul(1000));
    let plan = WebWatchPlan {
        url,
        reading_mode: token.reading_mode,
        use_saved_login: token.use_saved_login,
        extraction: token.extraction,
        condition,
        cadence_seconds,
        first_wake_at_ms,
    };
    let entry = WebWatchEntry {
        work_id: work.work_id.clone(),
        baseline: WebEvidence {
            observation_ref: format!("confirmation:{}:{revision}", work.work_id),
            observed_at_ms: now_ms,
            condition_matches: condition_matches(&plan.condition, &current),
            value: current,
            response,
        },
        plan,
        baseline_revision: revision,
        baseline_transition_id: latest_transition_id(&goal)?,
    };
    let mut watches = web_watches(session).map_err(EntrustedWorkError::Metadata)?;
    watches.retain(|stored| stored.work_id != entry.work_id);
    watches.push(entry.clone());
    store_entrusted_works(session, &works).map_err(EntrustedWorkError::Metadata)?;
    store_web_watches(session, watches).map_err(EntrustedWorkError::Metadata)?;
    Ok(WebWatchConfigured {
        work_id: entry.work_id,
        condition_revision: revision,
        plan: plan_view(&entry.plan),
        baseline: entry.baseline,
        first_wake_at_ms,
    })
}

pub(super) fn writable_work(
    session: &Session,
    request: &EntrustedWorkReadRequest,
) -> Result<(Vec<EntrustedWork>, usize, PersistentGoal), WebWatchError> {
    let (works, index, goal) =
        bound_current_work(session, &request.requester_target, &request.work_id)?;
    let work = &works[index];
    if goal.status != PersistentGoalStatus::Active {
        return Err(ObservationError::Blocked(ObservationBlock::GoalInactive).into());
    }
    if work.consent.evidence_ref.trim().is_empty() || work.consent.recorded_at.trim().is_empty() {
        return Err(ObservationError::Blocked(ObservationBlock::ConsentMissing).into());
    }
    if work.observation_plan.is_some() {
        return Err(WebWatchError::InvalidInput(
            "work already observes a local file".to_owned(),
        ));
    }
    Ok((works, index, goal))
}

fn expect_revision(work: &EntrustedWork, expected: u64) -> Result<(), WebWatchError> {
    if work.current_revision == expected {
        Ok(())
    } else {
        Err(EntrustedWorkError::RevisionConflict {
            expected,
            actual: work.current_revision,
        }
        .into())
    }
}

fn successful_page(
    url: &str,
    mode: WebReadingMode,
    use_saved_login: bool,
    guard: &NetworkGuard,
    limits: &WebFetchLimits,
    reader: Option<&dyn RenderedWebReader>,
) -> Result<ReadPage, WebWatchError> {
    let page = super::rendered_web_reader::read_selected(
        url,
        mode,
        use_saved_login,
        guard,
        limits,
        reader,
    )
    .map_err(|error| WebWatchError::Unreadable {
        failure: error.failure,
        http_status: error.response.as_ref().map(|page| page.http_status),
        browser_reason: error.browser_reason,
    })?;
    match status_failure(page.response.http_status) {
        Some(failure) => Err(WebWatchError::Unreadable {
            failure,
            http_status: Some(page.response.http_status),
            browser_reason: None,
        }),
        None => Ok(page),
    }
}

fn checked_login(mode: WebReadingMode, use_saved_login: bool) -> Result<(), WebWatchError> {
    if mode == WebReadingMode::Fetch && use_saved_login {
        return Err(WebWatchError::InvalidInput(
            "saved login requires browser reading mode".to_owned(),
        ));
    }
    Ok(())
}

fn needle(text: &str) -> Result<String, WebWatchError> {
    let normalized = normalize_fragment(text);
    if normalized.is_empty() || normalized.chars().count() > MAX_NEEDLE_CHARS {
        return Err(WebWatchError::InvalidInput(
            "label, until and phrase must be 1..64 characters".to_owned(),
        ));
    }
    Ok(normalized)
}

pub(super) fn checked_condition(
    condition: WebCondition,
    kind: &WebValueKind,
) -> Result<WebCondition, WebWatchError> {
    let invalid = |reason: &str| WebWatchError::InvalidInput(reason.to_owned());
    match (condition, kind) {
        (WebCondition::AtMost { value }, WebValueKind::Number { .. }) => {
            parse_plain_decimal(&value)
                .map(|value| WebCondition::AtMost { value })
                .ok_or_else(|| invalid("condition value must be a plain non-negative number"))
        }
        (WebCondition::AtLeast { value }, WebValueKind::Number { .. }) => {
            parse_plain_decimal(&value)
                .map(|value| WebCondition::AtLeast { value })
                .ok_or_else(|| invalid("condition value must be a plain non-negative number"))
        }
        (
            condition @ (WebCondition::TextPresent | WebCondition::TextAbsent),
            WebValueKind::Text { .. },
        ) => Ok(condition),
        _ => Err(invalid("condition does not fit the watched value")),
    }
}

fn decode_token(token: &str) -> Result<CandidateToken, WebWatchError> {
    let invalid = || WebWatchError::InvalidInput("candidate token is invalid".to_owned());
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(token.trim())
        .map_err(|_| invalid())?;
    let token: CandidateToken = serde_json::from_slice(&bytes).map_err(|_| invalid())?;
    let extraction = &token.extraction;
    let bounded = |text: &str| !text.trim().is_empty() && text.chars().count() <= MAX_ANCHOR_CHARS;
    let value_ok = match (&extraction.value, &token.shown) {
        (
            WebValueKind::Number {
                decimal_separator,
                thousands_separator,
                unit_before,
                unit_after,
            },
            WebObservedValue::Number { value },
        ) => {
            matches!(decimal_separator, '.' | ',')
                && !extraction.prefix.ends_with(UNSUPPORTED_NUMBER_LEAD)
                && !continues_number(&extraction.suffix)
                && thousands_separator.is_none_or(|separator| {
                    separator != *decimal_separator && ALL_SEPARATORS.contains(&separator)
                })
                && [unit_before, unit_after].into_iter().flatten().all(|unit| {
                    (1..=8).contains(&unit.chars().count()) && unit.chars().all(is_unit_char)
                })
                && parse_plain_decimal(value).as_deref() == Some(value.as_str())
        }
        (WebValueKind::Text { phrase }, WebObservedValue::Text { .. }) => {
            needle(phrase).is_ok_and(|normalized| normalized == *phrase)
        }
        _ => false,
    };
    let version_matches = match token.version {
        1 => token.reading_mode == WebReadingMode::Fetch && !token.use_saved_login,
        2 => token.reading_mode == WebReadingMode::Browser && !token.use_saved_login,
        3 => checked_login(token.reading_mode, token.use_saved_login).is_ok(),
        _ => false,
    };
    if !version_matches
        || extraction.version != token.reading_mode.text_version()
        || !bounded(&extraction.prefix)
        || !bounded(&extraction.suffix)
        || extraction.occurrence == 0
        || extraction.occurrence > extraction.match_count
        || !value_ok
    {
        return Err(invalid());
    }
    Ok(token)
}
