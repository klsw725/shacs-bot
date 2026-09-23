use shacs_core::runtime::trusted_runtime::{
    LocalSpec030ProjectionProvider, ProcessAdapterRegistration, SandboxObservation,
    Spec030FactStore, TraceDisclosureUpdate, WorkspaceTrustObservation,
};
use shacs_projection::*;
use std::error::Error;

type TestResult = Result<(), Box<dyn Error>>;

#[path = "readiness_daemon_tests.rs"]
mod daemon;

fn owner() -> Result<Spec030FactStore, Box<dyn Error>> {
    let facts = Spec030FactStore::new(WorkspaceTrustObservation::Trusted);
    facts.update_trace(TraceDisclosureUpdate {
        raw_content_possible: true,
        surfaces: vec![DataSurface::Session, DataSurface::Trace],
        trace: TraceDisclosureProjection {
            status: TraceStatus::Disabled,
            preview: None,
        },
    })?;
    facts.register_process_adapter(ProcessAdapterRegistration {
        adapter: ProcessAdapterKind::GenericExec,
        capabilities: ProcessAdapterCapabilities {
            timeout: true,
            abort: true,
            cwd: true,
            env: true,
            bounded_output: true,
            descendant_cleanup: true,
            startup_readiness: false,
            generation_fencing: false,
        },
        reason: ProcessControlReason::ControlledChildObservedNoRollback,
    })?;
    Ok(facts)
}

fn aggregate(
    owner: &Spec030RuntimeProjection,
    containment: Spec031Availability,
) -> Result<Spec031ReadinessReport, Box<dyn Error>> {
    let observations = super::readiness_runtime::observations(owner)?;
    let mut all = Spec031ReadinessComponentKind::REQUIRED
        .into_iter()
        .filter(|kind| {
            !observations
                .iter()
                .any(|observation| observation.kind == *kind)
        })
        .map(|kind| {
            super::readiness_observation::observation(
                kind,
                if kind == Spec031ReadinessComponentKind::Containment {
                    containment
                } else {
                    Spec031Availability::Ready
                },
                Spec031ReasonCode::Included,
                "controlled independent component observation",
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
    all.extend(observations);
    Ok(spec031_aggregate_readiness(&all)?)
}

#[test]
fn aggregate_preserves_actual_sandbox_observations_not_runtime_active() -> TestResult {
    for (sandbox, expected) in [
        (SandboxObservation::Unknown, Spec031Availability::Unknown),
        (SandboxObservation::Disabled, Spec031Availability::Degraded),
        (
            SandboxObservation::Unsupported,
            Spec031Availability::Degraded,
        ),
        (SandboxObservation::Failed, Spec031Availability::Blocked),
        (
            active(ProcessAdapterKind::GenericExec),
            Spec031Availability::Ready,
        ),
    ] {
        let facts = owner()?;
        facts.update_sandbox(sandbox)?;
        let projection = LocalSpec030ProjectionProvider::new(facts).projection();
        assert_eq!(projection.status(), Spec030RuntimeStatus::Active);
        let report = aggregate(&projection, Spec031Availability::Ready)?;
        assert_eq!(report.envelope().state(), expected);
    }
    Ok(())
}

fn active(adapter: ProcessAdapterKind) -> SandboxObservation {
    SandboxObservation::Active {
        applied_adapters: vec![adapter],
        filesystem_policy: SandboxFilesystemPolicy::Applied,
        network_policy: SandboxNetworkPolicy::Applied,
    }
}

#[test]
fn unavailable_owner_cannot_be_replaced_by_container_evidence() -> TestResult {
    let owner = Spec030RuntimeProjection::unavailable(Spec030UnavailableReason::OwnerUnavailable);
    let report = aggregate(&owner, Spec031Availability::Ready)?;
    assert_eq!(report.envelope().state(), Spec031Availability::Unknown);
    for kind in [
        Spec031ReadinessComponentKind::RuntimeControls,
        Spec031ReadinessComponentKind::ResourceDisclosure,
    ] {
        let observation = report
            .components()
            .iter()
            .find(|component| component.kind == kind)
            .ok_or("component")?;
        assert_eq!(
            observation.requirement,
            Spec031ReadinessRequirement::Required
        );
        assert_eq!(observation.state, Spec031Availability::Unavailable);
        assert_eq!(observation.freshness, Spec031Freshness::Unavailable);
        assert_eq!(observation.reason_code, Spec031ReasonCode::Missing);
    }
    Ok(())
}

#[test]
fn native_containment_unknown_is_not_overridden_by_trusted_profile() -> TestResult {
    let facts = owner()?;
    facts.update_sandbox(active(ProcessAdapterKind::GenericExec))?;
    let projection = LocalSpec030ProjectionProvider::new(facts).projection();
    let report = aggregate(&projection, Spec031Availability::Unknown)?;
    assert_eq!(report.envelope().state(), Spec031Availability::Unknown);
    Ok(())
}

#[test]
fn active_sandbox_requires_its_applied_adapter_observation() -> TestResult {
    let facts = owner()?;
    facts.update_sandbox(active(ProcessAdapterKind::Bash))?;
    let projection = LocalSpec030ProjectionProvider::new(facts).projection();
    let report = aggregate(&projection, Spec031Availability::Ready)?;
    assert_eq!(report.envelope().state(), Spec031Availability::Unknown);
    Ok(())
}

#[test]
fn active_sandbox_unknown_policy_is_not_ready() -> TestResult {
    let facts = owner()?;
    facts.update_sandbox(SandboxObservation::Active {
        applied_adapters: vec![ProcessAdapterKind::GenericExec],
        filesystem_policy: SandboxFilesystemPolicy::Unknown,
        network_policy: SandboxNetworkPolicy::Applied,
    })?;
    let projection = LocalSpec030ProjectionProvider::new(facts).projection();
    let report = aggregate(&projection, Spec031Availability::Ready)?;
    assert_eq!(report.envelope().state(), Spec031Availability::Unknown);
    Ok(())
}

#[test]
fn trace_disclosure_absence_stays_unavailable() -> TestResult {
    let facts = owner()?;
    facts.update_sandbox(active(ProcessAdapterKind::GenericExec))?;
    facts.update_trace(TraceDisclosureUpdate {
        raw_content_possible: true,
        surfaces: vec![DataSurface::Trace],
        trace: TraceDisclosureProjection {
            status: TraceStatus::Unavailable,
            preview: None,
        },
    })?;
    let projection = LocalSpec030ProjectionProvider::new(facts).projection();
    let report = aggregate(&projection, Spec031Availability::Ready)?;
    assert_eq!(report.envelope().state(), Spec031Availability::Unknown);
    let disclosure = report
        .components()
        .iter()
        .find(|component| component.kind == Spec031ReadinessComponentKind::ResourceDisclosure)
        .ok_or("disclosure")?;
    assert_eq!(disclosure.state, Spec031Availability::Unavailable);
    Ok(())
}

#[test]
fn usable_limited_adapter_degrades_aggregate() -> TestResult {
    let facts = owner()?;
    facts.update_sandbox(active(ProcessAdapterKind::GenericExec))?;
    facts.register_process_adapter(ProcessAdapterRegistration {
        adapter: ProcessAdapterKind::GenericExec,
        capabilities: ProcessAdapterCapabilities {
            timeout: true,
            abort: true,
            cwd: true,
            env: true,
            bounded_output: true,
            descendant_cleanup: false,
            startup_readiness: false,
            generation_fencing: false,
        },
        reason: ProcessControlReason::ControlledChildObservedNoRollback,
    })?;
    let projection = LocalSpec030ProjectionProvider::new(facts).projection();
    let report = aggregate(&projection, Spec031Availability::Ready)?;
    assert_eq!(report.envelope().state(), Spec031Availability::Degraded);
    Ok(())
}
