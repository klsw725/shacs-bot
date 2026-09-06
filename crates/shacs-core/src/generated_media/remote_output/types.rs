use super::super::{ProviderMediaCandidate, SafeProviderId};
use super::transport::UreqGuardedRemoteTransport;
use serde::Serialize;
use shacs_security::NetworkGuard;
use std::net::{IpAddr, SocketAddr};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemoteTransportError {
    ConnectionFailed,
    ResponseReadFailed,
    AmbiguousHeaders,
}

pub struct GuardedHopRequest<'a> {
    url: &'a str,
    max_bytes: usize,
}

impl<'a> GuardedHopRequest<'a> {
    pub(crate) const fn new(url: &'a str, max_bytes: usize) -> Self {
        Self { url, max_bytes }
    }

    pub const fn url(&self) -> &'a str {
        self.url
    }

    pub const fn max_bytes(&self) -> usize {
        self.max_bytes
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectedRemoteHop {
    pub(crate) peer_addr: SocketAddr,
    pub(crate) status: u16,
    pub(crate) content_type: String,
    pub(crate) location: Option<String>,
    pub(crate) body: Vec<u8>,
}

pub(crate) mod sealed {
    pub trait Sealed {}
}

/// Guarded transports and successful hop evidence are implemented only by `shacs-core`.
///
/// ```compile_fail
/// use shacs_core::generated_media::{
///     ConnectedRemoteHop, GuardedHopRequest, GuardedRemoteTransport, RemoteTransportError,
/// };
/// struct Forged;
/// impl GuardedRemoteTransport for Forged {
///     fn fetch(
///         &self,
///         _request: GuardedHopRequest<'_>,
///     ) -> Result<ConnectedRemoteHop, RemoteTransportError> {
///         Err(RemoteTransportError::ConnectionFailed)
///     }
/// }
/// ```
///
/// ```compile_fail
/// use shacs_core::generated_media::ConnectedRemoteHop;
/// let hop = ConnectedRemoteHop {
///     peer_addr: "93.184.216.34:443".parse().unwrap(),
///     status: 200,
///     content_type: "image/png".to_owned(),
///     location: None,
///     body: b"forged".to_vec(),
/// };
/// ```
pub trait GuardedRemoteTransport: sealed::Sealed + Send + Sync {
    fn fetch(
        &self,
        request: GuardedHopRequest<'_>,
    ) -> Result<ConnectedRemoteHop, RemoteTransportError>;
}

pub struct RemoteOutputEvaluationContext<'a> {
    guard: Option<&'a NetworkGuard>,
    transport: &'a UreqGuardedRemoteTransport,
    now: SystemTime,
}

impl<'a> RemoteOutputEvaluationContext<'a> {
    pub const fn new(
        guard: Option<&'a NetworkGuard>,
        transport: &'a UreqGuardedRemoteTransport,
        now: SystemTime,
    ) -> Self {
        Self {
            guard,
            transport,
            now,
        }
    }

    pub(crate) const fn into_parts(
        self,
    ) -> (
        Option<&'a NetworkGuard>,
        &'a UreqGuardedRemoteTransport,
        SystemTime,
    ) {
        (self.guard, self.transport, self.now)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RemoteReferenceExpiry(u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RemoteReferenceExpiryError;

impl RemoteReferenceExpiry {
    pub fn new(
        expires_at: SystemTime,
        now: SystemTime,
    ) -> Result<Self, RemoteReferenceExpiryError> {
        let seconds = expires_at
            .duration_since(UNIX_EPOCH)
            .map_err(|_| RemoteReferenceExpiryError)?
            .as_secs();
        let now_seconds = now
            .duration_since(UNIX_EPOCH)
            .map_err(|_| RemoteReferenceExpiryError)?
            .as_secs();
        if seconds <= now_seconds {
            return Err(RemoteReferenceExpiryError);
        }
        Ok(Self(seconds))
    }

    pub const fn unix_seconds(self) -> u64 {
        self.0
    }

    pub(crate) fn is_future_at(self, now: SystemTime) -> bool {
        now.duration_since(UNIX_EPOCH)
            .is_ok_and(|duration| self.0 > duration.as_secs())
    }
}

impl std::fmt::Display for RemoteReferenceExpiryError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("remote reference expiry must be in the future")
    }
}

impl std::error::Error for RemoteReferenceExpiryError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderRemoteReference {
    provider_id: SafeProviderId,
    domain: String,
    expires_at_unix_seconds: u64,
}

impl ProviderRemoteReference {
    pub(crate) fn new(
        provider_id: SafeProviderId,
        domain: String,
        expiry: RemoteReferenceExpiry,
    ) -> Self {
        Self {
            provider_id,
            domain,
            expires_at_unix_seconds: expiry.unix_seconds(),
        }
    }

    pub const fn provider_id(&self) -> &SafeProviderId {
        &self.provider_id
    }

    pub fn domain(&self) -> &str {
        &self.domain
    }

    pub const fn expires_at_unix_seconds(&self) -> u64 {
        self.expires_at_unix_seconds
    }
}

#[derive(Debug)]
pub enum RemoteOutputDecision {
    ReadyToPersist(ReadyToPersistRemoteMedia),
    Reference(ProviderRemoteReference),
    Rejected(RemoteRejection),
}

#[derive(Debug)]
pub struct ReadyToPersistRemoteMedia {
    candidate: ProviderMediaCandidate,
    evidence: RemoteFetchEvidence,
}

impl ReadyToPersistRemoteMedia {
    pub(crate) const fn new(
        candidate: ProviderMediaCandidate,
        evidence: RemoteFetchEvidence,
    ) -> Self {
        Self {
            candidate,
            evidence,
        }
    }

    pub const fn evidence(&self) -> &RemoteFetchEvidence {
        &self.evidence
    }

    pub fn into_candidate(self) -> ProviderMediaCandidate {
        self.candidate
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteFetchEvidence {
    pub(crate) connected_peer: IpAddr,
    pub(crate) redirects_followed: usize,
    pub(crate) byte_len: usize,
    pub(crate) mime_type: String,
}

impl RemoteFetchEvidence {
    pub const fn connected_peer(&self) -> IpAddr {
        self.connected_peer
    }

    pub const fn redirects_followed(&self) -> usize {
        self.redirects_followed
    }

    pub const fn byte_len(&self) -> usize {
        self.byte_len
    }

    pub fn mime_type(&self) -> &str {
        &self.mime_type
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RemoteRejection {
    reason: RemoteRejectionReason,
}

impl RemoteRejection {
    pub(crate) const fn new(reason: RemoteRejectionReason) -> Self {
        Self { reason }
    }

    pub const fn reason(self) -> RemoteRejectionReason {
        self.reason
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RemoteRejectionReason {
    PolicyRejected,
    GuardUnavailable,
    InvalidUrl,
    TargetPolicy,
    ConnectedPeerPolicy,
    Transport,
    HttpStatus,
    RedirectLocation,
    RedirectLoop,
    RedirectLimit,
    ByteLimit,
    MimeMismatch,
    AmbiguousHeaders,
    ReferenceExpired,
    UnsafeProviderIdentity,
}
