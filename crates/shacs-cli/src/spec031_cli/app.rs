use super::{reason_summary, render::envelope_line, severity};
use crate::{opaque_ref, CliError};
use shacs_app::app::{AppId, AppRegistryEntry, AppRegistryStore};
use shacs_app::app_lifecycle::{
    AppLifecycleBlocker, AppLifecycleReceipt, AppProcessState, AppSupervisorJournal,
};
use shacs_projection::{
    Spec031ActionRef, Spec031AppCapability, Spec031Availability, Spec031Capability,
    Spec031ConstructionError, Spec031Envelope, Spec031EnvelopeInput, Spec031Freshness,
    Spec031Lineage, Spec031ObservedAtUnixMs, Spec031ProjectionKind, Spec031Reason,
    Spec031ReasonCode, Spec031SafeSummary, Spec031SchemaVersion, Spec031Source, Spec031SourceOwner,
    Spec031SubjectRef,
};

pub(crate) fn read_receipt(
    store: &AppRegistryStore,
    app_id: &AppId,
) -> Result<Option<AppLifecycleReceipt>, CliError> {
    Ok(AppSupervisorJournal::new(store.apps_dir())
        .replay(app_id)?
        .receipts
        .pop())
}

pub(crate) fn lines(
    entry: &AppRegistryEntry,
    receipt: Option<&AppLifecycleReceipt>,
) -> Vec<String> {
    let mut lines = vec![match envelope(entry, receipt) {
        Ok(envelope) => envelope_line("app", &envelope),
        Err(error) => format!("Spec031 app: state=unavailable reason=unsupported detail={error}"),
    }];
    if let Some(receipt) = receipt {
        lines.push(format!(
            "Lifecycle: {:?} completed={} receipt={} request={}",
            receipt.current_state,
            receipt.completed,
            safe_ref(&format!("receipt:{}", receipt.receipt_id)),
            safe_ref(&format!("request:{}", receipt.request_id))
        ));
        lines.push(format!(
            "Blockers: {}",
            receipt
                .blockers
                .iter()
                .map(blocker)
                .collect::<Vec<_>>()
                .join(", ")
        ));
        for reference in &receipt.activation_refs {
            lines.push(format!("Activation: {}", safe_ref(reference)));
        }
        if let Some(reference) = &receipt.execution_snapshot_ref {
            lines.push(format!("Execution snapshot: {}", safe_ref(reference)));
        }
    }
    lines
}

fn envelope(
    entry: &AppRegistryEntry,
    receipt: Option<&AppLifecycleReceipt>,
) -> Result<Spec031Envelope, Spec031ConstructionError> {
    let (state, reason) = match receipt {
        None => (
            Spec031Availability::Unavailable,
            Spec031ReasonCode::MissingExternalOwnerEvidence,
        ),
        Some(receipt) => match receipt.current_state {
            AppProcessState::Failed | AppProcessState::RecoveryNeeded => {
                (Spec031Availability::Blocked, Spec031ReasonCode::Blocked)
            }
            AppProcessState::Starting | AppProcessState::Stopping => {
                (Spec031Availability::Unknown, Spec031ReasonCode::Requested)
            }
            AppProcessState::Running => (Spec031Availability::Ready, Spec031ReasonCode::Progress),
            AppProcessState::Stopped => (Spec031Availability::Ready, Spec031ReasonCode::Completed),
            AppProcessState::Installed => (
                Spec031Availability::Unavailable,
                Spec031ReasonCode::MissingExternalOwnerEvidence,
            ),
        },
    };
    Spec031Envelope::try_new(Spec031EnvelopeInput {
        schema_version: Spec031SchemaVersion::CURRENT,
        kind: Spec031ProjectionKind::App,
        state,
        severity: severity(state),
        reason: Spec031Reason {
            code: reason,
            safe_summary: Spec031SafeSummary::try_new(reason_summary(reason))?,
        },
        lineage: Spec031Lineage {
            subject_ref: Spec031SubjectRef::try_new(&opaque_ref(
                "subject:app",
                entry.app_id.as_str(),
            ))?,
            parent_ref: None,
            action_ref: receipt
                .map(|receipt| {
                    Spec031ActionRef::try_new(&safe_ref(&format!("request:{}", receipt.request_id)))
                })
                .transpose()?,
            digest: None,
        },
        source: Spec031Source {
            owner: Spec031SourceOwner::Spec032,
            observed_at_unix_ms: receipt.map(|receipt| {
                Spec031ObservedAtUnixMs::new(
                    u64::try_from(receipt.occurred_at_unix_ms).unwrap_or(u64::MAX),
                )
            }),
            freshness: if receipt.is_some() {
                Spec031Freshness::Current
            } else {
                Spec031Freshness::Unavailable
            },
        },
        capability: Spec031Capability::App(Spec031AppCapability {
            availability: state,
        }),
        children: Vec::new(),
    })
}

fn safe_ref(value: &str) -> String {
    Spec031SubjectRef::try_new(value)
        .map(|reference| reference.as_str().to_owned())
        .unwrap_or_else(|_| opaque_ref("ref", value))
}

fn blocker(value: &AppLifecycleBlocker) -> String {
    match value {
        AppLifecycleBlocker::CredentialMissing { name } => {
            format!("credential_missing:{}", safe_ref(name))
        }
        AppLifecycleBlocker::ActivationMissing { resource_ref } => {
            format!("activation_missing:{}", safe_ref(resource_ref))
        }
        AppLifecycleBlocker::ActivationStale { activation_ref } => {
            format!("activation_stale:{}", safe_ref(activation_ref))
        }
        AppLifecycleBlocker::ActivationDisabled { activation_ref } => {
            format!("activation_disabled:{}", safe_ref(activation_ref))
        }
        AppLifecycleBlocker::ActivationRevoked { activation_ref } => {
            format!("activation_revoked:{}", safe_ref(activation_ref))
        }
        AppLifecycleBlocker::ActivationRemoved { activation_ref } => {
            format!("activation_removed:{}", safe_ref(activation_ref))
        }
        AppLifecycleBlocker::AppNotEnabled => "app_not_enabled".to_owned(),
        AppLifecycleBlocker::ManifestDigestMismatch => "manifest_digest_mismatch".to_owned(),
        AppLifecycleBlocker::WorkspaceUntrusted => "workspace_untrusted".to_owned(),
        AppLifecycleBlocker::TrustedRuntimeUnavailable => "trusted_runtime_unavailable".to_owned(),
        AppLifecycleBlocker::RuntimePrerequisiteMissing => {
            "runtime_prerequisite_missing".to_owned()
        }
        AppLifecycleBlocker::ProcessPermissionDenied => "process_permission_denied".to_owned(),
        AppLifecycleBlocker::OwnerAlreadyRunning => "owner_already_running".to_owned(),
        AppLifecycleBlocker::RecoveryRequired => "recovery_required".to_owned(),
    }
}
