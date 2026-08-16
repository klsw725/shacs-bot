use super::fixture::{LoopbackFixture, NoRequestFixture, RawResponseFixture};
use super::support::{candidate, png};
use shacs_core::generated_media::{
    RemoteOutputDecision, RemoteOutputEvaluationContext, RemoteOutputPolicy, RemoteRejectionReason,
    UreqGuardedRemoteTransport,
};
use shacs_security::NetworkGuard;
use std::error::Error;
use std::time::{Duration, SystemTime};

fn rejected_reason(decision: RemoteOutputDecision) -> RemoteRejectionReason {
    match decision {
        RemoteOutputDecision::Rejected(rejection) => rejection.reason(),
        RemoteOutputDecision::ReadyToPersist(_) | RemoteOutputDecision::Reference(_) => {
            panic!("expected rejection")
        }
    }
}

#[test]
fn absent_guard_fails_closed_before_transport() -> Result<(), Box<dyn Error>> {
    // Given
    let fixture = NoRequestFixture::start()?;
    let transport =
        UreqGuardedRemoteTransport::new(NetworkGuard::default(), Duration::from_millis(200));
    let context = RemoteOutputEvaluationContext::new(None, &transport, SystemTime::UNIX_EPOCH);

    // When
    let decision =
        RemoteOutputPolicy::download(1024, 3).evaluate(candidate(&fixture.url()), context);
    let request_seen = fixture.finish()?;

    // Then
    assert_eq!(
        rejected_reason(decision),
        RemoteRejectionReason::GuardUnavailable
    );
    assert!(!request_seen);
    Ok(())
}

#[test]
fn initial_private_link_local_and_loopback_targets_fail_closed() {
    let guard = NetworkGuard::default();
    let transport = UreqGuardedRemoteTransport::new(guard.clone(), Duration::from_millis(200));
    for url in [
        "http://10.0.0.1/image.png",
        "http://169.254.169.254/image.png",
        "http://127.0.0.1/image.png",
    ] {
        let context =
            RemoteOutputEvaluationContext::new(Some(&guard), &transport, SystemTime::UNIX_EPOCH);
        let decision = RemoteOutputPolicy::download(1024, 3).evaluate(candidate(url), context);
        assert_eq!(
            rejected_reason(decision),
            RemoteRejectionReason::TargetPolicy
        );
    }
}

#[test]
fn redirect_target_is_revalidated_before_second_request() -> Result<(), Box<dyn Error>> {
    for location in [
        "http://10.0.0.1/private",
        "http://169.254.169.254/private",
        "http://localhost/private",
    ] {
        // Given
        let response = format!(
            "HTTP/1.1 302 Found\r\nLocation: {location}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
        );
        let fixture = RawResponseFixture::start(response.into_bytes())?;
        let guard = NetworkGuard::with_ssrf_whitelist(["127.0.0.1/32"]);
        let transport = UreqGuardedRemoteTransport::new(guard.clone(), Duration::from_secs(1));
        let context =
            RemoteOutputEvaluationContext::new(Some(&guard), &transport, SystemTime::UNIX_EPOCH);

        // When
        let decision =
            RemoteOutputPolicy::download(1024, 3).evaluate(candidate(&fixture.url()), context);
        fixture.finish()?;

        // Then
        assert_eq!(
            rejected_reason(decision),
            RemoteRejectionReason::TargetPolicy
        );
    }
    Ok(())
}

#[test]
fn concrete_transport_reports_actual_connected_peer() -> Result<(), Box<dyn Error>> {
    // Given
    let fixture = LoopbackFixture::start(png())?;
    let guard = NetworkGuard::with_ssrf_whitelist(["127.0.0.1/32"]);
    let transport = UreqGuardedRemoteTransport::new(guard.clone(), Duration::from_secs(1));
    let context =
        RemoteOutputEvaluationContext::new(Some(&guard), &transport, SystemTime::UNIX_EPOCH);

    // When
    let decision =
        RemoteOutputPolicy::download(1024, 0).evaluate(candidate(&fixture.url()), context);
    fixture.finish()?;

    // Then
    let RemoteOutputDecision::ReadyToPersist(ready) = decision else {
        return Err("expected guarded bytes".into());
    };
    assert!(ready.evidence().connected_peer().is_loopback());
    Ok(())
}

#[test]
fn userinfo_and_unsupported_schemes_are_rejected() {
    let guard = NetworkGuard::default();
    let transport = UreqGuardedRemoteTransport::new(guard.clone(), Duration::from_millis(200));
    for url in [
        "https://user@example.com/image.png",
        "file:///tmp/image.png",
    ] {
        let context =
            RemoteOutputEvaluationContext::new(Some(&guard), &transport, SystemTime::UNIX_EPOCH);
        let decision = RemoteOutputPolicy::download(1024, 3).evaluate(candidate(url), context);
        assert_eq!(rejected_reason(decision), RemoteRejectionReason::InvalidUrl);
    }
}

#[test]
fn redirect_loops_and_limit_are_rejected() -> Result<(), Box<dyn Error>> {
    // Given
    let loop_fixture = RawResponseFixture::start(
        b"HTTP/1.1 302 Found\r\nLocation: /start\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            .to_vec(),
    )?;
    let guard = NetworkGuard::with_ssrf_whitelist(["127.0.0.1/32"]);
    let transport = UreqGuardedRemoteTransport::new(guard.clone(), Duration::from_secs(1));
    let context =
        RemoteOutputEvaluationContext::new(Some(&guard), &transport, SystemTime::UNIX_EPOCH);

    // When
    let loop_decision =
        RemoteOutputPolicy::download(1024, 3).evaluate(candidate(&loop_fixture.url()), context);
    loop_fixture.finish()?;

    // Then
    assert_eq!(
        rejected_reason(loop_decision),
        RemoteRejectionReason::RedirectLoop
    );

    let limit_fixture = RawResponseFixture::start(
        b"HTTP/1.1 302 Found\r\nLocation: /two\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            .to_vec(),
    )?;
    let context =
        RemoteOutputEvaluationContext::new(Some(&guard), &transport, SystemTime::UNIX_EPOCH);
    let limit_decision =
        RemoteOutputPolicy::download(1024, 0).evaluate(candidate(&limit_fixture.url()), context);
    limit_fixture.finish()?;
    assert_eq!(
        rejected_reason(limit_decision),
        RemoteRejectionReason::RedirectLimit
    );
    Ok(())
}

#[test]
fn byte_cap_and_mime_magic_mismatch_fail_closed() -> Result<(), Box<dyn Error>> {
    let guard = NetworkGuard::with_ssrf_whitelist(["127.0.0.1/32"]);
    let transport = UreqGuardedRemoteTransport::new(guard.clone(), Duration::from_secs(1));
    for (body, content_type, limit, expected) in [
        (vec![0; 9], "image/png", 8, RemoteRejectionReason::ByteLimit),
        (
            png(),
            "image/jpeg",
            1024,
            RemoteRejectionReason::MimeMismatch,
        ),
    ] {
        // Given
        let header = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        let fixture = RawResponseFixture::start([header.into_bytes(), body].concat())?;
        let context =
            RemoteOutputEvaluationContext::new(Some(&guard), &transport, SystemTime::UNIX_EPOCH);

        // When
        let decision =
            RemoteOutputPolicy::download(limit, 0).evaluate(candidate(&fixture.url()), context);
        fixture.finish()?;

        // Then
        assert_eq!(rejected_reason(decision), expected);
    }
    Ok(())
}
