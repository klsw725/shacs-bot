use super::model::Spec034ReleaseManifest;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommittedPublicationIdentity {
    pub content_digest: String,
    pub artifact_digest: String,
    pub binding_digest: String,
    pub destination_digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommittedPublicationResult {
    pub manifest: Spec034ReleaseManifest,
    pub identity: CommittedPublicationIdentity,
}
