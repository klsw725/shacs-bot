use super::support::*;
use shacs_core::runtime::*;

fn price_line(price: &str) -> String {
    format!("<p>판매가 {price} 배송비 무료</p>")
}

#[test]
fn failed_checks_are_classified_keep_the_schedule_and_form_one_interval() -> TestResult {
    // Given
    let fixture = Fixture::new()?;
    fixture.watch_price("12,900", "11000")?;
    let large = format!(
        "<p>{}</p>{}",
        "x".repeat(70_000),
        price_line("&#8361;12,900")
    );
    let cases = [
        (Page::status(404), WebCheckFailure::HttpError, CADENCE_MS),
        (
            Page::status(429).header("Retry-After", "3600"),
            WebCheckFailure::RateLimited,
            3_600_000,
        ),
        (
            Page::status(503),
            WebCheckFailure::HttpError,
            2 * CADENCE_MS,
        ),
        (
            Page::html("<p>준비 중</p>"),
            WebCheckFailure::StaticContentUnavailable,
            CADENCE_MS,
        ),
        (
            Page::html(&format!("{0}{0}", price_line("&#8361;12,900"))),
            WebCheckFailure::AmbiguousValue,
            CADENCE_MS,
        ),
        (
            Page::html(&price_line("&#8361;10,000~12,000")),
            WebCheckFailure::AmbiguousValue,
            CADENCE_MS,
        ),
        (
            Page::html(&price_line("$10,900")),
            WebCheckFailure::FormatMismatch,
            CADENCE_MS,
        ),
        (
            Page::html(&large),
            WebCheckFailure::StaticContentUnavailable,
            CADENCE_MS,
        ),
        (
            Page {
                hang: true,
                ..Page::status(200)
            },
            WebCheckFailure::NetworkError,
            CADENCE_MS,
        ),
        (
            Page::status(302).header("Location", "http://192.168.0.1/"),
            WebCheckFailure::GuardBlocked,
            CADENCE_MS,
        ),
        (
            Page::status(302).header("Location", "/item"),
            WebCheckFailure::HttpError,
            CADENCE_MS,
        ),
    ];

    // When / Then: each failure is recorded as unknown and the next check stays scheduled.
    let mut first_failure_at = None;
    for (page, failure, delay) in cases.iter().cloned() {
        fixture.server.set("/item", page)?;
        let due = fixture.next_due()?;
        fixture.tick(due)?;
        let record = fixture.latest()?;
        let WebCheckResult::Failed {
            failure: recorded, ..
        } = record.result
        else {
            return Err(format!("expected {failure:?}, got {:?}", record.result).into());
        };
        assert_eq!(recorded, failure);
        assert_eq!(record.decision.verdict, ObservationVerdict::Unknown);
        assert_eq!(record.next_wake_at_ms, Some(due + delay), "{failure:?}");
        first_failure_at.get_or_insert(due);
    }
    let truncated = fixture
        .status()?
        .history
        .iter()
        .find_map(|record| match &record.result {
            WebCheckResult::Failed {
                response: Some(response),
                ..
            } if response.truncated => Some(response.http_status),
            _ => None,
        });
    assert_eq!(truncated, Some(200));

    // When: the page recovers with a value that now meets the condition.
    fixture
        .server
        .set("/item", Page::html(&price_line("&#8361;10,900")))?;
    fixture.tick(fixture.next_due()?)?;

    // Then: the recovery is compared with the last value before the failures.
    let status = fixture.status()?;
    let decision = status.current_decision.ok_or("no decision")?;
    assert_eq!(decision.verdict, ObservationVerdict::NotifyCandidate);
    assert!(status.failure_interval.is_none());
    let interval = status.failure_intervals.last().ok_or("no interval")?;
    assert_eq!(status.failure_intervals.len(), 1);
    assert_eq!(interval.failure_count, cases.len() as u32);
    assert_eq!(Some(interval.started_at_ms), first_failure_at);
    assert_eq!(
        interval.closed.map(|closed| closed.reason),
        Some(WebIntervalCloseReason::Recovered)
    );
    Ok(())
}

#[test]
fn a_redirect_chain_shares_one_time_limit() -> TestResult {
    // Given: three hops of 0.4 s each under the fixture's 1 s limit.
    let fixture = Fixture::new()?;
    fixture.watch_price("12,900", "11000")?;
    let slow = |page: Page| Page {
        delay: std::time::Duration::from_millis(400),
        ..page
    };
    fixture
        .server
        .set("/item", slow(Page::status(302).header("Location", "/r1")))?;
    fixture
        .server
        .set("/r1", slow(Page::status(302).header("Location", "/r2")))?;
    fixture
        .server
        .set("/r2", slow(Page::html(&price_page("12,900"))))?;

    // When
    fixture.tick(fixture.next_due()?)?;

    // Then
    assert!(matches!(
        fixture.latest()?.result,
        WebCheckResult::Failed {
            failure: WebCheckFailure::NetworkError,
            ..
        }
    ));
    Ok(())
}

#[test]
fn a_stored_url_with_userinfo_is_refused_before_any_request() -> TestResult {
    // Given: the stored plan was changed to carry credentials after confirmation.
    let fixture = Fixture::new()?;
    fixture.watch_price("12,900", "11000")?;
    let mut sessions =
        shacs_session::SessionManager::open_existing(&fixture.workspace)?.ok_or("no sessions")?;
    let mut session = sessions.load_existing("cli:owner").ok_or("no session")?;
    let url = fixture
        .server
        .url("/item")
        .replace("http://", "http://user:pw@");
    let plan = session
        .metadata
        .get_mut(WEB_WATCH_METADATA_KEY)
        .and_then(|store| store.pointer_mut("/watches/0/plan/url"))
        .ok_or("no stored plan")?;
    *plan = serde_json::json!(url);
    sessions.save_with_fsync(&session)?;

    // When
    fixture.tick(fixture.next_due()?)?;

    // Then
    assert!(matches!(
        fixture.latest()?.result,
        WebCheckResult::Failed {
            failure: WebCheckFailure::GuardBlocked,
            response: None,
            ..
        }
    ));
    Ok(())
}

#[test]
fn a_price_that_becomes_a_range_or_gets_a_second_price_after_its_unit_is_ambiguous() -> TestResult {
    // Given: `원` stands apart from the price, so it is the confirmed suffix.
    let fixture = Fixture::new()?;
    let url = fixture.server.url("/item");
    let single = Page::html("<p>가격 12,000 원 배송</p>");
    let range = Page::html("<p>가격 10,000 원 ~ 13,000 원 배송</p>");
    let struck = Page::html("<p>가격 <del>13,000 원</del> 10,000 원 배송</p>");
    fixture.server.set("/item", single.clone())?;
    let preview = fixture.preview(&url, number("가격"))?;
    let token = preview
        .candidates
        .first()
        .ok_or("no candidate")?
        .token
        .clone();

    // When: the range appears at confirmation; after confirming, a range and then a
    // struck-out old price appear at the checks.
    fixture.server.set("/item", range.clone())?;
    let at_confirmation = fixture.confirm(&url, &token, at_most("11000"), 1, T0);
    fixture.server.set("/item", single)?;
    fixture.confirm(&url, &token, at_most("11000"), 1, T0)?;
    let mut checks = Vec::new();
    for page in [range, struck] {
        fixture.server.set("/item", page)?;
        fixture.tick(fixture.next_due()?)?;
        let record = fixture.latest()?;
        let failure = match record.result {
            WebCheckResult::Failed { failure, .. } => Some(failure),
            _ => None,
        };
        checks.push((failure, record.decision.verdict));
    }

    // Then
    assert!(matches!(
        at_confirmation,
        Err(WebWatchError::Unreadable {
            failure: WebCheckFailure::AmbiguousValue,
            ..
        })
    ));
    let ambiguous = (
        Some(WebCheckFailure::AmbiguousValue),
        ObservationVerdict::Unknown,
    );
    assert_eq!(checks, vec![ambiguous, ambiguous]);
    Ok(())
}

#[test]
fn proxy_settings_in_the_environment_are_not_used() -> TestResult {
    // Given: a proxy that would receive the request and its credentials if one were used.
    let proxy = std::net::TcpListener::bind("127.0.0.1:0")?;
    proxy.set_nonblocking(true)?;
    let proxy_url = format!("http://watch:secret@{}", proxy.local_addr()?);

    // When: a preview runs in a child process with every proxy variable set.
    let output = std::process::Command::new(std::env::current_exe()?)
        .args([
            "--exact",
            "spec042_web_watch::failures::proxy_probe",
            "--ignored",
            "--nocapture",
        ])
        .env("ALL_PROXY", &proxy_url)
        .env("HTTPS_PROXY", &proxy_url)
        .env("HTTP_PROXY", &proxy_url)
        .env_remove("NO_PROXY")
        .env_remove("no_proxy")
        .output()?;

    // Then: the page was read directly and the proxy saw no connection.
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success() && stdout.contains("1 passed"),
        "{stdout}\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(matches!(
        proxy.accept(),
        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock
    ));
    Ok(())
}

#[test]
#[ignore = "subprocess probe invoked by proxy_settings_in_the_environment_are_not_used"]
fn proxy_probe() -> TestResult {
    let fixture = Fixture::new()?;
    let url = fixture.server.url("/item");
    fixture
        .server
        .set("/item", Page::html(&price_page("12,900")))?;

    let preview = fixture.preview(&url, number("판매가"))?;

    assert_eq!(preview.candidates.len(), 1);
    Ok(())
}

#[test]
fn signed_fractional_or_range_numbers_are_not_offered_as_one_value() -> TestResult {
    // Given: a readable number right after the signed ones must not be taken in their place.
    let fixture = Fixture::new()?;
    let url = fixture.server.url("/numbers");
    fixture.server.set(
        "/numbers",
        Page::html(
            "<p>변동 -5 원</p><p>할인 -&#8361;5 원</p><p>비율 .5 배</p><p>기온 &#8722;3 도</p>\
             <p>배송비 3,000원 무료</p><p>판매가 &#8361;10,000~12,000 배송비</p>\
             <p>정가 &#8361;10,000 ~ &#8361;12,000 배송비</p><p>회원가 10,000-12,000원 배송비</p>\
             <p>특가 10,000 원 ~ 12,000 원 배송비</p><p>증감 +5 원</p>\
             <p>예상가 10,000 ~ -12,000 배송비</p><p>가격대 $10 to $12 배송비</p>",
        ),
    )?;

    // When
    let mut offered = Vec::new();
    for label in [
        "변동",
        "할인",
        "비율",
        "기온",
        "판매가",
        "정가",
        "회원가",
        "특가",
        "증감",
        "예상가",
        "가격대",
    ] {
        offered.extend(fixture.preview(&url, number(label))?.candidates);
    }

    // Then
    assert!(offered.is_empty(), "{:?}", offered);
    Ok(())
}

#[test]
fn configuration_rejects_internal_addresses_userinfo_and_other_schemes() -> TestResult {
    // Given: the product guard, which never allows private addresses.
    let fixture = Fixture::new()?;
    fixture
        .server
        .set("/item", Page::html(&price_page("12,900")))?;
    fixture.server.set(
        "/hop",
        Page::status(302).header("Location", "http://192.168.0.1/"),
    )?;
    let production = NetworkGuard::default();
    let urls = [
        fixture.server.url("/item"),
        "http://10.0.0.1/".to_owned(),
        "http://[::1]/".to_owned(),
        "http://localhost/".to_owned(),
        "http://user:pass@203.0.113.10/".to_owned(),
        "ftp://203.0.113.10/file".to_owned(),
    ];

    // When
    let rejected = urls.iter().map(|url| {
        let request = WebPreviewRequest {
            work: fixture.request(),
            url: url.clone(),
            target: number("판매가"),
        };
        preview_web_watch(&fixture.workspace, &request, &production, &fixture.limits)
    });
    let redirected = fixture.preview(&fixture.server.url("/hop"), number("판매가"));

    // Then
    for (url, result) in urls.iter().zip(rejected) {
        assert!(
            matches!(result, Err(WebWatchError::InvalidInput(_))),
            "{url}"
        );
    }
    assert!(matches!(
        redirected,
        Err(WebWatchError::Unreadable {
            failure: WebCheckFailure::GuardBlocked,
            ..
        })
    ));
    Ok(())
}
