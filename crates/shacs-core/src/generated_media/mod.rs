mod artifact_store;
mod contracts;
mod identifiers;
mod image_operations;
mod provider_boundary;
mod publication;
mod remote_output;
mod safe_facts;

pub use artifact_store::{
    ArtifactReadStage, ArtifactStore, ArtifactStoreError, ArtifactTransactionStage,
    TransactionDecision,
};
pub use contracts::{
    ArtifactHandlingPolicy, CommittedArtifact, GeneratedArtifactDefinition,
    GeneratedArtifactMetadata, GeneratedArtifactRecord, GeneratedArtifactRef, GeneratedMediaKind,
    GeneratedProvenance, GeneratedProvenanceKind, GenerationOperation, InboundProvenance,
    InboundProvenanceKind, ProjectionDisclosure, RetentionPolicy,
};
pub use identifiers::{
    ArtifactId, CandidateId, GeneratedMediaContractError, MediaLineageId, MediaRootRelativePath,
    Sha256Digest,
};
pub use image_operations::{
    AdmittedImageOperation, ArtifactImageOperationRequest, ImageOperationAdmissionError,
    ImageOperationService, ValidatedImageOperationCandidate, ValidatedLocalImage,
    MAX_IMAGE_OPERATION_SOURCE_BYTES,
};
pub use provider_boundary::{
    ArtifactWriteRequest, ProviderMediaFailureReason, ProviderMediaLifecycleEvent,
    ProviderMediaLifecycleStatus,
};
pub use publication::{
    ArtifactPublicationError, ArtifactPublicationMetadata, ArtifactPublisher,
    RemotePublicationOutcome, RemotePublicationReference,
};
pub use remote_output::{
    ConnectedRemoteHop, GuardedHopRequest, GuardedRemoteTransport, ProviderRemoteReference,
    ReadyToPersistRemoteMedia, RemoteFetchEvidence, RemoteOutputDecision,
    RemoteOutputEvaluationContext, RemoteOutputPolicy, RemoteReferenceExpiry,
    RemoteReferenceExpiryError, RemoteRejection, RemoteRejectionReason, RemoteTransportError,
    UreqGuardedRemoteTransport,
};
pub use safe_facts::{
    GenerationOptionsSummary, SafeModelId, SafeOptionName, SafeOptionValue, SafeProviderId,
};
pub use shacs_providers::{
    ProviderMediaBytes, ProviderMediaCandidate, ProviderMediaCandidateId, ProviderMediaOrigin,
    ProviderRemoteMedia, ProviderRemoteMediaCandidate,
};
