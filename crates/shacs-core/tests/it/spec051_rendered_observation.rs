use super::spec042_web_watch::support::{self, Fixture, Page, TestResult, CADENCE_MS, T0};
use base64::Engine;
use serde_json::Value;
use sha2::{Digest, Sha256};
use shacs_core::runtime::{
    self, BrowserUnavailableReason, ConfirmWebWatchRequest, ObservationVerdict, RenderedWebFailure,
    RenderedWebPage, RenderedWebReader, RenderedWebRequest, WebCheckFailure, WebCheckResult,
    WebCondition, WebExtraction, WebNotifyKind, WebObservedValue, WebPreviewRequest,
    WebPreviewTarget, WebReadingMode, WebTextArea, WebWatchConfigured, WebWatchError,
    RENDERED_TEXT_VERSION, WEB_WATCH_METADATA_KEY,
};
use shacs_session::SessionManager;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;

const TEXT: &str = "판매가: ₩12,900 배송: 내일";
const TARGET: &str = "email:user@example.test";

struct Reader {
    result: Mutex<Result<RenderedWebPage, RenderedWebFailure>>,
    reads: AtomicUsize,
    pause: Mutex<Option<(mpsc::Sender<()>, mpsc::Receiver<()>)>>,
}

impl Reader {
    fn new() -> Self {
        Self {
            result: Mutex::new(Ok(page(TEXT))),
            reads: AtomicUsize::new(0),
            pause: Mutex::new(None),
        }
    }

    fn text(&self, text: &str) {
        *self.result.lock().expect("rendered result lock poisoned") = Ok(page(text));
    }

    fn busy(&self) {
        *self.result.lock().expect("rendered result lock poisoned") = Err(RenderedWebFailure {
            failure: WebCheckFailure::BrowserBusy,
            browser_reason: None,
            response: None,
        });
    }
}

impl RenderedWebReader for Reader {
    fn enabled(&self) -> bool {
        true
    }
    fn saved_login_enabled(&self) -> bool {
        false
    }
    fn read(
        &self,
        _request: &RenderedWebRequest<'_>,
    ) -> Result<RenderedWebPage, RenderedWebFailure> {
        self.reads.fetch_add(1, Ordering::SeqCst);
        let result = self
            .result
            .lock()
            .expect("rendered result lock poisoned")
            .clone();
        let pause = self.pause.lock().expect("read pause lock poisoned").take();
        if let Some((entered, release)) = pause {
            entered.send(()).expect("read entry receiver gone");
            release
                .recv_timeout(Duration::from_secs(5))
                .expect("read release deadline");
        }
        result
    }
}

fn page(text: &str) -> RenderedWebPage {
    RenderedWebPage {
        text: text.to_owned(),
        final_url: "https://example.com/item?private-query#fragment".to_owned(),
        http_status: 200,
        retry_after: None,
        truncated: false,
    }
}

fn fixture() -> Result<(Fixture, Arc<Reader>), Box<dyn std::error::Error>> {
    let mut fixture = Fixture::new()?;
    let reader = Arc::new(Reader::new());
    fixture.reading_mode = WebReadingMode::Browser;
    fixture.rendered_reader = Some(reader.clone());
    Ok((fixture, reader))
}

fn watch(fixture: &Fixture) -> Result<WebWatchConfigured, Box<dyn std::error::Error>> {
    let url = fixture.server.url("/item");
    let preview = fixture.preview(&url, support::number("판매가"))?;
    Ok(fixture.confirm(
        &url,
        &preview.candidates[0].token,
        support::at_most("11000"),
        1,
        T0,
    )?)
}

#[test]
fn reading_mode_is_bound_to_preview_plan_and_wake() -> TestResult {
    let (mut fixture, reader) = fixture()?;
    let url = fixture.server.url("/item");
    let preview = fixture.preview(&url, support::number("판매가"))?;
    let token = &preview.candidates[0].token;
    let decoded: Value =
        serde_json::from_slice(&base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(token)?)?;
    assert_eq!(decoded["version"], 3);
    assert_eq!(decoded["reading_mode"], "browser");
    let reads = reader.reads.load(Ordering::SeqCst);
    fixture.reading_mode = WebReadingMode::Fetch;
    assert!(matches!(
        fixture.confirm(&url, token, support::at_most("11000"), 1, T0),
        Err(WebWatchError::InvalidInput(_))
    ));
    assert_eq!(reader.reads.load(Ordering::SeqCst), reads);
    fixture.reading_mode = WebReadingMode::Browser;
    for (field, value) in [
        ("version", serde_json::json!(1)),
        ("reading_mode", serde_json::json!("unknown")),
    ] {
        let mut invalid = decoded.clone();
        invalid[field] = value;
        let invalid =
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(serde_json::to_vec(&invalid)?);
        assert!(matches!(
            fixture.confirm(&url, &invalid, support::at_most("11000"), 1, T0),
            Err(WebWatchError::InvalidInput(_))
        ));
    }
    assert_eq!(reader.reads.load(Ordering::SeqCst), reads);
    let browser = fixture.confirm(&url, token, support::at_most("11000"), 1, T0)?;
    fixture.tick(browser.first_wake_at_ms)?;
    let old = fixture.latest()?.wake;
    let old_due = fixture.next_due()?;

    fixture.reading_mode = WebReadingMode::Fetch;
    fixture
        .server
        .set("/item", Page::html("판매가: ₩10,900 배송: 내일"))?;
    let fresh = fixture.preview(&url, support::number("판매가"))?;
    let fetch = fixture.confirm(
        &url,
        &fresh.candidates[0].token,
        support::at_most("11000"),
        2,
        old_due - 1,
    )?;
    fixture.tick(old_due)?;
    assert_eq!(
        fixture.status()?.history.len(),
        1,
        "the stale browser wake must not read"
    );
    assert_eq!(fetch.condition_revision, browser.condition_revision + 1);
    assert_eq!(
        fetch.baseline.value,
        WebObservedValue::Number {
            value: "10900".to_owned()
        }
    );
    assert_eq!(reader.reads.load(Ordering::SeqCst), reads + 2);
    fixture.tick(fetch.first_wake_at_ms)?;
    assert_ne!(fixture.latest()?.wake.plan_digest, old.plan_digest);
    assert_eq!(
        fixture.latest()?.wake.condition_revision,
        fetch.condition_revision
    );
    assert!(fixture.deliver(fetch.first_wake_at_ms)?.is_empty());

    let unavailable = runtime::preview_web_watch(
        &fixture.workspace,
        &WebPreviewRequest {
            work: fixture.request(),
            url,
            reading_mode: WebReadingMode::Browser,
            use_saved_login: false,
            target: support::number("판매가"),
        },
        &fixture.guard,
        &fixture.limits,
        None,
    );
    assert!(matches!(
        unavailable,
        Err(WebWatchError::Unreadable {
            failure: WebCheckFailure::BrowserUnavailable,
            browser_reason: Some(BrowserUnavailableReason::Disabled),
            ..
        })
    ));
    Ok(())
}

#[test]
fn legacy_fetch_records_keep_their_digest_and_schedule() -> TestResult {
    let fixture = Fixture::new()?;
    let configured = fixture.watch_price("12,900", "11000")?;
    fixture.notify(Some(TARGET), None)?;
    let mut manager = SessionManager::new(&fixture.workspace)?;
    let mut session = manager.get_or_create("cli:owner");
    // Existing storage has neither a reading mode nor a changed JSON field order.
    let stored = &mut session.metadata[WEB_WATCH_METADATA_KEY];
    let entries = stored["watches"]
        .as_array_mut()
        .ok_or("watch entries are not an array")?;
    let plan = &mut entries[0]["plan"];
    plan.as_object_mut().ok_or("plan")?.remove("reading_mode");
    let url = plan["url"].as_str().ok_or("url")?;
    let extraction: WebExtraction = serde_json::from_value(plan["extraction"].clone())?;
    let legacy_bytes = format!(
        "{{\"url\":{},\"extraction\":{},\"condition\":{},\"cadence_seconds\":600,\"first_wake_at_ms\":{}}}",
        serde_json::to_string(url)?, serde_json::to_string(&extraction)?,
        serde_json::to_string(&configured.plan.condition)?, configured.first_wake_at_ms,
    );
    let expected = format!("sha256:{:x}", Sha256::digest(legacy_bytes.as_bytes()));
    manager.save_with_fsync(&session)?;
    fixture.tick(configured.first_wake_at_ms)?;
    assert_eq!(fixture.latest()?.wake.plan_digest, expected);
    assert_eq!(
        fixture.next_due()?,
        configured.first_wake_at_ms + CADENCE_MS
    );
    fixture
        .server
        .set("/item", Page::html(&support::price_page("10,900")))?;
    let due = fixture.next_due()?;
    fixture.tick(due)?;
    let mails = fixture.deliver(due)?;
    assert_eq!(mails.len(), 1);
    assert!(mails[0]
        .content
        .contains("static page text only (normalized_static_text_v1)"));
    assert!(mails[0].content.contains("no JavaScript, login or cookies"));
    assert!(!mails[0].content.contains(RENDERED_TEXT_VERSION));
    Ok(())
}

#[test]
fn rendered_value_drives_existing_condition_decisions() -> TestResult {
    let (fixture, reader) = fixture()?;
    watch(&fixture)?;
    let mut verdicts = Vec::new();
    for price in ["12,900", "12,500", "10,900"] {
        reader.text(&format!(
            "Cafe\u{301}\n 판매가:   ₩{price}\t배송: 내일 < &amp; >"
        ));
        fixture.tick(fixture.next_due()?)?;
        let record = fixture.latest()?;
        assert_eq!(record.text_version, RENDERED_TEXT_VERSION);
        verdicts.push(record.decision.verdict);
    }
    assert_eq!(
        verdicts,
        [
            ObservationVerdict::Suppress,
            ObservationVerdict::Suppress,
            ObservationVerdict::NotifyCandidate,
        ]
    );
    let record = fixture.latest()?;
    let WebCheckResult::Observed { response, value } = record.result else {
        panic!("not observed")
    };
    assert_eq!(
        value,
        WebObservedValue::Number {
            value: "10900".to_owned()
        }
    );
    let text = "Café 판매가: ₩10,900 배송: 내일 < &amp; >";
    assert_eq!(
        response.text_digest,
        format!("sha256:{:x}", Sha256::digest(text.as_bytes()))
    );
    assert_eq!(
        fixture.status()?.last_success_at_ms,
        Some(record.observed_at_ms)
    );
    assert_eq!(fixture.next_due()?, record.observed_at_ms + CADENCE_MS);
    assert_eq!(response.final_url, "https://example.com/item");

    let (phrase_fixture, phrase_reader) = self::fixture()?;
    phrase_reader.text("상태: < &amp; > 끝");
    let url = phrase_fixture.server.url("/item");
    let preview = phrase_fixture.preview(
        &url,
        WebPreviewTarget::Text {
            phrase: "< &amp; >".to_owned(),
            area: Some(WebTextArea {
                label: "상태:".to_owned(),
                until: "끝".to_owned(),
            }),
        },
    )?;
    phrase_fixture.confirm(
        &url,
        &preview.candidates[0].token,
        WebCondition::TextAbsent,
        1,
        T0,
    )?;
    phrase_reader.text("상태: & 끝");
    phrase_fixture.tick(phrase_fixture.next_due()?)?;
    assert_eq!(
        phrase_fixture.latest()?.decision.verdict,
        ObservationVerdict::NotifyCandidate
    );
    Ok(())
}

#[test]
fn rendered_http_failures_keep_retry_after_and_baseline() -> TestResult {
    let (fixture, reader) = fixture()?;
    let confirmed = watch(&fixture)?;
    for status in [429, 503] {
        let mut response = page("");
        response.http_status = status;
        response.retry_after = Some("3600".to_owned());
        *reader.result.lock().unwrap() = Ok(response);
        let due = fixture.next_due()?;
        fixture.tick(due)?;
        let record = fixture.latest()?;
        let WebCheckResult::Failed {
            failure,
            response,
            retry_after_ms,
            browser_reason,
        } = record.result
        else {
            panic!("HTTP error became a value")
        };
        assert_eq!(
            failure,
            if status == 429 {
                WebCheckFailure::RateLimited
            } else {
                WebCheckFailure::HttpError
            }
        );
        assert_eq!(retry_after_ms, Some(3_600_000));
        assert_eq!(browser_reason, None);
        let response = response.ok_or("missing HTTP facts")?;
        assert_eq!(response.http_status, status);
        assert!(response.text_digest.is_empty());
        assert_eq!(fixture.next_due()?, due + 3_600_000);
        assert_eq!(fixture.status()?.baseline, Some(confirmed.baseline.clone()));
    }
    assert_eq!(
        fixture
            .status()?
            .failure_interval
            .ok_or("failure interval")?
            .failure_count,
        2
    );
    Ok(())
}

#[test]
fn missing_rendered_anchors_are_not_a_new_baseline() -> TestResult {
    let (fixture, reader) = fixture()?;
    let confirmed = watch(&fixture)?;
    let session = SessionManager::new(&fixture.workspace)?.get_or_create("cli:owner");
    let extraction: WebExtraction = serde_json::from_value(
        session.metadata[WEB_WATCH_METADATA_KEY]["watches"][0]["plan"]["extraction"].clone(),
    )?;
    for (text, truncated, expected) in [
        (
            "waiting for data".to_owned(),
            false,
            WebCheckFailure::RenderedContentUnavailable,
        ),
        (
            extraction.prefix.clone(),
            true,
            WebCheckFailure::RenderedContentUnavailable,
        ),
        (
            format!("{TEXT} {TEXT}"),
            false,
            WebCheckFailure::AmbiguousValue,
        ),
        (
            format!("{}none{}", extraction.prefix, extraction.suffix),
            false,
            WebCheckFailure::FormatMismatch,
        ),
    ] {
        let mut response = page(&text);
        response.truncated = truncated;
        *reader.result.lock().unwrap() = Ok(response);
        fixture.tick(fixture.next_due()?)?;
        let record = fixture.latest()?;
        assert!(
            matches!(record.result, WebCheckResult::Failed { failure, .. } if failure == expected),
            "expected {expected:?}, got {:?}",
            record.result
        );
        assert_eq!(record.decision.verdict, ObservationVerdict::Unknown);
        assert!(record.decision.current.is_none());
        assert_eq!(fixture.status()?.baseline, Some(confirmed.baseline.clone()));
    }
    let mut complete = page(TEXT);
    complete.truncated = true;
    *reader.result.lock().unwrap() = Ok(complete);
    fixture.tick(fixture.next_due()?)?;
    assert!(
        matches!(fixture.latest()?.result, WebCheckResult::Observed { response, .. } if response.truncated)
    );
    Ok(())
}

#[test]
fn busy_window_notifies_once_then_recovers() -> TestResult {
    let (fixture, reader) = fixture()?;
    watch(&fixture)?;
    fixture.notify(Some(TARGET), Some(1200))?;
    reader.busy();
    let mut mails = Vec::new();
    for _ in 0..4 {
        let due = fixture.next_due()?;
        fixture.tick(due)?;
        fixture.tick(due)?; // New dispatchers replay the same wake without a second observation.
        mails.extend(fixture.deliver(due)?);
    }
    assert_eq!(fixture.status()?.history.len(), 4);
    assert_eq!(mails.len(), 1);
    assert!(mails[0].content.contains("browser_busy"));
    assert_eq!(
        fixture.status()?.notifications[0].kind,
        WebNotifyKind::Failure
    );
    reader.text(TEXT);
    let due = fixture.next_due()?;
    fixture.tick(due)?;
    mails.extend(fixture.deliver(due)?);
    assert_eq!(mails.len(), 2);
    assert_eq!(
        fixture
            .status()?
            .notifications
            .iter()
            .map(|notice| notice.kind)
            .collect::<Vec<_>>(),
        [WebNotifyKind::Failure, WebNotifyKind::Recovery]
    );
    assert!(fixture.deliver(due)?.is_empty());

    // A mode change while an old read is in flight retires the result, not
    // just its queued wake. The reader does not hold the session mutation lock.
    let due = fixture.next_due()?;
    reader.text("판매가: ₩10,900 배송: 내일");
    let (entered, observed) = mpsc::channel();
    let (release, released) = mpsc::channel();
    *reader.pause.lock().unwrap() = Some((entered, released));
    let history = fixture.status()?.history.len();
    std::thread::scope(|scope| -> TestResult {
        let check = scope.spawn(|| fixture.tick(due).map_err(|error| error.to_string()));
        observed.recv_timeout(Duration::from_secs(5))?;
        fixture.server.set("/item", Page::html(TEXT))?;
        let url = fixture.server.url("/item");
        let preview = runtime::preview_web_watch(
            &fixture.workspace,
            &WebPreviewRequest {
                work: fixture.request(),
                url: url.clone(),
                reading_mode: WebReadingMode::Fetch,
                use_saved_login: false,
                target: support::number("판매가"),
            },
            &fixture.guard,
            &fixture.limits,
            None,
        )?;
        runtime::confirm_web_watch(
            &fixture.workspace,
            ConfirmWebWatchRequest {
                work: fixture.request(),
                expected_revision: 2,
                url,
                reading_mode: WebReadingMode::Fetch,
                use_saved_login: false,
                candidate_token: preview.candidates[0].token.clone(),
                condition: support::at_most("11000"),
                cadence_seconds: 600,
                now_ms: due + 1,
            },
            &fixture.guard,
            &fixture.limits,
            None,
        )?;
        release.send(())?;
        check
            .join()
            .unwrap()
            .map_err(|error| -> Box<dyn std::error::Error> { error.into() })?;
        Ok(())
    })?;
    assert_eq!(fixture.status()?.history.len(), history);
    assert_eq!(fixture.next_due()?, due + 1 + CADENCE_MS);
    assert!(fixture.deliver(due + 1)?.is_empty());
    Ok(())
}

#[test]
fn notifications_keep_the_confirmed_reading_limits() -> TestResult {
    let (fixture, reader) = fixture()?;
    watch(&fixture)?;
    fixture.notify(Some(TARGET), Some(1200))?;
    let mut mails = Vec::new();
    reader.text("판매가: ₩10,900 배송: 내일");
    let due = fixture.next_due()?;
    fixture.tick(due)?;
    mails.extend(fixture.deliver(due)?);
    reader.busy();
    for _ in 0..3 {
        let due = fixture.next_due()?;
        fixture.tick(due)?;
        mails.extend(fixture.deliver(due)?);
    }
    reader.text("판매가: ₩10,900 배송: 내일");
    let due = fixture.next_due()?;
    fixture.tick(due)?;
    mails.extend(fixture.deliver(due)?);
    let late = fixture.next_due()? + 2 * CADENCE_MS;
    fixture.tick(late)?;
    mails.extend(fixture.deliver(late)?);
    let deadline = chrono::DateTime::from_timestamp_millis(i64::try_from(late + 2 * CADENCE_MS)?)
        .ok_or("deadline")?
        .to_rfc3339();
    fixture.deadline(Some(&deadline), Some(600), false, 0, late)?;
    fixture.remind(late + CADENCE_MS)?;
    mails.extend(fixture.deliver(late + CADENCE_MS)?);
    assert_eq!(
        fixture
            .status()?
            .notifications
            .iter()
            .map(|notice| notice.kind)
            .collect::<Vec<_>>(),
        [
            WebNotifyKind::Change,
            WebNotifyKind::Failure,
            WebNotifyKind::Recovery,
            WebNotifyKind::Gap,
            WebNotifyKind::Deadline
        ]
    );
    assert_eq!(mails.len(), 5);
    for mail in mails {
        // The consuming AgentLoop has no browser reader or current watch config.
        assert!(mail.content.contains("rendered_text_v1"));
        assert!(mail.content.contains("no saved login profile"));
        assert!(mail.content.contains("500 ms"));
        assert!(!mail.content.contains("private-query") && !mail.content.contains("fragment"));
        assert!(!mail.content.contains("no JavaScript, login or cookies"));
    }
    Ok(())
}
