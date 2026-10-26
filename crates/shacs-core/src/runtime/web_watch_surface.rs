use std::path::Path;

use base64::Engine;
use serde::{Deserialize, Serialize};
use shacs_security::NetworkGuard;
use shacs_session::{Session, SessionMutationGuard};

use super::entrusted_work_access::{bound_current_work, latest_transition_id};
use super::entrusted_work_store::store_entrusted_works;
use super::entrusted_work_surface::load_owner_session;
use super::web_watch_decision::condition_matches;
use super::web_watch_extract::{candidates, continues_number, extract, UNSUPPORTED_NUMBER_LEAD};
use super::web_watch_fetch::{
    display_url, read_page, sha256_ref, status_failure, validate_watch_url, ReadPage,
    WebFetchLimits,
};
use super::web_watch_number::{is_unit_char, parse_plain_decimal, ALL_SEPARATORS};
use super::web_watch_read::plan_view;
use super::web_watch_redact::{safe_extraction, safe_text, MAX_ANCHOR_CHARS};
use super::web_watch_store::{store_web_watches, web_watches};
use super::web_watch_text::normalize_fragment;
use super::{
    ConfirmWebWatchRequest, EntrustedWork, EntrustedWorkError, EntrustedWorkReadRequest,
    EntrustedWorkRevision, ObservationBlock, ObservationError, PersistentGoal,
    PersistentGoalStatus, WebCandidate, WebCondition, WebEvidence, WebExtraction, WebObservedValue,
    WebPreview, WebPreviewRequest, WebPreviewTarget, WebTextArea, WebValueKind, WebWatchConfigured,
    WebWatchEntry, WebWatchError, WebWatchPlan, WEB_TEXT_VERSION, WEB_WATCH_MAX_CADENCE_SECONDS,
    WEB_WATCH_MIN_CADENCE_SECONDS,
};

const MAX_NEEDLE_CHARS: usize = 64;

/// Opaque preview candidate handed back on confirmation; nothing is stored at preview.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct CandidateToken {
    version: u8,
    url_digest: String,
    extraction: WebExtraction,
    shown: WebObservedValue,
}

pub fn preview_web_watch(
    workspace: &Path,
    request: &WebPreviewRequest,
    guard: &NetworkGuard,
    limits: &WebFetchLimits,
) -> Result<WebPreview, WebWatchError> {
    validate_watch_url(&request.url, guard).map_err(WebWatchError::InvalidInput)?;
    let target = match &request.target {
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
    };
    {
        let _guard = SessionMutationGuard::acquire(workspace, &request.work.owner_session_id)?;
        let (_, session) = load_owner_session(workspace, &request.work.owner_session_id)?;
        writable_work(&session, &request.work)?;
    }
    let page = successful_page(&request.url, guard, limits)?;
    let url_digest = sha256_ref(&request.url);
    let candidates = candidates(&page.text, &target)
        .into_iter()
        .map(|candidate| {
            let token = CandidateToken {
                version: 1,
                url_digest: url_digest.clone(),
                extraction: candidate.extraction.clone(),
                shown: candidate.value.clone(),
            };
            let bytes = serde_json::to_vec(&token)
                .map_err(|error| ObservationError::Durable(error.into()))?;
            Ok(WebCandidate {
                token: base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes),
                shown_text: safe_text(&candidate.shown_text, MAX_ANCHOR_CHARS),
                value: candidate.value,
                extraction: safe_extraction(&candidate.extraction),
            })
        })
        .collect::<Result<Vec<_>, WebWatchError>>()?;
    Ok(WebPreview {
        url: display_url(&request.url),
        final_url: page.response.final_url,
        http_status: page.response.http_status,
        truncated: page.response.truncated,
        text_digest: page.response.text_digest,
        candidates,
    })
}

/// Re-reads the page with the previewed criterion. Only an unchanged value becomes the
/// first baseline; the plan, baseline and next wake are stored in one session write.
pub fn confirm_web_watch(
    workspace: &Path,
    request: ConfirmWebWatchRequest,
    guard: &NetworkGuard,
    limits: &WebFetchLimits,
) -> Result<WebWatchConfigured, WebWatchError> {
    if !(WEB_WATCH_MIN_CADENCE_SECONDS..=WEB_WATCH_MAX_CADENCE_SECONDS)
        .contains(&request.cadence_seconds)
    {
        return Err(WebWatchError::InvalidInput(
            "cadence must be 600..86400 seconds".to_owned(),
        ));
    }
    validate_watch_url(&request.url, guard).map_err(WebWatchError::InvalidInput)?;
    let token = decode_token(&request.candidate_token)?;
    if token.url_digest != sha256_ref(&request.url) {
        return Err(WebWatchError::InvalidInput(
            "candidate was previewed for a different URL".to_owned(),
        ));
    }
    let condition = checked_condition(request.condition.clone(), &token.extraction.value)?;
    let owner = request.work.owner_session_id.clone();
    {
        let _guard = SessionMutationGuard::acquire(workspace, &owner)?;
        let (_, session) = load_owner_session(workspace, &owner)?;
        let (works, index, _) = writable_work(&session, &request.work)?;
        expect_revision(&works[index], request.expected_revision)?;
    }
    let page = successful_page(&request.url, guard, limits)?;
    let current =
        extract(&page.text, &token.extraction).map_err(|failure| WebWatchError::Unreadable {
            failure,
            http_status: Some(page.response.http_status),
        })?;
    if current != token.shown {
        return Err(WebWatchError::ValueChanged {
            shown: token.shown,
            current,
        });
    }
    let _guard = SessionMutationGuard::acquire(workspace, &owner)?;
    let (mut manager, mut session) = load_owner_session(workspace, &owner)?;
    let (mut works, index, goal) = writable_work(&session, &request.work)?;
    let work = &mut works[index];
    expect_revision(work, request.expected_revision)?;
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
        changed_at: request.now_ms.to_string(),
    });
    let first_wake_at_ms = request
        .now_ms
        .saturating_add(request.cadence_seconds.saturating_mul(1000));
    let plan = WebWatchPlan {
        url: request.url,
        extraction: token.extraction,
        condition,
        cadence_seconds: request.cadence_seconds,
        first_wake_at_ms,
    };
    let entry = WebWatchEntry {
        work_id: work.work_id.clone(),
        baseline: WebEvidence {
            observation_ref: format!("confirmation:{}:{revision}", work.work_id),
            observed_at_ms: request.now_ms,
            condition_matches: condition_matches(&plan.condition, &current),
            value: current,
            response: page.response,
        },
        plan,
        baseline_revision: revision,
        baseline_transition_id: latest_transition_id(&goal)?,
    };
    let mut watches = web_watches(&session).map_err(EntrustedWorkError::Metadata)?;
    watches.retain(|stored| stored.work_id != entry.work_id);
    watches.push(entry.clone());
    store_entrusted_works(&mut session, &works).map_err(EntrustedWorkError::Metadata)?;
    store_web_watches(&mut session, watches).map_err(EntrustedWorkError::Metadata)?;
    manager.save_with_fsync(&session)?;
    Ok(WebWatchConfigured {
        work_id: entry.work_id,
        condition_revision: revision,
        plan: plan_view(&entry.plan),
        baseline: entry.baseline,
        first_wake_at_ms,
    })
}

fn writable_work(
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
    guard: &NetworkGuard,
    limits: &WebFetchLimits,
) -> Result<ReadPage, WebWatchError> {
    let page = read_page(url, guard, limits).map_err(|failure| WebWatchError::Unreadable {
        failure,
        http_status: None,
    })?;
    match status_failure(page.response.http_status) {
        Some(failure) => Err(WebWatchError::Unreadable {
            failure,
            http_status: Some(page.response.http_status),
        }),
        None => Ok(page),
    }
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

fn checked_condition(
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
                && thousands_separator.map_or(true, |separator| {
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
    if token.version != 1
        || extraction.version != WEB_TEXT_VERSION
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
