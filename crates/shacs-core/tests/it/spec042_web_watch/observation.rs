use super::support::*;
use shacs_core::runtime::*;

fn number_value(value: &str) -> WebObservedValue {
    WebObservedValue::Number {
        value: value.to_owned(),
    }
}

#[test]
fn confirmed_value_is_the_baseline_and_only_a_changed_condition_outcome_is_a_candidate(
) -> TestResult {
    // Given: a page with an injected instruction and a price hidden in an attribute, watched
    // for <= 11000 through a URL with a query.
    let fixture = Fixture::new()?;
    let url = fixture.server.url("/item?session=secret-token#reviews");
    fixture
        .server
        .set("/item", Page::html(&price_page("12,900")))?;
    let preview = fixture.preview(&url, number("판매가"))?;
    let candidate = preview.candidates.first().ok_or("no candidate")?;
    let configured = fixture.confirm(&url, &candidate.token, at_most("11000"), 1, T0)?;

    // When: the same value, an unrelated change and a change that meets the condition.
    let mut verdicts = Vec::new();
    for price in ["12,900", "12,500", "10,900"] {
        fixture
            .server
            .set("/item", Page::html(&price_page(price)))?;
        fixture.tick(fixture.next_due()?)?;
        let decision = fixture.latest()?.decision;
        verdicts.push((decision.verdict, decision.reason));
    }

    // Then
    assert_eq!(preview.candidates.len(), 1);
    assert_eq!(candidate.shown_text, "\u{20a9}12,900");
    assert_eq!(configured.baseline.value, number_value("12900"));
    assert!(!configured.baseline.condition_matches);
    assert_eq!(configured.plan.condition, at_most("11000"));
    assert_eq!(configured.plan.url, fixture.server.url("/item"));
    assert_eq!(
        verdicts,
        vec![
            (
                ObservationVerdict::Suppress,
                WebDecisionReason::EquivalentValue
            ),
            (
                ObservationVerdict::Suppress,
                WebDecisionReason::SameConditionOutcome
            ),
            (
                ObservationVerdict::NotifyCandidate,
                WebDecisionReason::ConditionOutcomeChanged
            ),
        ]
    );
    let status = fixture.status()?;
    let decision = status.current_decision.clone().ok_or("no decision")?;
    assert_eq!(
        decision.previous.map(|evidence| evidence.value),
        Some(number_value("12500"))
    );
    let current = decision.current.ok_or("no current evidence")?;
    assert_eq!(current.value, number_value("10900"));
    assert!(current.condition_matches);
    assert_eq!(status.last_success_at_ms, Some(current.observed_at_ms));
    assert_eq!(fixture.next_due()?, current.observed_at_ms + CADENCE_MS);
    let output = serde_json::to_string(&status)?;
    assert!(!output.contains("secret-token") && !output.contains("reviews"));
    Ok(())
}

#[test]
fn a_value_that_changed_before_confirmation_is_not_stored() -> TestResult {
    // Given
    let fixture = Fixture::new()?;
    let url = fixture.server.url("/item");
    fixture
        .server
        .set("/item", Page::html(&price_page("12,900")))?;
    let preview = fixture.preview(&url, number("판매가"))?;
    let token = preview
        .candidates
        .first()
        .ok_or("no candidate")?
        .token
        .clone();

    // When
    fixture
        .server
        .set("/item", Page::html(&price_page("12,800")))?;
    let changed = fixture.confirm(&url, &token, at_most("11000"), 1, T0);

    // Then
    assert!(matches!(changed, Err(WebWatchError::ValueChanged { .. })));
    let status = fixture.status()?;
    assert!(status.plan.is_none());
    assert_eq!(status.condition_revision, 1);
    assert_eq!(status.next_wake, ObservationNextWake::NotConfigured);
    Ok(())
}

#[test]
fn a_phrase_condition_needs_its_region_and_reports_the_phrase_leaving() -> TestResult {
    // Given: notify when "품절" (sold out) is no longer shown.
    let fixture = Fixture::new()?;
    let url = fixture.server.url("/item");
    fixture
        .server
        .set("/item", Page::html("<p>재고: 품절 배송: 내일</p>"))?;
    let preview = fixture.preview(
        &url,
        WebPreviewTarget::Text {
            phrase: "품절".to_owned(),
            area: None,
        },
    )?;
    let token = &preview.candidates.first().ok_or("no candidate")?.token;
    let configured = fixture.confirm(&url, token, WebCondition::TextAbsent, 1, T0)?;

    // When: the region disappears, then comes back with only the phrase removed.
    fixture.server.set("/item", Page::html("<p>준비 중</p>"))?;
    fixture.tick(fixture.next_due()?)?;
    let missing = fixture.latest()?;
    fixture
        .server
        .set("/item", Page::html("<p>재고: 배송: 내일</p>"))?;
    fixture.tick(fixture.next_due()?)?;
    let removed = fixture.latest()?;

    // Then
    assert!(!configured.baseline.condition_matches);
    assert!(matches!(
        missing.result,
        WebCheckResult::Failed {
            failure: WebCheckFailure::StaticContentUnavailable,
            ..
        }
    ));
    assert_eq!(missing.decision.verdict, ObservationVerdict::Unknown);
    assert_eq!(
        removed.decision.verdict,
        ObservationVerdict::NotifyCandidate
    );
    assert_eq!(
        removed.decision.current.map(|evidence| evidence.value),
        Some(WebObservedValue::Text { present: false })
    );
    Ok(())
}

#[test]
fn a_phrase_area_is_watched_before_the_phrase_first_appears() -> TestResult {
    // Given: notify when "재입고" (restocked) appears between "재고:" and "배송:".
    let fixture = Fixture::new()?;
    let url = fixture.server.url("/item");
    fixture
        .server
        .set("/item", Page::html("<p>재고: 품절 배송: 내일</p>"))?;
    let preview = fixture.preview(
        &url,
        WebPreviewTarget::Text {
            phrase: "재입고".to_owned(),
            area: Some(WebTextArea {
                label: "재고:".to_owned(),
                until: "배송:".to_owned(),
            }),
        },
    )?;
    let candidate = preview.candidates.first().ok_or("no candidate")?;
    let configured = fixture.confirm(&url, &candidate.token, WebCondition::TextPresent, 1, T0)?;

    // When
    fixture
        .server
        .set("/item", Page::html("<p>재고: 재입고 배송: 내일</p>"))?;
    fixture.tick(fixture.next_due()?)?;

    // Then
    assert_eq!(preview.candidates.len(), 1);
    assert_eq!(candidate.shown_text, "품절");
    assert_eq!(
        configured.baseline.value,
        WebObservedValue::Text { present: false }
    );
    assert!(!configured.baseline.condition_matches);
    let decision = fixture.latest()?.decision;
    assert_eq!(decision.verdict, ObservationVerdict::NotifyCandidate);
    assert_eq!(
        decision.current.map(|evidence| evidence.value),
        Some(WebObservedValue::Text { present: true })
    );
    Ok(())
}

#[test]
fn the_declared_charset_is_followed_for_html_and_xhtml() -> TestResult {
    // Given: the same EUC-KR page, once as HTML and once as XHTML.
    let fixture = Fixture::new()?;
    let url = fixture.server.url("/item");
    let (body, _, _) =
        encoding_rs::EUC_KR.encode("<html><body><p>판매가 12,900원 배송비 무료</p></body></html>");
    let mut offered = Vec::new();
    for content_type in [
        "text/html; charset=euc-kr",
        "application/xhtml+xml; charset=euc-kr",
    ] {
        fixture
            .server
            .set("/item", Page::encoded(content_type, body.to_vec()))?;

        // When
        let preview = fixture.preview(&url, number("판매가"))?;
        offered.push(
            preview
                .candidates
                .into_iter()
                .map(|candidate| (candidate.shown_text, candidate.value))
                .collect::<Vec<_>>(),
        );
    }

    // Then
    let read = vec![("12,900원".to_owned(), number_value("12900"))];
    assert_eq!(offered, vec![read.clone(), read]);
    Ok(())
}
