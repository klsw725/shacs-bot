#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProviderMediaContractError;

impl std::fmt::Display for ProviderMediaContractError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("provider media candidate id is invalid")
    }
}

impl std::error::Error for ProviderMediaContractError {}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ProviderMediaCandidateId(String);

impl ProviderMediaCandidateId {
    pub fn new(value: impl Into<String>) -> Result<Self, ProviderMediaContractError> {
        let value = value.into();
        if value.is_empty()
            || value.len() > 128
            || !value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        {
            return Err(ProviderMediaContractError);
        }
        Ok(Self(value))
    }

    pub fn into_string(self) -> String {
        self.0
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ProviderMediaLifecycleStatus {
    Started,
    Partial,
    Final,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ProviderMediaLifecycleObservation {
    candidate_id: ProviderMediaCandidateId,
    status: ProviderMediaLifecycleStatus,
    sequence: Option<u32>,
}

impl ProviderMediaLifecycleObservation {
    pub fn started(candidate_id: ProviderMediaCandidateId) -> Self {
        Self {
            candidate_id,
            status: ProviderMediaLifecycleStatus::Started,
            sequence: None,
        }
    }

    pub fn partial(candidate_id: ProviderMediaCandidateId, sequence: u32) -> Self {
        Self {
            candidate_id,
            status: ProviderMediaLifecycleStatus::Partial,
            sequence: Some(sequence),
        }
    }

    pub fn final_candidate(candidate_id: ProviderMediaCandidateId, sequence: u32) -> Self {
        Self {
            candidate_id,
            status: ProviderMediaLifecycleStatus::Final,
            sequence: Some(sequence),
        }
    }

    pub fn failed(candidate_id: ProviderMediaCandidateId, sequence: Option<u32>) -> Self {
        Self {
            candidate_id,
            status: ProviderMediaLifecycleStatus::Failed,
            sequence,
        }
    }

    pub fn cancelled(candidate_id: ProviderMediaCandidateId, sequence: Option<u32>) -> Self {
        Self {
            candidate_id,
            status: ProviderMediaLifecycleStatus::Cancelled,
            sequence,
        }
    }

    pub const fn status(&self) -> ProviderMediaLifecycleStatus {
        self.status
    }

    pub const fn sequence(&self) -> Option<u32> {
        self.sequence
    }

    pub fn candidate_id(&self) -> &ProviderMediaCandidateId {
        &self.candidate_id
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderMediaOrigin {
    provider_id: String,
    model_id: String,
}

impl ProviderMediaOrigin {
    pub fn new(provider_id: impl Into<String>, model_id: impl Into<String>) -> Self {
        Self {
            provider_id: provider_id.into(),
            model_id: model_id.into(),
        }
    }

    pub fn into_parts(self) -> (String, String) {
        (self.provider_id, self.model_id)
    }

    pub fn as_parts(&self) -> (&str, &str) {
        (&self.provider_id, &self.model_id)
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct ProviderMediaBytes {
    mime_type: String,
    bytes: Vec<u8>,
}

impl ProviderMediaBytes {
    pub fn new(mime_type: impl Into<String>, bytes: Vec<u8>) -> Self {
        Self {
            mime_type: mime_type.into(),
            bytes,
        }
    }

    pub fn into_parts(self) -> (String, Vec<u8>) {
        (self.mime_type, self.bytes)
    }
}

impl std::fmt::Debug for ProviderMediaBytes {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("ProviderMediaBytes([REDACTED])")
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct ProviderRemoteMedia {
    mime_type: String,
    url: String,
}

impl ProviderRemoteMedia {
    pub fn new(mime_type: impl Into<String>, url: impl Into<String>) -> Self {
        Self {
            mime_type: mime_type.into(),
            url: url.into(),
        }
    }

    pub fn into_parts(self) -> (String, String) {
        (self.mime_type, self.url)
    }
}

impl std::fmt::Debug for ProviderRemoteMedia {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("ProviderRemoteMedia([REDACTED])")
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct ProviderMediaCandidate {
    candidate_id: ProviderMediaCandidateId,
    origin: ProviderMediaOrigin,
    payload: ProviderMediaPayload,
}

#[derive(Clone, PartialEq, Eq)]
enum ProviderMediaPayload {
    Bytes(ProviderMediaBytes),
    Remote(ProviderRemoteMedia),
}

impl ProviderMediaCandidate {
    pub fn bytes(
        candidate_id: ProviderMediaCandidateId,
        origin: ProviderMediaOrigin,
        media: ProviderMediaBytes,
    ) -> Self {
        Self {
            candidate_id,
            origin,
            payload: ProviderMediaPayload::Bytes(media),
        }
    }

    pub fn remote(
        candidate_id: ProviderMediaCandidateId,
        origin: ProviderMediaOrigin,
        media: ProviderRemoteMedia,
    ) -> Self {
        Self {
            candidate_id,
            origin,
            payload: ProviderMediaPayload::Remote(media),
        }
    }

    pub fn into_local_bytes(self) -> Result<ProviderLocalMediaCandidate, ProviderRemoteMedia> {
        match self.payload {
            ProviderMediaPayload::Bytes(media) => Ok(ProviderLocalMediaCandidate {
                candidate_id: self.candidate_id,
                origin: self.origin,
                media,
            }),
            ProviderMediaPayload::Remote(media) => Err(media),
        }
    }
}

impl std::fmt::Debug for ProviderMediaCandidate {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ProviderMediaCandidate")
            .field("candidate_id", &self.candidate_id)
            .field("origin", &self.origin)
            .field("payload", &"[REDACTED]")
            .finish()
    }
}

pub struct ProviderLocalMediaCandidate {
    candidate_id: ProviderMediaCandidateId,
    origin: ProviderMediaOrigin,
    media: ProviderMediaBytes,
}

impl ProviderLocalMediaCandidate {
    pub fn into_parts(
        self,
    ) -> (
        ProviderMediaCandidateId,
        ProviderMediaOrigin,
        ProviderMediaBytes,
    ) {
        (self.candidate_id, self.origin, self.media)
    }
}
mod remote_candidate;
pub use remote_candidate::ProviderRemoteMediaCandidate;
