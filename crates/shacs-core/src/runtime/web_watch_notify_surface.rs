use std::path::Path;

use shacs_session::{Session, SessionMutationGuard};

use super::entrusted_work_access::bound_current_work;
use super::entrusted_work_surface::load_owner_session;
use super::web_watch_store::{store_web_notify_settings, web_notify_settings, web_watches};
use super::{
    ConfigureWebNotifyRequest, EntrustedWorkError, WebNotifySettings, WebNotifySettingsView,
    WebWatchError, WEB_NOTIFY_MAX_THRESHOLD_SECONDS,
};

const MAX_ADDRESS_CHARS: usize = 254;

/// Sets or clears the notification target and failure threshold of a confirmed web watch.
/// The new settings revision leaves the observation revision and the baseline as they are.
pub fn configure_web_watch_notifications(
    workspace: &Path,
    request: ConfigureWebNotifyRequest,
) -> Result<WebNotifySettingsView, WebWatchError> {
    let owner = &request.work.owner_session_id;
    let _guard = SessionMutationGuard::acquire(workspace, owner)?;
    let (mut manager, mut session) = load_owner_session(workspace, owner)?;
    let (works, index, _) = bound_current_work(
        &session,
        &request.work.requester_target,
        &request.work.work_id,
    )?;
    let work_id = works[index].work_id.clone();
    let (settings, cadence_seconds) = next_notify_settings(
        &session,
        work_id,
        request.target,
        request.failure_threshold_seconds,
        request.email_ready,
        request.now_ms,
    )?;
    store_web_notify_settings(&mut session, settings.clone())
        .map_err(EntrustedWorkError::Metadata)?;
    manager.save_with_fsync(&session)?;
    Ok(settings_view(&settings, cadence_seconds))
}

/// The next notification settings revision of `work_id` in `session`, checked as above, and
/// the cadence of its watch.
pub(super) fn next_notify_settings(
    session: &Session,
    work_id: String,
    target: Option<String>,
    failure_threshold_seconds: Option<u64>,
    email_ready: bool,
    now_ms: u64,
) -> Result<(WebNotifySettings, u64), WebWatchError> {
    let invalid = |reason: String| WebWatchError::InvalidInput(reason);
    let cadence_seconds = web_watches(session)
        .map_err(EntrustedWorkError::Metadata)?
        .into_iter()
        .find(|watch| watch.work_id == work_id)
        .map(|watch| watch.plan.cadence_seconds)
        .ok_or_else(|| invalid("confirm a web watch before setting notifications".to_owned()))?;
    if let Some(target) = &target {
        if !target.strip_prefix("email:").is_some_and(valid_address) {
            return Err(invalid(
                "notification target must be email:<address>".to_owned(),
            ));
        }
        email_sendable(email_ready)?;
    }
    let floor = cadence_seconds.saturating_mul(2);
    if failure_threshold_seconds
        .is_some_and(|seconds| !(floor..=WEB_NOTIFY_MAX_THRESHOLD_SECONDS).contains(&seconds))
    {
        return Err(invalid(format!(
            "failure threshold must be {floor}..{WEB_NOTIFY_MAX_THRESHOLD_SECONDS} seconds"
        )));
    }
    let revision = web_notify_settings(session, &work_id)
        .map_err(EntrustedWorkError::Metadata)?
        .map_or(0, |settings| settings.revision)
        .checked_add(1)
        .ok_or_else(|| invalid("notification revision overflow".to_owned()))?;
    let settings = WebNotifySettings {
        work_id,
        revision,
        target,
        failure_threshold_seconds,
        changed_at_ms: now_ms,
    };
    Ok((settings, cadence_seconds))
}

pub(super) fn email_sendable(email_ready: bool) -> Result<(), WebWatchError> {
    if email_ready {
        Ok(())
    } else {
        Err(WebWatchError::InvalidInput(
            "the Email channel cannot send: enable it with consentGranted and smtp settings"
                .to_owned(),
        ))
    }
}

pub(super) fn settings_view(
    settings: &WebNotifySettings,
    cadence_seconds: u64,
) -> WebNotifySettingsView {
    WebNotifySettingsView {
        revision: settings.revision,
        target: settings.target.clone(),
        failure_threshold_seconds: settings.failure_threshold_seconds,
        effective_threshold_seconds: settings.threshold_seconds(cadence_seconds),
        changed_at_ms: settings.changed_at_ms,
    }
}

pub(super) fn valid_address(address: &str) -> bool {
    let Some((local, domain)) = address.split_once('@') else {
        return false;
    };
    !local.is_empty()
        && !domain.contains('@')
        && domain.contains('.')
        && !domain.starts_with('.')
        && !domain.ends_with('.')
        && address.chars().count() <= MAX_ADDRESS_CHARS
        && !address.chars().any(|character| {
            character.is_whitespace()
                || character.is_control()
                || matches!(character, '<' | '>' | ',' | ';' | '"')
        })
}
