#[path = "../examples/spec034_sequential_integration_fixture/scenario.rs"]
mod scenario;

use shacs_projection::{spec034_current_requirements, SPEC034_RETIRED_REQUIREMENTS};
use std::collections::BTreeSet;
use std::error::Error;
use std::path::Path;

const EXPANDED_RECEIPTS: &[(&str, &[&str])] = &[
    (
        "034-MH002",
        &["edit", "mask", "variation", "raw_options_bounded"],
    ),
    (
        "034-MH003",
        &[
            "path_traversal",
            "mime",
            "size",
            "provenance",
            "replacement",
        ],
    ),
    (
        "034-MH006",
        &[
            "identity",
            "media",
            "origin",
            "sources",
            "options",
            "lifecycle",
            "disclosure",
        ],
    ),
    (
        "034-AC002",
        &[
            "source_mime",
            "source_size",
            "source_provenance",
            "mask_mime",
            "mask_size",
            "path_traversal",
        ],
    ),
    (
        "034-AC004",
        &[
            "initial_guard",
            "redirect_guard",
            "scheme",
            "byte_cap",
            "mime_cap",
            "outcomes",
            "credential_omission",
        ],
    ),
    (
        "034-AC005",
        &[
            "metadata",
            "digest",
            "source_chain",
            "retention",
            "disclosure",
        ],
    ),
    (
        "034-AC010",
        &[
            "policy_identity",
            "unsupported_claims_false",
            "scoped_non_guarantees",
            "spec_complete_scoped",
        ],
    ),
];

#[test]
fn documentation_policy_observes_complete_scoped_contract() -> Result<(), Box<dyn Error>> {
    // Given
    let repo = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .ok_or("repository root not found")?;

    // When
    let report = scenario::docs_policy::run(repo)?;

    // Then
    assert_eq!(
        report
            .sub_observations
            .iter()
            .map(|observation| observation.name)
            .collect::<Vec<_>>(),
        [
            "policy_identity",
            "unsupported_claims_false",
            "scoped_non_guarantees",
            "spec_complete_scoped",
        ]
    );
    assert!(report.is_complete());
    Ok(())
}

#[test]
fn sequential_receipts_link_every_requirement_to_observed_production_behavior(
) -> Result<(), Box<dyn Error>> {
    // Given / When
    let report = scenario::run()?;
    let receipt_ids = report
        .receipts
        .iter()
        .map(|receipt| receipt.requirement_id)
        .collect::<Vec<_>>();
    let canonical_ids = spec034_current_requirements()
        .map(|requirement| requirement.id)
        .collect::<Vec<_>>();

    // Then
    assert_eq!(report.receipts.len(), 13);
    assert!(report.is_complete());
    assert_eq!(receipt_ids, canonical_ids);
    assert_eq!(receipt_ids.iter().collect::<BTreeSet<_>>().len(), 13);
    assert_eq!(report.retired_requirements, SPEC034_RETIRED_REQUIREMENTS);
    assert!(receipt_ids
        .iter()
        .all(|id| !SPEC034_RETIRED_REQUIREMENTS.contains(id)));
    assert!(report
        .receipts
        .iter()
        .all(|receipt| !receipt.name.is_empty()
            && !receipt.production_source.is_empty()
            && !receipt.observable.is_null()));
    assert!(report.catalog_validated_after_observation);
    assert_eq!(
        report.lifecycle_states,
        ["started", "partial", "final", "failed", "cancelled"]
    );
    assert!(report.edit.replacement_revalidated);
    assert!(report.crash.before_rename_hidden_and_clean);
    assert!(report.crash.after_rename_recovered);
    assert_eq!(
        report
            .secret_scan
            .inputs
            .iter()
            .map(|input| input.name.as_str())
            .collect::<Vec<_>>(),
        ["artifact", "edit", "remote"]
    );
    assert_eq!(report.secret_scan.forbidden_classes.len(), 8);
    assert!(report.secret_scan.matches.is_empty());
    assert!(report.adversarial.all_observed());
    for (requirement_id, expected_names) in EXPANDED_RECEIPTS {
        let receipt = report
            .receipts
            .iter()
            .find(|receipt| receipt.requirement_id == *requirement_id)
            .ok_or("expanded receipt missing")?;
        let observations = receipt.observable["sub_observations"]
            .as_array()
            .ok_or("sub-observation table missing")?;
        let names = observations
            .iter()
            .filter_map(|observation| observation["name"].as_str())
            .collect::<BTreeSet<_>>();
        assert_eq!(names, expected_names.iter().copied().collect());
        assert!(observations
            .iter()
            .all(|observation| observation["observed"] == true));
    }
    assert!(report.cleanup);
    Ok(())
}
