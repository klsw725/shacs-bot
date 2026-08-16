use super::{ProviderMediaCandidateId, ProviderMediaOrigin, ProviderRemoteMedia};

/// Provider-parsed remote media that cannot be constructed by downstream crates.
///
/// ```compile_fail
/// use shacs_providers::{
///     ProviderMediaCandidateId, ProviderMediaOrigin, ProviderRemoteMedia,
///     ProviderRemoteMediaCandidate,
/// };
/// let candidate = ProviderRemoteMediaCandidate::new(
///     ProviderMediaCandidateId::new("user-url").unwrap(),
///     ProviderMediaOrigin::new("user", "input"),
///     ProviderRemoteMedia::new("image/png", "https://example.com/user-input.png"),
/// );
/// ```
#[derive(Clone, PartialEq, Eq)]
pub struct ProviderRemoteMediaCandidate {
    candidate_id: ProviderMediaCandidateId,
    origin: ProviderMediaOrigin,
    media: ProviderRemoteMedia,
}

impl ProviderRemoteMediaCandidate {
    pub(crate) const fn new(
        candidate_id: ProviderMediaCandidateId,
        origin: ProviderMediaOrigin,
        media: ProviderRemoteMedia,
    ) -> Self {
        Self {
            candidate_id,
            origin,
            media,
        }
    }

    pub fn into_parts(
        self,
    ) -> (
        ProviderMediaCandidateId,
        ProviderMediaOrigin,
        ProviderRemoteMedia,
    ) {
        (self.candidate_id, self.origin, self.media)
    }
}

impl std::fmt::Debug for ProviderRemoteMediaCandidate {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ProviderRemoteMediaCandidate")
            .field("candidate_id", &self.candidate_id)
            .field("origin", &self.origin)
            .field("media", &"[REDACTED]")
            .finish()
    }
}
