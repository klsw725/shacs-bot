use super::*;
use serde::de::DeserializeOwned;
use std::collections::BTreeSet;
use std::fmt::{Display, Formatter};
use std::path::{Component, Path};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Spec034SchemaError {
    MalformedJson,
    StaleSchemaVersion,
    MissingOwnerFact,
    DuplicateOwnerFact,
    InvalidOwnerFact,
    MissingReview,
    DuplicateReview,
    FailedReview,
    FailedCargoCommand,
    MissingRequirement,
    DuplicateRequirement,
    UnknownRequirement,
    IncorrectPrimaryPrd,
    InvalidEvidence,
    DirtyWorktree,
    OpenBlocker,
    CleanupIncomplete,
    InvalidReleaseSource,
    DuplicateBlocker,
}

impl Display for Spec034SchemaError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for Spec034SchemaError {}

pub fn parse_spec034_owner_facts(input: &str) -> Result<Spec034OwnerFacts, Spec034SchemaError> {
    let facts = parse_json(input)?;
    validate_owner_facts(&facts)?;
    Ok(facts)
}

pub fn parse_spec034_review_evidence(
    input: &str,
) -> Result<Spec034ReviewEvidence, Spec034SchemaError> {
    let reviews = parse_json(input)?;
    validate_reviews(&reviews)?;
    Ok(reviews)
}

pub fn parse_spec034_release_evidence(
    input: &str,
) -> Result<Spec034ReleaseEvidence, Spec034SchemaError> {
    let release = parse_json(input)?;
    validate_release(&release)?;
    Ok(release)
}

fn parse_json<T: DeserializeOwned>(input: &str) -> Result<T, Spec034SchemaError> {
    serde_json::from_str(input).map_err(|_| Spec034SchemaError::MalformedJson)
}

fn validate_release(release: &Spec034ReleaseEvidence) -> Result<(), Spec034SchemaError> {
    if release.schema != SPEC034_RELEASE_SCHEMA {
        return Err(Spec034SchemaError::StaleSchemaVersion);
    }
    validate_identifier(&release.run_id)?;
    validate_source(&release.source)?;
    validate_owner_facts(&release.owner_facts)?;
    if release
        .owner_facts
        .facts
        .iter()
        .any(|fact| fact.availability == Spec034Availability::Unavailable)
    {
        return Err(Spec034SchemaError::MissingOwnerFact);
    }
    validate_reviews(&release.review_evidence)?;
    validate_requirements(&release.requirements)?;
    validate_blockers(&release.blockers)?;
    if !release.cleanup_complete {
        return Err(Spec034SchemaError::CleanupIncomplete);
    }
    Ok(())
}

fn validate_owner_facts(facts: &Spec034OwnerFacts) -> Result<(), Spec034SchemaError> {
    if facts.schema != SPEC034_OWNER_FACTS_SCHEMA {
        return Err(Spec034SchemaError::StaleSchemaVersion);
    }
    let kinds = facts
        .facts
        .iter()
        .map(|fact| fact.kind)
        .collect::<BTreeSet<_>>();
    if kinds.len() != facts.facts.len() {
        return Err(Spec034SchemaError::DuplicateOwnerFact);
    }
    if kinds != Spec034OwnerFactKind::required().into_iter().collect() {
        return Err(Spec034SchemaError::MissingOwnerFact);
    }
    for fact in &facts.facts {
        match fact.availability {
            Spec034Availability::Available => {
                if fact.evidence.is_empty() || fact.unavailable_reason.is_some() {
                    return Err(Spec034SchemaError::InvalidOwnerFact);
                }
                validate_evidence_list(&fact.evidence)?;
            }
            Spec034Availability::Unavailable => {
                if !fact.evidence.is_empty() || fact.unavailable_reason.is_none() {
                    return Err(Spec034SchemaError::InvalidOwnerFact);
                }
            }
        }
    }
    Ok(())
}

fn validate_reviews(reviews: &Spec034ReviewEvidence) -> Result<(), Spec034SchemaError> {
    if reviews.schema != SPEC034_REVIEW_SCHEMA {
        return Err(Spec034SchemaError::StaleSchemaVersion);
    }
    let kinds = reviews
        .reviews
        .iter()
        .map(|review| review.kind)
        .collect::<BTreeSet<_>>();
    if kinds.len() != reviews.reviews.len() {
        return Err(Spec034SchemaError::DuplicateReview);
    }
    if kinds != Spec034ReviewKind::required().into_iter().collect() {
        return Err(Spec034SchemaError::MissingReview);
    }
    for review in &reviews.reviews {
        if review.verdict != Spec034ReviewVerdict::Pass || !review.final_review {
            return Err(Spec034SchemaError::FailedReview);
        }
        validate_evidence_list(&review.evidence)?;
    }
    if reviews.cargo_commands.is_empty() {
        return Err(Spec034SchemaError::FailedCargoCommand);
    }
    for command in &reviews.cargo_commands {
        if command.argv.first().map(String::as_str) != Some("cargo")
            || command.argv.iter().any(|argument| {
                argument.contains(['\n', '\0']) || Path::new(argument).is_absolute()
            })
            || !command.passed
            || command.exit_code != 0
        {
            return Err(Spec034SchemaError::FailedCargoCommand);
        }
        validate_evidence(&command.evidence)?;
    }
    Ok(())
}

fn validate_requirements(rows: &[Spec034RequirementCoverage]) -> Result<(), Spec034SchemaError> {
    let mut seen = BTreeSet::new();
    for row in rows {
        let expected = SPEC034_REQUIREMENTS
            .iter()
            .find(|requirement| requirement.id == row.requirement_id)
            .ok_or(Spec034SchemaError::UnknownRequirement)?;
        if !seen.insert(row.requirement_id.as_str()) {
            return Err(Spec034SchemaError::DuplicateRequirement);
        }
        if row.primary_prd != expected.primary_prd {
            return Err(Spec034SchemaError::IncorrectPrimaryPrd);
        }
        validate_evidence_list(&row.evidence)?;
    }
    if seen.len() != SPEC034_REQUIREMENTS.len() {
        return Err(Spec034SchemaError::MissingRequirement);
    }
    Ok(())
}

fn validate_blockers(blockers: &[Spec034BlockerRecord]) -> Result<(), Spec034SchemaError> {
    let mut seen = BTreeSet::new();
    for blocker in blockers {
        if !seen.insert((blocker.kind, blocker.requirement_id.as_deref())) {
            return Err(Spec034SchemaError::DuplicateBlocker);
        }
        if blocker.disposition == Spec034BlockerDisposition::Open {
            return Err(Spec034SchemaError::OpenBlocker);
        }
        if let Some(requirement_id) = &blocker.requirement_id {
            if !SPEC034_REQUIREMENTS
                .iter()
                .any(|requirement| requirement.id == requirement_id)
            {
                return Err(Spec034SchemaError::UnknownRequirement);
            }
        }
        validate_evidence_list(&blocker.evidence)?;
    }
    Ok(())
}

fn validate_source(source: &Spec034SourceEvidence) -> Result<(), Spec034SchemaError> {
    if !matches!(source.head_oid.len(), 40 | 64)
        || !source.head_oid.bytes().all(|byte| byte.is_ascii_hexdigit())
        || !valid_digest(&source.source_digest)
    {
        return Err(Spec034SchemaError::InvalidReleaseSource);
    }
    if !source.worktree_clean {
        return Err(Spec034SchemaError::DirtyWorktree);
    }
    Ok(())
}

fn validate_evidence_list(evidence: &[Spec034EvidenceRef]) -> Result<(), Spec034SchemaError> {
    if evidence.is_empty() {
        return Err(Spec034SchemaError::InvalidEvidence);
    }
    evidence.iter().try_for_each(validate_evidence)
}

fn validate_evidence(evidence: &Spec034EvidenceRef) -> Result<(), Spec034SchemaError> {
    let path = Path::new(&evidence.locator);
    if evidence.locator.is_empty()
        || path.is_absolute()
        || path
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
        || !valid_digest(&evidence.digest)
    {
        return Err(Spec034SchemaError::InvalidEvidence);
    }
    Ok(())
}

fn validate_identifier(value: &str) -> Result<(), Spec034SchemaError> {
    if value.is_empty()
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b':'))
    {
        return Err(Spec034SchemaError::InvalidReleaseSource);
    }
    Ok(())
}

fn valid_digest(value: &str) -> bool {
    value.len() == 71
        && value.starts_with("sha256:")
        && value[7..].bytes().all(|byte| byte.is_ascii_hexdigit())
}
