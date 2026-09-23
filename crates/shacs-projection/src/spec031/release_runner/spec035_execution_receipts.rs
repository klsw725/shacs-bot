use super::model::Spec031ReleaseArtifactError as Error;
use super::spec035_execution_io::Evidence;
use super::spec035_execution_model::{
    AbsenceProof, Cleanup, Disposition, FileRef, IncidentDisposition, IncidentEvidence,
    IncidentReceipt, IncidentSchema, Receipt, Verdict,
};
use std::collections::HashSet;

pub(super) fn validate_incident(
    evidence: &Evidence<'_>,
    incident: &IncidentEvidence,
) -> Result<(), Error> {
    match incident.disposition {
        None => {
            validate_receipt(evidence, &incident.receipt, &incident.id)?;
        }
        Some(IncidentDisposition::UserAcceptedRepairedBaseline) => {
            let receipt: IncidentReceipt = evidence.json(&incident.receipt)?;
            match receipt.schema {
                IncidentSchema::V1 => {}
            }
            evidence.identity(&receipt.run_id, &receipt.source_sha256)?;
            if incident.id != "default-config-rewrite" || receipt.subject != incident.id {
                return Err(Error::InvalidCoverageEvidence);
            }
            super::spec035_admission_incident::accepted_incident(
                &evidence.bytes(&receipt.acceptance)?,
            )?;
        }
    }
    Ok(())
}

pub(super) fn validate_receipt(
    evidence: &Evidence<'_>,
    file: &FileRef,
    subject: &str,
) -> Result<Vec<String>, Error> {
    validate_receipt_phase(evidence, file, subject, false)
}

pub(super) fn validate_receipt_phase(
    evidence: &Evidence<'_>,
    file: &FileRef,
    subject: &str,
    pending_allowed: bool,
) -> Result<Vec<String>, Error> {
    let receipt: Receipt = evidence.json(file)?;
    evidence.identity(&receipt.run_id, &receipt.source_sha256)?;
    if receipt.subject != subject || receipt.checks.is_empty() {
        return Err(Error::InvalidCoverageEvidence);
    }
    if receipt.verdict == Verdict::Failed
        || (receipt.verdict == Verdict::Blocked && !pending_allowed)
    {
        return Err(Error::BlockedExternalEvidence);
    }
    let mut seen = HashSet::new();
    let mut commands = Vec::new();
    let mut pending = false;
    for check in receipt.checks {
        if check.id.is_empty() || !seen.insert(check.id) {
            return Err(Error::InvalidCoverageEvidence);
        }
        evidence.source_locator(&check.producer)?;
        match check.verdict {
            Verdict::Failed => return Err(Error::BlockedExternalEvidence),
            Verdict::Blocked => {
                if !pending_allowed || !check.commands.is_empty() || !check.artifacts.is_empty() {
                    return Err(Error::BlockedExternalEvidence);
                }
                pending = true;
                continue;
            }
            Verdict::Pass => {
                if check.commands.is_empty() || check.artifacts.is_empty() {
                    return Err(Error::InvalidCoverageEvidence);
                }
            }
        }
        for command in &check.commands {
            if !evidence
                .execution
                .commands
                .iter()
                .any(|record| record.id == *command)
            {
                return Err(Error::InvalidCommandEvidence);
            }
        }
        for artifact in check.artifacts {
            evidence.bytes(&artifact)?;
        }
        commands.extend(check.commands);
    }
    if pending != (receipt.verdict == Verdict::Blocked) {
        return Err(Error::InvalidCoverageEvidence);
    }
    Ok(commands)
}

pub(super) fn validate_cleanup(evidence: &Evidence<'_>) -> Result<(), Error> {
    let cleanup: Cleanup = evidence.json(&evidence.execution.cleanup)?;
    evidence.identity(&cleanup.run_id, &cleanup.source_sha256)?;
    if cleanup.verdict != Verdict::Pass {
        return Err(Error::BlockedExternalEvidence);
    }
    let required: HashSet<_> = evidence
        .execution
        .resources
        .iter()
        .map(String::as_str)
        .collect();
    if required.is_empty()
        || required.len() != evidence.execution.resources.len()
        || required.contains("")
    {
        return Err(Error::MissingCleanupReceipt);
    }
    let mut seen = HashSet::new();
    for resource in &cleanup.resources {
        if !required.contains(resource.id.as_str()) || !seen.insert(resource.id.as_str()) {
            return Err(Error::MissingCleanupReceipt);
        }
        match resource.disposition {
            Disposition::Removed | Disposition::Stopped | Disposition::Absent => {}
        }
        let proof: AbsenceProof = evidence.json(&resource.proof)?;
        evidence.identity(&proof.run_id, &proof.source_sha256)?;
        if proof.resource_id != resource.id || !proof.absent {
            return Err(Error::MissingCleanupReceipt);
        }
    }
    if seen != required {
        return Err(Error::MissingCleanupReceipt);
    }
    Ok(())
}
