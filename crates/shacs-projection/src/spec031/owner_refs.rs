use super::Spec031FixtureFamily;

pub(super) fn subject_ref(family: Spec031FixtureFamily) -> &'static str {
    ref_parts(family).0
}

pub(super) fn action_ref(family: Spec031FixtureFamily) -> &'static str {
    ref_parts(family).1
}

pub(super) fn digest(family: Spec031FixtureFamily) -> &'static str {
    ref_parts(family).2
}

pub(super) fn summary(family: Spec031FixtureFamily) -> &'static str {
    ref_parts(family).3
}

fn ref_parts(
    family: Spec031FixtureFamily,
) -> (&'static str, &'static str, &'static str, &'static str) {
    match family {
        Spec031FixtureFamily::Session => (
            "subject:session:canonical",
            "action:session:canonical",
            "sha256:sessioncanonical",
            "session owner defined zero active turns",
        ),
        Spec031FixtureFamily::Turn => (
            "subject:turn:canonical",
            "action:turn:canonical",
            "sha256:turncanonical",
            "turn adapter evidence is missing",
        ),
        Spec031FixtureFamily::Subagent => (
            "subject:subagent:canonical",
            "action:subagent:canonical",
            "sha256:subagentcanonical",
            "subagent recovery is blocked",
        ),
        Spec031FixtureFamily::Tool => (
            "subject:tool:canonical",
            "action:tool:canonical",
            "sha256:toolcanonical",
            "tool attempt owner defined zero",
        ),
        Spec031FixtureFamily::Approval => (
            "subject:approval:canonical",
            "action:approval:canonical",
            "sha256:approvalcanonical",
            "approval is pending",
        ),
        Spec031FixtureFamily::Recovery => (
            "subject:recovery:canonical",
            "action:recovery:canonical",
            "sha256:recoverycanonical",
            "recovery checkpoint is required",
        ),
        Spec031FixtureFamily::Readiness => (
            "subject:readiness:canonical",
            "action:readiness:canonical",
            "sha256:readinesscanonical",
            "readiness owner evidence missing",
        ),
        Spec031FixtureFamily::Context => (
            "subject:context:canonical",
            "action:context:canonical",
            "sha256:contextcanonical",
            "context item included",
        ),
        Spec031FixtureFamily::Extension => (
            "subject:extension:canonical",
            "action:extension:canonical",
            "sha256:extensioncanonical",
            "extension available with limitation",
        ),
        Spec031FixtureFamily::ExternalAppOwner => (
            "subject:app:external-owner",
            "action:app:external-owner",
            "sha256:appcanonical",
            "app owner evidence missing",
        ),
        Spec031FixtureFamily::ExternalMediaOwner => (
            "subject:media:external-owner",
            "action:media:external-owner",
            "sha256:mediacanonical",
            "media owner evidence missing",
        ),
        Spec031FixtureFamily::Delivery => (
            "subject:delivery:canonical",
            "action:delivery:canonical",
            "sha256:deliverycanonical",
            "progress delivery dropped",
        ),
        Spec031FixtureFamily::ReleaseEvidence => (
            "subject:release:canonical",
            "action:release:canonical",
            "sha256:releasecanonical",
            "release evidence has blocker",
        ),
    }
}
