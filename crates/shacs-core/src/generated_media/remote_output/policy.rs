use super::types::{
    GuardedHopRequest, GuardedRemoteTransport, ProviderRemoteReference, ReadyToPersistRemoteMedia,
    RemoteFetchEvidence, RemoteOutputDecision, RemoteOutputEvaluationContext,
    RemoteReferenceExpiry, RemoteRejection, RemoteRejectionReason,
};
use crate::generated_media::{
    ProviderMediaBytes, ProviderMediaCandidate, ProviderRemoteMediaCandidate,
};
use shacs_security::{parse_http_url, resolve_redirect_url, NetworkGuard};
use std::collections::BTreeSet;

#[derive(Debug, Clone, Copy)]
enum RemoteDisposition {
    Download,
    Reference(RemoteReferenceExpiry),
    Reject,
}

struct RemoteDownloadCandidate {
    candidate_id: shacs_providers::ProviderMediaCandidateId,
    origin: shacs_providers::ProviderMediaOrigin,
    expected_mime: String,
    url: String,
}

struct InternalEvaluationContext<'a> {
    guard: Option<&'a NetworkGuard>,
    transport: &'a dyn GuardedRemoteTransport,
    now: std::time::SystemTime,
}

#[derive(Debug, Clone, Copy)]
pub struct RemoteOutputPolicy {
    disposition: RemoteDisposition,
    max_bytes: usize,
    max_redirects: usize,
}

impl RemoteOutputPolicy {
    pub const fn download(max_bytes: usize, max_redirects: usize) -> Self {
        Self {
            disposition: RemoteDisposition::Download,
            max_bytes,
            max_redirects,
        }
    }

    pub const fn reference(expiry: RemoteReferenceExpiry) -> Self {
        Self {
            disposition: RemoteDisposition::Reference(expiry),
            max_bytes: 0,
            max_redirects: 0,
        }
    }

    pub const fn reject() -> Self {
        Self {
            disposition: RemoteDisposition::Reject,
            max_bytes: 0,
            max_redirects: 0,
        }
    }

    pub fn evaluate(
        self,
        candidate: ProviderRemoteMediaCandidate,
        context: RemoteOutputEvaluationContext<'_>,
    ) -> RemoteOutputDecision {
        let (guard, transport, now) = context.into_parts();
        self.evaluate_with_transport(
            candidate,
            InternalEvaluationContext {
                guard,
                transport,
                now,
            },
        )
    }

    fn evaluate_with_transport(
        self,
        candidate: ProviderRemoteMediaCandidate,
        context: InternalEvaluationContext<'_>,
    ) -> RemoteOutputDecision {
        let InternalEvaluationContext {
            guard,
            transport,
            now,
        } = context;
        match self.disposition {
            RemoteDisposition::Reject => return rejected(RemoteRejectionReason::PolicyRejected),
            RemoteDisposition::Reference(expiry) if !expiry.is_future_at(now) => {
                return rejected(RemoteRejectionReason::ReferenceExpired)
            }
            RemoteDisposition::Download | RemoteDisposition::Reference(_) => {}
        }
        let Some(guard) = guard else {
            return rejected(RemoteRejectionReason::GuardUnavailable);
        };
        let (candidate_id, origin, remote) = candidate.into_parts();
        let (expected_mime, url) = remote.into_parts();
        let parsed = match validate_url(&url, guard) {
            Ok(parsed) => parsed,
            Err(()) => return rejected(classify_url_rejection(&url)),
        };
        match self.disposition {
            RemoteDisposition::Reference(expiry) => {
                let (provider_id, _) = origin.into_parts();
                let Ok(provider_id) = super::super::SafeProviderId::new(provider_id) else {
                    return rejected(RemoteRejectionReason::UnsafeProviderIdentity);
                };
                RemoteOutputDecision::Reference(ProviderRemoteReference::new(
                    provider_id,
                    parsed.hostname.to_ascii_lowercase(),
                    expiry,
                ))
            }
            RemoteDisposition::Download => self.download_candidate(
                RemoteDownloadCandidate {
                    candidate_id,
                    origin,
                    expected_mime,
                    url,
                },
                guard,
                transport,
            ),
            RemoteDisposition::Reject => rejected(RemoteRejectionReason::PolicyRejected),
        }
    }

    fn download_candidate(
        self,
        candidate: RemoteDownloadCandidate,
        guard: &NetworkGuard,
        transport: &dyn GuardedRemoteTransport,
    ) -> RemoteOutputDecision {
        let RemoteDownloadCandidate {
            candidate_id,
            origin,
            expected_mime,
            url,
        } = candidate;
        let mut current_url = url;
        let mut visited = BTreeSet::new();
        visited.insert(current_url.clone());
        let mut redirects = 0usize;
        loop {
            let response = match transport.fetch(GuardedHopRequest::new(
                &current_url,
                self.max_bytes.saturating_add(1),
            )) {
                Ok(response) => response,
                Err(super::types::RemoteTransportError::AmbiguousHeaders) => {
                    return rejected(RemoteRejectionReason::AmbiguousHeaders)
                }
                Err(
                    super::types::RemoteTransportError::ConnectionFailed
                    | super::types::RemoteTransportError::ResponseReadFailed,
                ) => return rejected(RemoteRejectionReason::Transport),
            };
            if guard.is_private(response.peer_addr.ip()) {
                return rejected(RemoteRejectionReason::ConnectedPeerPolicy);
            }
            if (300..400).contains(&response.status) {
                let Some(location) = response.location else {
                    return rejected(RemoteRejectionReason::RedirectLocation);
                };
                if redirects >= self.max_redirects {
                    return rejected(RemoteRejectionReason::RedirectLimit);
                }
                let Ok(next_url) = resolve_redirect_url(&current_url, &location) else {
                    return rejected(RemoteRejectionReason::InvalidUrl);
                };
                if validate_url(&next_url, guard).is_err() {
                    return rejected(classify_url_rejection(&next_url));
                }
                if !visited.insert(next_url.clone()) {
                    return rejected(RemoteRejectionReason::RedirectLoop);
                }
                redirects += 1;
                current_url = next_url;
                continue;
            }
            if !(200..300).contains(&response.status) {
                return rejected(RemoteRejectionReason::HttpStatus);
            }
            if response.body.len() > self.max_bytes {
                return rejected(RemoteRejectionReason::ByteLimit);
            }
            let response_mime = normalized_mime(&response.content_type);
            let magic_mime = sniff_image_mime(&response.body);
            if response_mime != Some(expected_mime.as_str()) || magic_mime != response_mime {
                return rejected(RemoteRejectionReason::MimeMismatch);
            }
            let peer = response.peer_addr.ip();
            let byte_len = response.body.len();
            let candidate = ProviderMediaCandidate::bytes(
                candidate_id,
                origin,
                ProviderMediaBytes::new(&expected_mime, response.body),
            );
            let evidence = RemoteFetchEvidence {
                connected_peer: peer,
                redirects_followed: redirects,
                byte_len,
                mime_type: expected_mime,
            };
            return RemoteOutputDecision::ReadyToPersist(ReadyToPersistRemoteMedia::new(
                candidate, evidence,
            ));
        }
    }
}

fn validate_url(url: &str, guard: &NetworkGuard) -> Result<shacs_security::ParsedUrl, ()> {
    let parsed = parse_http_url(url).map_err(|_| ())?;
    if parsed.authority.contains('@') {
        return Err(());
    }
    guard.validate_url_target(url).map_err(|_| ())?;
    Ok(parsed)
}

fn classify_url_rejection(url: &str) -> RemoteRejectionReason {
    match parse_http_url(url) {
        Ok(parsed) if !parsed.authority.contains('@') => RemoteRejectionReason::TargetPolicy,
        Ok(_) | Err(_) => RemoteRejectionReason::InvalidUrl,
    }
}

fn normalized_mime(value: &str) -> Option<&str> {
    value
        .split(';')
        .next()
        .map(str::trim)
        .filter(|value| matches!(*value, "image/png" | "image/jpeg" | "image/webp"))
}

fn sniff_image_mime(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("image/png")
    } else if bytes.starts_with(b"\xff\xd8\xff") {
        Some("image/jpeg")
    } else if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        Some("image/webp")
    } else {
        None
    }
}

const fn rejected(reason: RemoteRejectionReason) -> RemoteOutputDecision {
    RemoteOutputDecision::Rejected(RemoteRejection::new(reason))
}
