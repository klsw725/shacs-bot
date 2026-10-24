use super::fixture::NoRequestFixture;
use super::support::candidate;
use shacs_core::generated_media::{
    RemoteOutputDecision, RemoteOutputEvaluationContext, RemoteOutputPolicy, RemoteReferenceExpiry,
    RemoteRejectionReason, UreqGuardedRemoteTransport,
};
use shacs_security::NetworkGuard;
use std::error::Error;
use std::time::{Duration, SystemTime};

#[test]
fn explicit_reject_precedes_guard_dns_and_transport() -> Result<(), Box<dyn Error>> {
    // Given
    let fixture = NoRequestFixture::start()?;
    let transport =
        UreqGuardedRemoteTransport::new(NetworkGuard::default(), Duration::from_millis(200));
    let context = RemoteOutputEvaluationContext::new(
        None,
        &transport,
        SystemTime::UNIX_EPOCH + Duration::from_secs(100),
    );

    // When
    let decision = RemoteOutputPolicy::reject().evaluate(candidate(&fixture.url()), context);
    let request_seen = fixture.finish()?;

    // Then
    assert!(matches!(
        decision,
        RemoteOutputDecision::Rejected(rejection)
            if rejection.reason() == RemoteRejectionReason::PolicyRejected
    ));
    assert!(!request_seen);
    Ok(())
}

#[test]
fn reference_expiry_must_be_future_in_unix_second_precision() {
    // Given
    let now = SystemTime::UNIX_EPOCH + Duration::from_millis(100_100);
    let same_second = SystemTime::UNIX_EPOCH + Duration::from_millis(100_900);

    // When
    let expiry = RemoteReferenceExpiry::new(same_second, now);

    // Then
    assert!(expiry.is_err());
}

#[test]
fn delayed_reference_emission_revalidates_expiry_and_provider_fact() -> Result<(), Box<dyn Error>> {
    // Given
    let created_at = SystemTime::UNIX_EPOCH + Duration::from_secs(100);
    let expires_at = SystemTime::UNIX_EPOCH + Duration::from_secs(200);
    let expiry = RemoteReferenceExpiry::new(expires_at, created_at)?;
    let guard = NetworkGuard::default();
    let transport = UreqGuardedRemoteTransport::new(guard.clone(), Duration::from_millis(200));
    let context = RemoteOutputEvaluationContext::new(Some(&guard), &transport, expires_at);

    // When
    let decision = RemoteOutputPolicy::reference(expiry).evaluate(
        candidate("https://example.com/output?token=secret"),
        context,
    );

    // Then
    assert!(matches!(
        decision,
        RemoteOutputDecision::Rejected(rejection)
            if rejection.reason() == RemoteRejectionReason::ReferenceExpired
    ));
    Ok(())
}

#[test]
fn live_reference_contains_safe_provider_domain_and_future_expiry() -> Result<(), Box<dyn Error>> {
    // Given
    let now = SystemTime::UNIX_EPOCH + Duration::from_secs(100);
    let expiry =
        RemoteReferenceExpiry::new(SystemTime::UNIX_EPOCH + Duration::from_secs(200), now)?;
    let guard = NetworkGuard::default();
    let transport = UreqGuardedRemoteTransport::new(guard.clone(), Duration::from_millis(200));
    let context = RemoteOutputEvaluationContext::new(Some(&guard), &transport, now);

    // When
    let decision = RemoteOutputPolicy::reference(expiry).evaluate(
        candidate("https://Example.COM./private/path?token=secret"),
        context,
    );
    let rendered = format!("{decision:?}");

    // Then
    let RemoteOutputDecision::Reference(reference) = decision else {
        return Err("expected reference".into());
    };
    assert_eq!(reference.provider_id().as_str(), "openrouter");
    assert_eq!(reference.domain(), "example.com");
    assert_eq!(reference.expires_at_unix_seconds(), 200);
    assert!(!rendered.contains("private/path"));
    assert!(!rendered.contains("token=secret"));
    Ok(())
}
