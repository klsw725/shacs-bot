use shacs_projection::spec030_integration_targets;
use std::collections::BTreeSet;

#[test]
fn runner_catalog_has_exact_unique_targets_and_prd006_integrity() {
    let targets = spec030_integration_targets();
    let ids = targets
        .iter()
        .map(|target| target.command_id)
        .collect::<BTreeSet<_>>();
    let names = targets
        .iter()
        .map(|target| (target.package, target.target))
        .collect::<BTreeSet<_>>();

    assert_eq!(ids.len(), targets.len());
    assert_eq!(names.len(), targets.len());
    assert!(targets.iter().any(|target| {
        target.target == "spec030_semantic_evidence" && target.prds.contains(&"006")
    }));
    assert!(targets
        .iter()
        .any(|target| { target.target == "spec030_integrity" && target.prds.contains(&"006") }));
}
