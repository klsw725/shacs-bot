mod credential;
mod disclosure;
mod hooks;
mod process;
mod profile;
mod projection;
mod release_runner;
mod resource;
mod sandbox;
mod surface;
mod validation;
mod vocabulary;

pub use credential::CredentialStatusProjection;
pub use disclosure::{
    DataDisclosureProjection, DataSurface, TraceDestination, TraceDisclosureProjection,
    TracePreviewProjection, TraceStatus,
};
pub use hooks::{HookDenialProjection, HookDiagnosticProjection, HookRuntimeProjection};
pub use process::{
    ProcessAdapterCapabilities, ProcessAdapterProjection, ProcessControlReason,
    ProcessControlScope, ProcessOutcomeProjection,
};
pub use profile::{LifecycleBoundaryProjection, TrustedRuntimeProfileProjection};
pub use projection::{
    Spec030ParseError, Spec030ParseErrorKind, Spec030RuntimeProjection,
    Spec030RuntimeProjectionInput, SPEC030_SCHEMA_VERSION,
};
pub use release_runner::*;
pub use resource::{
    ResourceActivation, ResourceCandidateProjection, ResourceCollisionStatus,
    ResourceDiagnosticProjection, ResourceKind, ResourceLoadStatus, ResourcePrecedence,
    ResourceSource, TrustedCodeDisclosure,
};
pub use sandbox::SandboxStatusProjection;
pub use surface::{
    render_spec030_runtime, serialize_spec030_runtime, Spec030ProjectionProvider,
    UnavailableSpec030ProjectionProvider,
};
pub use validation::{Spec030ValidationError, Spec030ValidationViolation};
pub use vocabulary::*;

use validation::validate_runtime;
