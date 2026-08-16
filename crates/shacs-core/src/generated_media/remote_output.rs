mod peer_transport;
mod policy;
mod transport;
mod types;

pub use policy::RemoteOutputPolicy;
pub use transport::UreqGuardedRemoteTransport;
pub use types::{
    ConnectedRemoteHop, GuardedHopRequest, GuardedRemoteTransport, ProviderRemoteReference,
    ReadyToPersistRemoteMedia, RemoteFetchEvidence, RemoteOutputDecision,
    RemoteOutputEvaluationContext, RemoteReferenceExpiry, RemoteReferenceExpiryError,
    RemoteRejection, RemoteRejectionReason, RemoteTransportError,
};
