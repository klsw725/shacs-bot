use chrono::{DateTime, SecondsFormat};

use super::web_watch::value_text;
use super::{
    WebCheckFailure, WebCondition, WebEvidence, WebNotifyContent, WebNotifyIntent, WebReadingMode,
};

const LIMITS: &str = "Limits: static page text only (normalized_static_text_v1); no JavaScript, \
login or cookies; the value is not verified elsewhere. No external action performed. Transport \
send is not a remote receipt.";
const RENDERED_LIMITS: &str = "Limits: rendered page text (rendered_text_v1); no saved login \
profile; read-only observation, no input or capture. Load and 500 ms network idle in the \
initial page session are bounded, and do not prove the final value; text is limited to 1 MiB \
and may be truncated. Temporary cookies and cache may exist until cleanup. The value is not \
verified elsewhere. Transport send is not a remote receipt.";
const SAVED_LOGIN_LIMITS: &str = "Limits: rendered page text (rendered_text_v1); saved login \
profile; read-only observation, no input or capture. Load and 500 ms network idle in the \
initial page session are bounded, and do not prove the final value; text is limited to 1 MiB \
and may be truncated. Confirmed values and evidence are stored and sent to the confirmed \
email target; whole page text, screenshots, cookies and credentials are not. Cookies and \
site data remain in browser/profile/ under secret-file protection. HTTP cache is outside \
that profile in the temporary generation folder, outside secret-file protection; normal \
cleanup deletes it, forced termination may leave it. The value is not verified elsewhere. \
Transport send is not a remote receipt.";

/// The notification text, built only from the stored intent.
pub(super) fn render_web_notification(intent: &WebNotifyIntent) -> String {
    let page = format!("Page: {} ({})", intent.url, intent.url_digest);
    let body = match &intent.content {
        WebNotifyContent::Change {
            condition,
            phrase,
            previous,
            current,
            recovered,
        } => {
            let recovered = recovered.as_ref().map_or_else(String::new, |interval| {
                format!(
                    "\nRecovered: checks had failed since {} ({} failed checks) before this check.",
                    time(interval.started_at_ms),
                    interval.failure_count
                )
            });
            format!(
                "Web page change notification\n{page}\nCondition: {} (revision {})\n{}\n{}{recovered}",
                condition_text(condition, phrase.as_deref()),
                intent.wake.condition_revision,
                evidence("Previous", previous),
                evidence("New", current),
            )
        }
        WebNotifyContent::Failure {
            failure,
            started_at_ms,
            failure_count,
            failing_for_ms,
            last_success_at_ms,
            next_check_at_ms,
            held_until_next_check,
        } => format!(
            "Web page check failure notification\n{page}\nFailing since: {} ({failure_count} failed \
             checks, {} minutes without assistant downtime)\nLast failure: {}\nLast successful \
             check: {}\nNext check: {}{}",
            time(*started_at_ms),
            failing_for_ms / 60_000,
            failure_text(*failure),
            optional_time(*last_success_at_ms),
            optional_time(*next_check_at_ms),
            if *held_until_next_check {
                "\nSent now: the site asked to wait (or the rate-limit backoff applies), so no \
                 check can run before the next check, which is past the failure threshold."
            } else {
                ""
            },
        ),
        WebNotifyContent::Recovery {
            started_at_ms,
            failure_count,
            recovered_at_ms,
        } => format!(
            "Web page check recovered\n{page}\nChecks failed from {} ({failure_count} failed \
             checks) and succeeded again at {}.",
            time(*started_at_ms),
            time(*recovered_at_ms),
        ),
        WebNotifyContent::Gap {
            from_ms,
            to_ms,
            next_check_at_ms,
        } => format!(
            "Assistant downtime notification\n{page}\nScheduled checks did not run from {} to {} \
             ({} minutes) because the assistant was not running; it checked again at {}.\nNext \
             check: {}",
            time(*from_ms),
            time(*to_ms),
            to_ms.saturating_sub(*from_ms) / 60_000,
            time(*to_ms),
            optional_time(*next_check_at_ms),
        ),
        WebNotifyContent::Deadline {
            goal,
            deadline_at_ms,
            deadline_text,
            remind_at_ms,
            reached_at_ms,
            condition,
            phrase,
            latest,
        } => format!(
            "Decision deadline reminder\n{page}\nGoal: {}\nDeadline: {} (set as {deadline_text})\n\
             Time left: {} at this reminder (planned for {})\nCondition: {} (revision {})\n{}",
            goal.split_whitespace().collect::<Vec<_>>().join(" "),
            time(*deadline_at_ms),
            duration_text(deadline_at_ms.saturating_sub(*reached_at_ms)),
            time(*remind_at_ms),
            condition_text(condition, phrase.as_deref()),
            intent.wake.condition_revision,
            latest.as_ref().map_or_else(
                || "Latest value: none read yet".to_owned(),
                |latest| evidence("Latest value", latest)
            ),
        ),
    };
    let limits = match intent.reading_mode {
        WebReadingMode::Fetch => LIMITS,
        WebReadingMode::Browser if intent.use_saved_login => SAVED_LOGIN_LIMITS,
        WebReadingMode::Browser => RENDERED_LIMITS,
    };
    let login_help = if intent.use_saved_login
        && matches!(
            intent.content,
            WebNotifyContent::Failure {
                failure: WebCheckFailure::RenderedContentUnavailable,
                ..
            }
        ) {
        "\n로그인이 풀렸을 수 있습니다. 감시 확인이 끝난 뒤 /browser takeover로 브라우저를 넘겨받아 다시 로그인하세요. 로그인 뒤 돌려주고 /browser close로 닫으면 다음 예정 확인에서 다시 읽습니다. 비밀번호나 인증 코드는 대화로 보내지 마세요. slash 명령이 안 되면 \"브라우저 넘겨줘\", \"브라우저 닫아\"라고 요청하세요."
    } else {
        ""
    };
    format!("{body}{login_help}\n{limits}")
}

pub(super) fn duration_text(ms: u64) -> String {
    let minutes = ms / 60_000;
    format!(
        "{}d {}h {}m",
        minutes / 1_440,
        minutes % 1_440 / 60,
        minutes % 60
    )
}

fn evidence(label: &str, evidence: &WebEvidence) -> String {
    format!(
        "{label}: {} (condition met: {}) read at {}; text {}, HTTP {}",
        value_text(&evidence.value),
        evidence.condition_matches,
        time(evidence.observed_at_ms),
        evidence.response.text_digest,
        evidence.response.http_status,
    )
}

pub(super) fn condition_text(condition: &WebCondition, phrase: Option<&str>) -> String {
    let phrase = phrase.unwrap_or_default();
    match condition {
        WebCondition::AtMost { value } => format!("value <= {value}"),
        WebCondition::AtLeast { value } => format!("value >= {value}"),
        WebCondition::TextPresent => format!("phrase \"{phrase}\" present"),
        WebCondition::TextAbsent => format!("phrase \"{phrase}\" absent"),
    }
}

pub(super) fn failure_text(failure: WebCheckFailure) -> &'static str {
    match failure {
        WebCheckFailure::NetworkError => "network_error",
        WebCheckFailure::GuardBlocked => "guard_blocked",
        WebCheckFailure::HttpError => "http_error",
        WebCheckFailure::RateLimited => "rate_limited",
        WebCheckFailure::StaticContentUnavailable => "static_content_unavailable",
        WebCheckFailure::AmbiguousValue => "ambiguous_value",
        WebCheckFailure::FormatMismatch => "format_mismatch",
        WebCheckFailure::BrowserUnavailable => "browser_unavailable",
        WebCheckFailure::BrowserBusy => "browser_busy",
        WebCheckFailure::RenderedContentUnavailable => "rendered_content_unavailable",
    }
}

pub(super) fn time(ms: u64) -> String {
    let utc = i64::try_from(ms)
        .ok()
        .and_then(DateTime::from_timestamp_millis)
        .map(|at| at.to_rfc3339_opts(SecondsFormat::Secs, true))
        .unwrap_or_default();
    format!("{utc} ({ms} ms)")
}

pub(super) fn optional_time(ms: Option<u64>) -> String {
    ms.map_or_else(|| "none".to_owned(), time)
}
