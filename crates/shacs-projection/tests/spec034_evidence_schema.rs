use shacs_projection::{
    parse_spec034_owner_facts, parse_spec034_release_evidence, Spec034BlockerDisposition,
    Spec034BlockerKind, Spec034BlockerRecord, Spec034CargoCommandEvidence, Spec034EvidenceRef,
    Spec034OwnerFactKind, Spec034OwnerFactRecord, Spec034OwnerFacts, Spec034PrimaryPrd,
    Spec034ReleaseEvidence, Spec034RequirementCoverage, Spec034ReviewEvidence, Spec034ReviewKind,
    Spec034ReviewRecord, Spec034ReviewVerdict, Spec034SchemaError, Spec034SourceEvidence,
    Spec034UnavailableReason, SPEC034_OWNER_FACTS_SCHEMA, SPEC034_RELEASE_SCHEMA,
    SPEC034_REQUIREMENTS, SPEC034_REVIEW_SCHEMA,
};

#[test]
fn requirement_catalog_contains_exact_parent_ids_and_prd_mappings() {
    // Given
    let expected_ids = (1..=10)
        .map(|number| format!("034-MH{number:03}"))
        .chain((1..=12).map(|number| format!("034-AC{number:03}")))
        .collect::<Vec<_>>();

    // When
    let actual_ids = SPEC034_REQUIREMENTS
        .iter()
        .map(|requirement| requirement.id.to_owned())
        .collect::<Vec<_>>();

    // Then
    assert_eq!(actual_ids, expected_ids);
    assert_eq!(SPEC034_REQUIREMENTS.len(), 22);
    assert_eq!(
        SPEC034_REQUIREMENTS
            .iter()
            .map(|requirement| requirement.primary_prd)
            .collect::<Vec<_>>(),
        vec![
            Spec034PrimaryPrd::Prd000,
            Spec034PrimaryPrd::Prd001,
            Spec034PrimaryPrd::Prd001,
            Spec034PrimaryPrd::Prd001,
            Spec034PrimaryPrd::Prd002,
            Spec034PrimaryPrd::Prd002,
            Spec034PrimaryPrd::Prd002,
            Spec034PrimaryPrd::Prd003,
            Spec034PrimaryPrd::Prd003,
            Spec034PrimaryPrd::Prd002,
            Spec034PrimaryPrd::Prd000,
            Spec034PrimaryPrd::Prd001,
            Spec034PrimaryPrd::Prd001,
            Spec034PrimaryPrd::Prd002,
            Spec034PrimaryPrd::Prd002,
            Spec034PrimaryPrd::Prd002,
            Spec034PrimaryPrd::Prd003,
            Spec034PrimaryPrd::Prd003,
            Spec034PrimaryPrd::Prd002,
            Spec034PrimaryPrd::Prd004,
            Spec034PrimaryPrd::Prd003,
            Spec034PrimaryPrd::Prd003,
        ]
    );
}

#[test]
fn release_parser_rejects_missing_duplicate_and_unknown_requirement_ids() {
    // Given
    let release = valid_release();
    let mut missing = release.clone();
    missing.requirements.pop();
    let mut duplicate = release.clone();
    duplicate.requirements[1] = duplicate.requirements[0].clone();
    let mut unknown = release;
    unknown.requirements[0].requirement_id = "034-MH999".to_owned();

    // When
    let missing_result = parse_release(&missing);
    let duplicate_result = parse_release(&duplicate);
    let unknown_result = parse_release(&unknown);

    // Then
    assert_eq!(missing_result, Err(Spec034SchemaError::MissingRequirement));
    assert_eq!(
        duplicate_result,
        Err(Spec034SchemaError::DuplicateRequirement)
    );
    assert_eq!(unknown_result, Err(Spec034SchemaError::UnknownRequirement));
}

#[test]
fn release_parser_keeps_reviews_distinct_from_cargo_evidence() {
    // Given
    let mut missing_review = valid_release();
    missing_review.review_evidence.reviews.pop();
    let mut failed_cargo = valid_release();
    failed_cargo.review_evidence.cargo_commands[0].exit_code = 1;
    failed_cargo.review_evidence.cargo_commands[0].passed = true;

    // When
    let review_result = parse_release(&missing_review);
    let cargo_result = parse_release(&failed_cargo);

    // Then
    assert_eq!(review_result, Err(Spec034SchemaError::MissingReview));
    assert_eq!(cargo_result, Err(Spec034SchemaError::FailedCargoCommand));
}

#[test]
fn release_parser_rejects_stale_schema_dirty_source_and_open_blocker() {
    // Given
    let mut stale = valid_release();
    stale.schema = "spec034.release_evidence.v0".to_owned();
    let mut dirty = valid_release();
    dirty.source.worktree_clean = false;
    let mut blocked = valid_release();
    blocked.blockers[0].disposition = Spec034BlockerDisposition::Open;

    // When
    let stale_result = parse_release(&stale);
    let dirty_result = parse_release(&dirty);
    let blocked_result = parse_release(&blocked);

    // Then
    assert_eq!(stale_result, Err(Spec034SchemaError::StaleSchemaVersion));
    assert_eq!(dirty_result, Err(Spec034SchemaError::DirtyWorktree));
    assert_eq!(blocked_result, Err(Spec034SchemaError::OpenBlocker));
}

#[test]
fn release_parser_does_not_promote_unavailable_owner_fact_to_success() {
    // Given
    let mut release = valid_release();
    release.owner_facts.facts[0] = Spec034OwnerFactRecord::unavailable(
        Spec034OwnerFactKind::CurrentOsAuthority,
        Spec034UnavailableReason::NotRecorded,
    );

    // When
    let result = parse_release(&release);

    // Then
    assert_eq!(result, Err(Spec034SchemaError::MissingOwnerFact));
}

#[test]
fn owner_fact_round_trip_preserves_unavailable_without_inventing_authority() {
    // Given
    let mut facts = valid_owner_facts();
    facts.facts[0] = Spec034OwnerFactRecord::unavailable(
        Spec034OwnerFactKind::CurrentOsAuthority,
        Spec034UnavailableReason::NotRecorded,
    );

    // When
    let encoded = serde_json::to_string(&facts).expect("owner facts serialize");
    let parsed = parse_spec034_owner_facts(&encoded);

    // Then
    assert_eq!(parsed, Ok(facts));
}

#[test]
fn parsers_reject_malformed_json_and_unknown_fields() {
    // Given
    let malformed = "{not-json";
    let mut value = serde_json::to_value(valid_owner_facts()).expect("owner facts value");
    value
        .as_object_mut()
        .expect("owner facts object")
        .insert("authority".to_owned(), serde_json::json!(true));

    // When
    let malformed_result = parse_spec034_owner_facts(malformed);
    let unknown_result = parse_spec034_owner_facts(&value.to_string());

    // Then
    assert_eq!(malformed_result, Err(Spec034SchemaError::MalformedJson));
    assert_eq!(unknown_result, Err(Spec034SchemaError::MalformedJson));
}

fn parse_release(
    release: &Spec034ReleaseEvidence,
) -> Result<Spec034ReleaseEvidence, Spec034SchemaError> {
    let encoded = serde_json::to_string(release).expect("release serializes");
    parse_spec034_release_evidence(&encoded)
}

fn valid_release() -> Spec034ReleaseEvidence {
    Spec034ReleaseEvidence {
        schema: SPEC034_RELEASE_SCHEMA.to_owned(),
        run_id: "spec034-task-5".to_owned(),
        source: Spec034SourceEvidence {
            head_oid: "0123456789abcdef0123456789abcdef01234567".to_owned(),
            source_digest: digest(),
            worktree_clean: true,
        },
        owner_facts: valid_owner_facts(),
        review_evidence: valid_reviews(),
        requirements: SPEC034_REQUIREMENTS
            .iter()
            .map(|requirement| Spec034RequirementCoverage {
                requirement_id: requirement.id.to_owned(),
                primary_prd: requirement.primary_prd,
                evidence: vec![evidence()],
            })
            .collect(),
        blockers: vec![Spec034BlockerRecord {
            kind: Spec034BlockerKind::CleanupIncomplete,
            disposition: Spec034BlockerDisposition::Cleared,
            requirement_id: None,
            evidence: vec![evidence()],
        }],
        cleanup_complete: true,
    }
}

fn valid_owner_facts() -> Spec034OwnerFacts {
    Spec034OwnerFacts {
        schema: SPEC034_OWNER_FACTS_SCHEMA.to_owned(),
        facts: Spec034OwnerFactKind::required()
            .into_iter()
            .map(|kind| Spec034OwnerFactRecord::available(kind, vec![evidence()]))
            .collect(),
    }
}

fn valid_reviews() -> Spec034ReviewEvidence {
    Spec034ReviewEvidence {
        schema: SPEC034_REVIEW_SCHEMA.to_owned(),
        reviews: Spec034ReviewKind::required()
            .into_iter()
            .map(|kind| Spec034ReviewRecord {
                kind,
                verdict: Spec034ReviewVerdict::Pass,
                final_review: true,
                evidence: vec![evidence()],
            })
            .collect(),
        cargo_commands: vec![Spec034CargoCommandEvidence {
            argv: vec!["cargo".to_owned(), "test".to_owned()],
            exit_code: 0,
            passed: true,
            evidence: evidence(),
        }],
    }
}

fn evidence() -> Spec034EvidenceRef {
    Spec034EvidenceRef {
        locator: "evidence/task-5.json".to_owned(),
        digest: digest(),
    }
}

fn digest() -> String {
    format!("sha256:{}", "a".repeat(64))
}
