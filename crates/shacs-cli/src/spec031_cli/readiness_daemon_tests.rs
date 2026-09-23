use super::*;

fn daemon_owner() -> Result<Spec030RuntimeProjection, Box<dyn Error>> {
    let facts = owner()?;
    facts.record_daemon_started()?;
    facts.update_sandbox(active(ProcessAdapterKind::GenericExec))?;
    Ok(LocalSpec030ProjectionProvider::new(facts).projection())
}

#[test]
fn healthy_daemon_lifecycle_with_active_exec_sandbox_is_ready() -> TestResult {
    let projection = daemon_owner()?;
    let original = projection.clone();
    let daemon = projection
        .process_adapters()
        .iter()
        .find(|adapter| adapter.adapter == ProcessAdapterKind::DaemonWorker)
        .ok_or("daemon adapter")?;
    assert_eq!(daemon.support, ProcessAdapterSupport::Supported);
    assert_eq!(daemon.control_scope, ProcessControlScope::LifecycleOnly);
    assert_eq!(daemon.reason, ProcessControlReason::DaemonLifecycleOnly);
    assert_eq!(daemon.availability, Spec030Availability::Available);
    assert!(daemon.capabilities.startup_readiness);
    assert!(daemon.capabilities.generation_fencing);
    assert!(!daemon.capabilities.descendant_cleanup);
    assert_eq!(
        projection.sandbox().applied_adapters,
        [ProcessAdapterKind::GenericExec]
    );

    let report = aggregate(&projection, Spec031Availability::Ready)?;

    assert_eq!(projection, original);
    assert_eq!(report.envelope().state(), Spec031Availability::Ready);
    Ok(())
}

fn input(projection: &Spec030RuntimeProjection) -> Spec030RuntimeProjectionInput {
    Spec030RuntimeProjectionInput {
        availability: projection.availability(),
        status: projection.status(),
        unavailable_reason: projection.unavailable_reason(),
        profile: projection.profile().clone(),
        lifecycle_boundaries: projection.lifecycle_boundaries().to_vec(),
        hooks: projection.hooks().clone(),
        process_adapters: projection.process_adapters().to_vec(),
        credential: projection.credential().clone(),
        sandbox: projection.sandbox().clone(),
        resources: projection.resources().to_vec(),
        disclosure: projection.disclosure().clone(),
    }
}

#[test]
fn daemon_missing_either_required_capability_remains_degraded() -> TestResult {
    for (startup_readiness, generation_fencing) in [(true, false), (false, true)] {
        let mut input = input(&daemon_owner()?);
        let daemon = input
            .process_adapters
            .iter_mut()
            .find(|adapter| adapter.adapter == ProcessAdapterKind::DaemonWorker)
            .ok_or("daemon adapter")?;
        daemon.capabilities.startup_readiness = startup_readiness;
        daemon.capabilities.generation_fencing = generation_fencing;
        let projection = Spec030RuntimeProjection::try_new(input)?;

        let report = aggregate(&projection, Spec031Availability::Ready)?;

        assert_eq!(report.envelope().state(), Spec031Availability::Degraded);
    }
    Ok(())
}

#[test]
fn daemon_explicit_availability_degradation_is_preserved() -> TestResult {
    let mut input = input(&daemon_owner()?);
    input
        .process_adapters
        .iter_mut()
        .find(|adapter| adapter.adapter == ProcessAdapterKind::DaemonWorker)
        .ok_or("daemon adapter")?
        .availability = Spec030Availability::Degraded;
    let projection = Spec030RuntimeProjection::try_new(input)?;

    let report = aggregate(&projection, Spec031Availability::Ready)?;

    assert_eq!(report.envelope().state(), Spec031Availability::Degraded);
    Ok(())
}

#[test]
fn daemon_lifecycle_missing_unavailable_or_inactive_is_not_ready() -> TestResult {
    for (status, expected) in [
        (None, Spec031Availability::Unknown),
        (
            Some(LifecycleBoundaryStatus::Unavailable),
            Spec031Availability::Unavailable,
        ),
        (
            Some(LifecycleBoundaryStatus::Inactive),
            Spec031Availability::Degraded,
        ),
    ] {
        let mut input = input(&daemon_owner()?);
        match status {
            Some(status) => {
                input
                    .lifecycle_boundaries
                    .iter_mut()
                    .find(|boundary| boundary.kind == LifecycleBoundaryKind::DaemonWorker)
                    .ok_or("daemon lifecycle")?
                    .status = status
            }
            None => input
                .lifecycle_boundaries
                .retain(|boundary| boundary.kind != LifecycleBoundaryKind::DaemonWorker),
        }
        let projection = Spec030RuntimeProjection::try_new(input)?;

        let report = aggregate(&projection, Spec031Availability::Ready)?;

        let controls = report
            .components()
            .iter()
            .find(|component| component.kind == Spec031ReadinessComponentKind::RuntimeControls)
            .ok_or("runtime controls")?;
        assert_eq!(controls.state, expected);
        assert_eq!(controls.requirement, Spec031ReadinessRequirement::Required);
        assert_eq!(
            report.envelope().state(),
            match expected {
                Spec031Availability::Unavailable => Spec031Availability::Unknown,
                state => state,
            }
        );
    }
    Ok(())
}

#[test]
fn daemon_readiness_does_not_override_sandbox_fail_closed() -> TestResult {
    for (sandbox, expected) in [
        (SandboxObservation::Unknown, Spec031Availability::Unknown),
        (SandboxObservation::Disabled, Spec031Availability::Degraded),
        (
            SandboxObservation::Unsupported,
            Spec031Availability::Degraded,
        ),
        (SandboxObservation::Failed, Spec031Availability::Blocked),
    ] {
        let facts = owner()?;
        facts.record_daemon_started()?;
        facts.update_sandbox(sandbox)?;
        let projection = LocalSpec030ProjectionProvider::new(facts).projection();

        let report = aggregate(&projection, Spec031Availability::Ready)?;

        assert_eq!(report.envelope().state(), expected);
    }
    Ok(())
}

#[test]
fn daemon_readiness_does_not_override_independent_containment() -> TestResult {
    for containment in [Spec031Availability::Unknown, Spec031Availability::Blocked] {
        let projection = daemon_owner()?;

        let report = aggregate(&projection, containment)?;

        assert_eq!(report.envelope().state(), containment);
    }
    Ok(())
}

#[test]
fn daemon_exception_does_not_promote_other_adapters_or_scopes() -> TestResult {
    for (kind, scope, reason) in [
        (
            ProcessAdapterKind::PythonKernel,
            ProcessControlScope::LifecycleOnly,
            ProcessControlReason::DaemonLifecycleOnly,
        ),
        (
            ProcessAdapterKind::Mcp,
            ProcessControlScope::TransportOnly,
            ProcessControlReason::McpTransportOnly,
        ),
        (
            ProcessAdapterKind::DaemonWorker,
            ProcessControlScope::ControlledChild,
            ProcessControlReason::ControlledChildObservedNoRollback,
        ),
    ] {
        let mut input = input(&daemon_owner()?);
        let capabilities = input
            .process_adapters
            .iter()
            .find(|adapter| adapter.adapter == ProcessAdapterKind::DaemonWorker)
            .ok_or("daemon adapter")?
            .capabilities;
        let adapter = input
            .process_adapters
            .iter_mut()
            .find(|adapter| adapter.adapter == kind)
            .ok_or("adapter")?;
        adapter.support = ProcessAdapterSupport::Supported;
        adapter.availability = Spec030Availability::Available;
        adapter.capabilities = capabilities;
        adapter.control_scope = scope;
        adapter.reason = reason;
        let projection = Spec030RuntimeProjection::try_new(input)?;

        let report = aggregate(&projection, Spec031Availability::Ready)?;

        assert_eq!(report.envelope().state(), Spec031Availability::Degraded);
    }
    Ok(())
}
