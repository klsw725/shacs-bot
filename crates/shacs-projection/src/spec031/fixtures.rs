use super::owner_contract::canonical_owner_record_input;
use super::{
    spec031_missing_external_owner_evidence, spec031_project_owner_record,
    Spec031ConstructionError, Spec031Envelope,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Spec031FixtureFamily {
    Session,
    Turn,
    Subagent,
    Tool,
    Approval,
    Recovery,
    Readiness,
    Context,
    Extension,
    ExternalAppOwner,
    ExternalMediaOwner,
    Delivery,
    ReleaseEvidence,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Spec031CanonicalFixture {
    family: Spec031FixtureFamily,
    envelope: Spec031Envelope,
}

impl Spec031CanonicalFixture {
    pub const fn family(&self) -> Spec031FixtureFamily {
        self.family
    }

    pub const fn envelope(&self) -> &Spec031Envelope {
        &self.envelope
    }
}

pub fn spec031_canonical_fixture_registry(
) -> Result<Vec<Spec031CanonicalFixture>, Spec031ConstructionError> {
    let mut fixtures = Vec::with_capacity(SPEC031_FIXTURE_FAMILIES.len());
    for family in SPEC031_FIXTURE_FAMILIES {
        fixtures.push(Spec031CanonicalFixture {
            family,
            envelope: canonical_envelope(family)?,
        });
    }
    Ok(fixtures)
}

const SPEC031_FIXTURE_FAMILIES: [Spec031FixtureFamily; 13] = [
    Spec031FixtureFamily::Session,
    Spec031FixtureFamily::Turn,
    Spec031FixtureFamily::Subagent,
    Spec031FixtureFamily::Tool,
    Spec031FixtureFamily::Approval,
    Spec031FixtureFamily::Recovery,
    Spec031FixtureFamily::Readiness,
    Spec031FixtureFamily::Context,
    Spec031FixtureFamily::Extension,
    Spec031FixtureFamily::ExternalAppOwner,
    Spec031FixtureFamily::ExternalMediaOwner,
    Spec031FixtureFamily::Delivery,
    Spec031FixtureFamily::ReleaseEvidence,
];

fn canonical_envelope(
    family: Spec031FixtureFamily,
) -> Result<Spec031Envelope, Spec031ConstructionError> {
    match family {
        Spec031FixtureFamily::ExternalAppOwner
        | Spec031FixtureFamily::ExternalMediaOwner
        | Spec031FixtureFamily::Readiness => spec031_missing_external_owner_evidence(family),
        Spec031FixtureFamily::Session
        | Spec031FixtureFamily::Turn
        | Spec031FixtureFamily::Subagent
        | Spec031FixtureFamily::Tool
        | Spec031FixtureFamily::Approval
        | Spec031FixtureFamily::Recovery
        | Spec031FixtureFamily::Context
        | Spec031FixtureFamily::Extension
        | Spec031FixtureFamily::Delivery
        | Spec031FixtureFamily::ReleaseEvidence => {
            spec031_project_owner_record(canonical_owner_record_input(family)?)
        }
    }
}
