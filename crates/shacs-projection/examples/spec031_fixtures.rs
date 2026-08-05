use shacs_projection::{
    spec031_canonical_fixture_registry, spec031_missing_external_owner_evidence,
    Spec031FixtureFamily, Spec031ReasonCode,
};
use std::{error::Error, io};

fn main() -> Result<(), Box<dyn Error>> {
    let registry = spec031_canonical_fixture_registry()?;
    let mut missing_owner = Vec::new();

    for family in [
        Spec031FixtureFamily::ExternalAppOwner,
        Spec031FixtureFamily::ExternalMediaOwner,
        Spec031FixtureFamily::Readiness,
    ] {
        let envelope = spec031_missing_external_owner_evidence(family)?;
        if envelope.state() == shacs_projection::Spec031Availability::Ready {
            return Err(Box::new(io::Error::new(
                io::ErrorKind::InvalidData,
                "missing owner evidence projected ready",
            )));
        }
        if envelope.reason().code != Spec031ReasonCode::MissingExternalOwnerEvidence {
            return Err(Box::new(io::Error::new(
                io::ErrorKind::InvalidData,
                "missing owner evidence used the wrong typed reason code",
            )));
        }
        missing_owner.push(serde_json::json!({
            "family": format!("{:?}", family),
            "envelope": envelope,
        }));
    }

    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "registry": registry_json(&registry),
            "missing_owner": missing_owner,
        }))?
    );

    Ok(())
}

fn registry_json(registry: &[shacs_projection::Spec031CanonicalFixture]) -> Vec<serde_json::Value> {
    registry
        .iter()
        .map(|fixture| {
            serde_json::json!({
                "family": format!("{:?}", fixture.family()),
                "envelope": fixture.envelope(),
            })
        })
        .collect()
}
