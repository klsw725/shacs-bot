use crate::controlled_child::{ControlledChildCommand, ControlledChildReceipt};
use sha2::{Digest, Sha256};
use shacs_projection::{
    ResourceActivation, ResourceCandidateProjection, ResourceCollisionStatus, ResourceKind,
    ResourceLoadStatus, ResourcePrecedence, ResourceSource, TrustedCodeDisclosure,
};
use std::ffi::OsString;
use std::path::PathBuf;
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct ResourceCandidate {
    pub resource_ref: String,
    pub kind: ResourceKind,
    pub source: ResourceSource,
    pub precedence: ResourcePrecedence,
    pub path: PathBuf,
    pub activation: ResourceActivation,
    pub trusted_code_disclosure: TrustedCodeDisclosure,
    pub load_check: ResourceLoadCheck,
    pub diagnostics: Vec<ResourceDiagnostic>,
}

#[derive(Debug, Clone)]
pub enum ResourceLoadCheck {
    Content,
    PackageCommand(ControlledChildCommand),
    PythonImport {
        interpreter: OsString,
        module: String,
        cwd: PathBuf,
        timeout: Duration,
    },
    JavaScriptModule {
        runtime: JavaScriptRuntime,
        program: OsString,
        module_path: PathBuf,
        cwd: PathBuf,
        timeout: Duration,
    },
    EmbeddedJavaScriptHost,
    DependencyResolution,
    Unsupported {
        reason: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JavaScriptRuntime {
    Node,
    Bun,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkspaceResourceTrust {
    Trusted,
    Untrusted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResourceEvidence {
    NotProvided,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceFact {
    pub projection: ResourceCandidateProjection,
    pub receipt: Option<ControlledChildReceipt>,
    pub authorization: ResourceEvidence,
    pub sandbox: ResourceEvidence,
}

impl ResourceFact {
    pub(super) fn from_candidate(
        candidate: ResourceCandidate,
        resolution: ResourceResolution,
    ) -> Self {
        let canonical_path = candidate.path.to_string_lossy().into_owned();
        Self::resolved(candidate, canonical_path, None, resolution)
    }

    pub(super) fn from_canonical(
        candidate: CanonicalCandidate,
        resolution: ResourceResolution,
    ) -> Self {
        Self::resolved(
            candidate.candidate,
            candidate.canonical_path,
            Some(candidate.content_sha256),
            resolution,
        )
    }

    fn resolved(
        candidate: ResourceCandidate,
        canonical_path: String,
        content_sha256: Option<String>,
        resolution: ResourceResolution,
    ) -> Self {
        Self {
            projection: ResourceCandidateProjection {
                resource_ref: candidate.resource_ref,
                kind: candidate.kind,
                source: candidate.source,
                precedence: candidate.precedence,
                canonical_path,
                content_sha256,
                collision: resolution.collision,
                load_status: resolution.load_status,
                activation: resolution.activation,
                trusted_code_disclosure: candidate.trusted_code_disclosure,
                diagnostics: candidate
                    .diagnostics
                    .into_iter()
                    .map(|diagnostic| diagnostic.projection())
                    .collect(),
            },
            receipt: resolution.receipt,
            authorization: ResourceEvidence::NotProvided,
            sandbox: ResourceEvidence::NotProvided,
        }
    }
}

pub(super) struct ResourceResolution {
    pub collision: ResourceCollisionStatus,
    pub activation: ResourceActivation,
    pub load_status: ResourceLoadStatus,
    pub receipt: Option<ControlledChildReceipt>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResourceDiagnosticKind {
    MalformedPath,
    CollisionWinner,
    CollisionLoser,
    WorkspaceTrustRequired,
    LoadFailed,
    RuntimeUnsupported,
    ScopedUnsupported,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceDiagnostic {
    pub resource_ref: String,
    pub kind: ResourceDiagnosticKind,
    pub path: Option<String>,
    pub reason: String,
}

impl ResourceDiagnostic {
    pub fn projection(&self) -> shacs_projection::ResourceDiagnosticProjection {
        shacs_projection::ResourceDiagnosticProjection {
            code: self.kind.label().to_owned(),
            path: self.path.clone(),
            reason: self.reason.clone(),
        }
    }
}

impl ResourceDiagnosticKind {
    pub const fn label(self) -> &'static str {
        match self {
            Self::MalformedPath => "malformedPath",
            Self::CollisionWinner => "collisionWinner",
            Self::CollisionLoser => "collisionLoser",
            Self::WorkspaceTrustRequired => "workspaceTrustRequired",
            Self::LoadFailed => "loadFailed",
            Self::RuntimeUnsupported => "runtimeUnsupported",
            Self::ScopedUnsupported => "scopedUnsupported",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrustedResourceInspection {
    pub resources: Vec<ResourceFact>,
    pub diagnostics: Vec<ResourceDiagnostic>,
}

pub(super) struct CanonicalCandidate {
    pub candidate: ResourceCandidate,
    pub canonical_path: String,
    pub path_bytes: Vec<u8>,
    pub content_sha256: String,
}

impl CanonicalCandidate {
    pub fn new(candidate: ResourceCandidate) -> Result<Self, CanonicalCandidateError> {
        let canonical = match candidate.path.canonicalize() {
            Ok(path) => path,
            Err(error) => {
                return Err(CanonicalCandidateError {
                    candidate: Box::new(candidate),
                    reason: error.to_string(),
                })
            }
        };
        let Some(canonical_path) = canonical.to_str().map(str::to_owned) else {
            return Err(CanonicalCandidateError {
                candidate: Box::new(candidate),
                reason: "canonical path is not valid UTF-8".to_owned(),
            });
        };
        let content = match std::fs::read(&canonical) {
            Ok(content) => content,
            Err(error) => {
                return Err(CanonicalCandidateError {
                    candidate: Box::new(candidate),
                    reason: error.to_string(),
                })
            }
        };
        Ok(Self {
            candidate,
            canonical_path,
            path_bytes: canonical.as_os_str().as_encoded_bytes().to_vec(),
            content_sha256: format!("{:x}", Sha256::digest(content)),
        })
    }
}

pub(super) struct CanonicalCandidateError {
    pub candidate: Box<ResourceCandidate>,
    pub reason: String,
}
