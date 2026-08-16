#[path = "spec034_remote_output/adversarial.rs"]
mod adversarial;
#[path = "spec034_remote_output/decision_contract.rs"]
mod decision_contract;
#[path = "spec034_remote_output/fixture.rs"]
mod fixture;
#[path = "spec034_remote_output/support.rs"]
mod support;

use shacs_core::generated_media::{
    RemoteOutputDecision, RemoteOutputEvaluationContext, RemoteOutputPolicy, RemoteReferenceExpiry,
    RemoteRejectionReason, UreqGuardedRemoteTransport,
};
use shacs_security::NetworkGuard;
use std::error::Error;
use std::time::{Duration, SystemTime};
use support::{candidate, png};

fn evaluation<'a>(
    guard: Option<&'a NetworkGuard>,
    transport: &'a UreqGuardedRemoteTransport,
) -> RemoteOutputEvaluationContext<'a> {
    RemoteOutputEvaluationContext::new(guard, transport, SystemTime::UNIX_EPOCH)
}

#[test]
fn loopback_fixture_proves_peer_binding_and_credential_absence() -> Result<(), Box<dyn Error>> {
    // Given
    let fixture = fixture::LoopbackFixture::start(png())?;
    let guard = NetworkGuard::with_ssrf_whitelist(["127.0.0.1/32"]);
    let transport = shacs_core::generated_media::UreqGuardedRemoteTransport::new(
        guard.clone(),
        Duration::from_secs(2),
    );

    // When
    let decision = RemoteOutputPolicy::download(1024, 0).evaluate(
        candidate(&fixture.url()),
        evaluation(Some(&guard), &transport),
    );
    let request = fixture.finish()?;

    // Then
    let RemoteOutputDecision::ReadyToPersist(ready) = decision else {
        return Err("loopback fixture was not accepted".into());
    };
    assert!(ready.evidence().connected_peer().is_loopback());
    assert_eq!(ready.evidence().mime_type(), "image/png");
    assert_eq!(ready.evidence().byte_len(), png().len());
    let request = request.to_ascii_lowercase();
    for forbidden in [
        "authorization:",
        "cookie:",
        "proxy-authorization:",
        "x-openrouter-",
    ] {
        assert!(
            !request.contains(forbidden),
            "credential header leaked: {request}"
        );
    }
    Ok(())
}

#[test]
fn valid_guarded_provider_candidate_becomes_ready_to_persist() -> Result<(), Box<dyn Error>> {
    // Given
    let fixture = fixture::LoopbackFixture::start(png())?;
    let guard = NetworkGuard::with_ssrf_whitelist(["127.0.0.1/32"]);
    let transport = UreqGuardedRemoteTransport::new(guard.clone(), Duration::from_secs(2));

    // When
    let decision = RemoteOutputPolicy::download(1024, 3).evaluate(
        candidate(&fixture.url()),
        evaluation(Some(&guard), &transport),
    );
    fixture.finish()?;

    // Then
    assert!(matches!(decision, RemoteOutputDecision::ReadyToPersist(_)));
    Ok(())
}

#[test]
fn explicit_future_reference_contains_no_raw_url() -> Result<(), Box<dyn Error>> {
    // Given
    let now = SystemTime::UNIX_EPOCH + Duration::from_secs(100);
    let expiry = RemoteReferenceExpiry::new(now + Duration::from_secs(60), now)?;
    let policy = RemoteOutputPolicy::reference(expiry);
    let guard = NetworkGuard::default();
    let transport = UreqGuardedRemoteTransport::new(guard.clone(), Duration::from_secs(2));
    let candidate = candidate("https://Example.COM./signed/path?token=secret");

    // When
    let context = RemoteOutputEvaluationContext::new(Some(&guard), &transport, now);
    let decision = policy.evaluate(candidate, context);
    let rendered = format!("{decision:?}");

    // Then
    let RemoteOutputDecision::Reference(reference) = decision else {
        return Err("expected safe remote reference".into());
    };
    assert_eq!(reference.domain(), "example.com");
    assert_eq!(reference.expires_at_unix_seconds(), 160);
    assert!(rendered.contains("example.com"));
    assert!(!rendered.contains("signed/path"));
    assert!(!rendered.contains("token=secret"));
    Ok(())
}

#[test]
fn duplicate_location_and_content_type_headers_fail_closed() -> Result<(), Box<dyn Error>> {
    // Given
    let guard = NetworkGuard::with_ssrf_whitelist(["127.0.0.1/32"]);
    let transport = shacs_core::generated_media::UreqGuardedRemoteTransport::new(
        guard.clone(),
        Duration::from_secs(2),
    );
    let duplicate_content_type = fixture::RawResponseFixture::start(
        b"HTTP/1.1 200 OK\r\nContent-Type: image/png\r\nContent-Type: image/png\r\nContent-Length: 15\r\nConnection: close\r\n\r\n\x89PNG\r\n\x1a\nfixture".to_vec(),
    )?;

    // When
    let content_type_decision = RemoteOutputPolicy::download(1024, 1).evaluate(
        candidate(&duplicate_content_type.url()),
        evaluation(Some(&guard), &transport),
    );
    duplicate_content_type.finish()?;

    // Then
    assert!(matches!(
        content_type_decision,
        RemoteOutputDecision::Rejected(rejection)
            if rejection.reason() == RemoteRejectionReason::AmbiguousHeaders
    ));

    // Given
    let duplicate_location = fixture::RawResponseFixture::start(
        b"HTTP/1.1 302 Found\r\nLocation: /one\r\nLocation: /two\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_vec(),
    )?;

    // When
    let location_decision = RemoteOutputPolicy::download(1024, 1).evaluate(
        candidate(&duplicate_location.url()),
        evaluation(Some(&guard), &transport),
    );
    duplicate_location.finish()?;

    // Then
    assert!(matches!(
        location_decision,
        RemoteOutputDecision::Rejected(rejection)
            if rejection.reason() == RemoteRejectionReason::AmbiguousHeaders
    ));
    Ok(())
}

#[test]
fn malformed_location_and_content_type_headers_fail_closed() -> Result<(), Box<dyn Error>> {
    let guard = NetworkGuard::with_ssrf_whitelist(["127.0.0.1/32"]);
    let transport = shacs_core::generated_media::UreqGuardedRemoteTransport::new(
        guard.clone(),
        Duration::from_secs(2),
    );
    for response in [
        b"HTTP/1.1 200 OK\r\nContent-Type: image/\xff\r\nContent-Length: 15\r\nConnection: close\r\n\r\n\x89PNG\r\n\x1a\nfixture".to_vec(),
        b"HTTP/1.1 302 Found\r\nLocation: /\xff\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_vec(),
    ] {
        // Given
        let fixture = fixture::RawResponseFixture::start(response)?;

        // When
        let decision = RemoteOutputPolicy::download(1024, 1).evaluate(
            candidate(&fixture.url()),
            evaluation(Some(&guard), &transport),
        );
        fixture.finish()?;

        // Then
        assert!(matches!(decision, RemoteOutputDecision::Rejected(_)));
    }
    Ok(())
}

#[test]
fn redirect_headers_are_followed_without_reading_declared_body() -> Result<(), Box<dyn Error>> {
    // Given
    let fixture = fixture::UnreadRedirectBodyFixture::start(png())?;
    let guard = NetworkGuard::with_ssrf_whitelist(["127.0.0.1/32"]);
    let transport = shacs_core::generated_media::UreqGuardedRemoteTransport::new(
        guard.clone(),
        Duration::from_secs(2),
    );

    // When
    let decision = RemoteOutputPolicy::download(1024, 1).evaluate(
        candidate(&fixture.url()),
        evaluation(Some(&guard), &transport),
    );
    let second_request_seen = fixture.finish()?;

    // Then
    assert!(matches!(decision, RemoteOutputDecision::ReadyToPersist(_)));
    assert!(second_request_seen);
    Ok(())
}
