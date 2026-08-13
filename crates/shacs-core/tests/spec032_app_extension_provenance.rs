use shacs_app::app::AppLifecycleState;
use shacs_app::app_lifecycle::AppProcessState;
use shacs_core::runtime::{
    resolve_app_extension_provenance, ActivationReason, ActivationStatus, AppExtensionBlocker,
    AppExtensionReplayInput, AppExtensionStatus,
};
use std::error::Error;

#[path = "spec032_app_extension_provenance/support.rs"]
mod support;
use support::{activation, eligible, facts, ineligible};

#[test]
fn projection_is_absent_when_extension_is_only_discovered_or_installed(
) -> Result<(), Box<dyn Error>> {
    // Given
    let discovered = AppExtensionReplayInput::new(None, None, None);
    let installed = facts(
        AppLifecycleState::Installed,
        AppProcessState::Installed,
        "content-a",
    );

    // When / Then
    assert!(resolve_app_extension_provenance(&discovered).is_none());
    assert!(
        resolve_app_extension_provenance(&AppExtensionReplayInput::new(
            Some(&installed),
            Some(&eligible("content-a")),
            None,
        ))
        .is_none()
    );
    Ok(())
}

#[test]
fn persisted_activation_status_and_reason_are_projected_exhaustively() -> Result<(), Box<dyn Error>>
{
    // Given
    let source = facts(
        AppLifecycleState::Enabled,
        AppProcessState::Running,
        "content-a",
    );
    let resource = eligible("content-a");
    let cases = [
        (
            ActivationStatus::Active,
            ActivationReason::Activated,
            AppExtensionStatus::Active,
        ),
        (
            ActivationStatus::Stale,
            ActivationReason::ContentDigestMismatch,
            AppExtensionStatus::Stale,
        ),
        (
            ActivationStatus::Disabled,
            ActivationReason::UserDisabled,
            AppExtensionStatus::Disabled,
        ),
        (
            ActivationStatus::Revoked,
            ActivationReason::UserRevoked,
            AppExtensionStatus::Revoked,
        ),
        (
            ActivationStatus::Removed,
            ActivationReason::SourceRemoved,
            AppExtensionStatus::Removed,
        ),
    ];

    for (activation_status, reason, expected) in cases {
        // When
        let activation = activation(activation_status, reason, "content-a");
        let projection = resolve_app_extension_provenance(&AppExtensionReplayInput::new(
            Some(&source),
            Some(&resource),
            Some(&activation),
        ))
        .ok_or("activated extension must retain projection")?;

        // Then
        assert_eq!(projection.status, expected);
        assert_eq!(projection.activation_status, activation_status);
        assert_eq!(projection.activation_reason, reason);
    }
    Ok(())
}

#[test]
fn live_untrusted_fact_blocks_without_rewriting_persisted_activation() -> Result<(), Box<dyn Error>>
{
    // Given
    let source = facts(
        AppLifecycleState::Enabled,
        AppProcessState::Running,
        "content-a",
    );
    let activation = activation(
        ActivationStatus::Active,
        ActivationReason::Activated,
        "content-a",
    );
    let resource = ineligible("content-a");

    // When
    let projection = resolve_app_extension_provenance(&AppExtensionReplayInput::new(
        Some(&source),
        Some(&resource),
        Some(&activation),
    ))
    .ok_or("activation history missing")?;

    // Then
    assert_eq!(projection.status, AppExtensionStatus::Untrusted);
    assert_eq!(projection.blockers, [AppExtensionBlocker::Spec030Untrusted]);
    assert_eq!(projection.activation_status, ActivationStatus::Active);
    assert_eq!(activation.status(), ActivationStatus::Active);
    Ok(())
}

#[test]
fn same_name_with_different_digest_is_stale() -> Result<(), Box<dyn Error>> {
    // Given
    let source = facts(
        AppLifecycleState::Enabled,
        AppProcessState::Running,
        "content-b",
    );
    let activation = activation(
        ActivationStatus::Active,
        ActivationReason::Activated,
        "content-a",
    );
    let resource = eligible("content-b");

    // When
    let projection = resolve_app_extension_provenance(&AppExtensionReplayInput::new(
        Some(&source),
        Some(&resource),
        Some(&activation),
    ))
    .ok_or("activation history missing")?;

    // Then
    assert_eq!(projection.extension_name, "formatter");
    assert_eq!(projection.status, AppExtensionStatus::Stale);
    assert_eq!(
        projection.blockers,
        [AppExtensionBlocker::ContentDigestMismatch]
    );
    Ok(())
}

#[test]
fn disabled_and_removed_extensions_retain_activation_history() -> Result<(), Box<dyn Error>> {
    // Given
    let disabled = facts(
        AppLifecycleState::Disabled,
        AppProcessState::Stopped,
        "content-a",
    );
    let active = activation(
        ActivationStatus::Active,
        ActivationReason::Activated,
        "content-a",
    );
    let removed = activation(
        ActivationStatus::Removed,
        ActivationReason::SourceRemoved,
        "content-a",
    );

    // When
    let disabled_projection = resolve_app_extension_provenance(&AppExtensionReplayInput::new(
        Some(&disabled),
        Some(&eligible("content-a")),
        Some(&active),
    ))
    .ok_or("disabled history missing")?;
    let removed_projection =
        resolve_app_extension_provenance(&AppExtensionReplayInput::new(None, None, Some(&removed)))
            .ok_or("removed history missing")?;

    // Then
    assert_eq!(disabled_projection.status, AppExtensionStatus::Disabled);
    assert_eq!(disabled_projection.activation_ref, active.activation_ref());
    assert_eq!(removed_projection.status, AppExtensionStatus::Removed);
    assert_eq!(removed_projection.activation_ref, removed.activation_ref());
    Ok(())
}

#[test]
fn replay_projection_performs_zero_live_dispatch() -> Result<(), Box<dyn Error>> {
    // Given
    let source = facts(
        AppLifecycleState::Enabled,
        AppProcessState::Running,
        "content-a",
    );
    let activation = activation(
        ActivationStatus::Active,
        ActivationReason::Activated,
        "content-a",
    );

    // When
    let projection = resolve_app_extension_provenance(&AppExtensionReplayInput::new(
        Some(&source),
        Some(&eligible("content-a")),
        Some(&activation),
    ))
    .ok_or("activation history missing")?;

    // Then
    assert_eq!(projection.replay_dispatch_counters.total(), 0);
    Ok(())
}

#[test]
fn source_identity_mismatch_is_untrusted_and_missing_source_does_not_invent_app_id(
) -> Result<(), Box<dyn Error>> {
    let mut source = facts(
        AppLifecycleState::Enabled,
        AppProcessState::Running,
        "content-a",
    );
    source.source_identity = "app:other".to_owned();
    let active = activation(
        ActivationStatus::Active,
        ActivationReason::Activated,
        "content-a",
    );
    let projection = resolve_app_extension_provenance(&AppExtensionReplayInput::new(
        Some(&source),
        Some(&eligible("content-a")),
        Some(&active),
    ))
    .ok_or("projection")?;
    assert_eq!(projection.status, AppExtensionStatus::Untrusted);
    assert_eq!(
        projection.blockers,
        [AppExtensionBlocker::ResourceIdentityMismatch]
    );

    let removed = activation(
        ActivationStatus::Removed,
        ActivationReason::SourceRemoved,
        "content-a",
    );
    let history =
        resolve_app_extension_provenance(&AppExtensionReplayInput::new(None, None, Some(&removed)))
            .ok_or("history")?;
    assert!(history.source_app_id.is_none());
    Ok(())
}
