#[path = "../examples/spec034_sequential_integration_fixture/scenario.rs"]
mod scenario;

use shacs_projection::SPEC034_REQUIREMENTS;
use std::collections::BTreeSet;
use std::error::Error;

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
        "034-MH008",
        &[
            "states",
            "stored_provenance",
            "generated_provenance",
            "minimum_fields",
        ],
    ),
    (
        "034-MH009",
        &[
            "runtime_injection",
            "source",
            "sandbox",
            "credential",
            "disclosure",
            "snapshot",
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
        "034-AC007",
        &[
            "included",
            "unsupported",
            "extraction_failed",
            "analyzer_missing",
            "truncated",
            "unavailable",
        ],
    ),
    ("034-AC008", &["injected", "missing", "codec", "duration"]),
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
    let current_dir = std::env::current_dir()?;
    let repo = current_dir
        .ancestors()
        .find(|path| path.join("crates/Cargo.toml").is_file())
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
    let canonical_ids = SPEC034_REQUIREMENTS
        .iter()
        .map(|requirement| requirement.id)
        .collect::<Vec<_>>();

    // Then
    assert_eq!(report.receipts.len(), 22);
    assert!(report.is_complete());
    assert_eq!(receipt_ids, canonical_ids);
    assert_eq!(receipt_ids.iter().collect::<BTreeSet<_>>().len(), 22);
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
    assert!(report.analyzer.codec_unsupported);
    assert!(report.analyzer.duration_capped);
    assert!(report.edit.replacement_revalidated);
    assert!(report.crash.before_rename_hidden_and_clean);
    assert!(report.crash.after_rename_recovered);
    assert_eq!(report.replay.probe_counts, [1, 1, 1, 1]);
    assert_eq!(report.replay.replay_counts, [0, 0, 0, 0]);
    assert!(report.surfaces.semantic_parity);
    assert!(!report.surfaces.http_websocket_raw_equal);
    assert!(report.surfaces.cli_diff.is_empty());
    assert!(report.surfaces.http_diff.is_empty());
    assert!(report.surfaces.websocket_diff.is_empty());
    assert!(report.surfaces.channel_diff.is_empty());
    assert!(report.surfaces.tui_diff.is_empty());
    assert!(!report.secret_scan.inputs.is_empty());
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
    assert_eq!(
        report
            .surfaces
            .states
            .keys()
            .copied()
            .collect::<BTreeSet<_>>(),
        [
            "included",
            "unsupported",
            "extraction_failed",
            "analyzer_missing",
            "truncated",
            "unavailable",
        ]
        .into_iter()
        .collect()
    );
    assert!(report
        .surfaces
        .states
        .values()
        .all(|state| state.all_empty()));
    assert!(report.cleanup);
    Ok(())
}
