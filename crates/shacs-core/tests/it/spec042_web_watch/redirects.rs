use super::support::*;
use shacs_core::runtime::*;
use shacs_core::tools::{UreqWebClient, WebClient};
use std::time::Duration;

/// Where the old shared resolver sent `ftp://203.0.113.10/x`: a page on the same site.
const MISREAD_PATH: &str = "/ftp://203.0.113.10/x";

#[test]
fn a_redirect_to_another_scheme_is_an_http_error_for_the_watcher() -> TestResult {
    // Given
    let fixture = Fixture::new()?;
    fixture.watch_price("12,900", "11000")?;
    fixture.server.set(
        "/item",
        Page::status(302).header("Location", "ftp://203.0.113.10/x"),
    )?;
    fixture
        .server
        .set(MISREAD_PATH, Page::html(&price_page("12,900")))?;

    // When
    fixture.tick(fixture.next_due()?)?;

    // Then
    assert!(matches!(
        fixture.latest()?.result,
        WebCheckResult::Failed {
            failure: WebCheckFailure::HttpError,
            ..
        }
    ));
    Ok(())
}

#[test]
fn a_redirect_to_another_scheme_is_refused_by_the_fetch_tool_client() -> TestResult {
    // Given
    let fixture = Fixture::new()?;
    fixture.server.set(
        "/fetch",
        Page::status(302).header("Location", "ftp://203.0.113.10/x"),
    )?;
    fixture
        .server
        .set(MISREAD_PATH, Page::html("<p>not the target</p>"))?;

    // When
    let fetched = UreqWebClient.get(
        &fixture.server.url("/fetch"),
        "spec042-test",
        Duration::from_secs(1),
        5,
        &fixture.guard,
    );

    // Then
    assert!(fetched.is_err());
    Ok(())
}
