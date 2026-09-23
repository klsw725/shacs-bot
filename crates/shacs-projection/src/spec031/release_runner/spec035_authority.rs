use super::model::Spec031ReleaseArtifactError as Error;
use super::spec035_catalog::{catalog, Requirement, PRDS, SPEC_ROOT};
use std::path::Path;

pub(super) fn catalog_at(repo: &Path) -> Result<Vec<Requirement>, Error> {
    let mut rows = catalog();
    for (prefix, section, count) in [
        ("must", "Must Have", 13),
        ("acceptance", "Acceptance Criteria", 12),
        ("closure", "Closure Evidence", 10),
    ] {
        let path = format!("{SPEC_ROOT}/SPEC.md");
        let lines = numbered_section(repo, &path, section, count)?;
        for (index, line) in lines.into_iter().enumerate() {
            let id = format!("spec035:{prefix}:{:02}", index + 1);
            let row = rows
                .iter_mut()
                .find(|row| row.id == id)
                .ok_or(Error::UnmappedCoverageRequirement)?;
            row.source_locator = format!("{path}:{line}");
        }
    }
    for (prd, authority) in PRDS.iter().enumerate() {
        let path = format!("{SPEC_ROOT}/prds/{}", authority.file);
        let lines = numbered_section(repo, &path, authority.section, authority.count)?;
        for (index, line) in lines.into_iter().enumerate() {
            let id = format!("spec035:PRD{prd:03}-{}-{}", authority.tag, index + 1);
            let row = rows
                .iter_mut()
                .find(|row| row.id == id)
                .ok_or(Error::UnmappedCoverageRequirement)?;
            row.source_locator = format!("{path}:{line}");
        }
    }
    Ok(rows)
}

fn numbered_section(
    repo: &Path,
    path: &str,
    section: &str,
    count: usize,
) -> Result<Vec<usize>, Error> {
    let text = std::fs::read_to_string(super::validate::require_safe_file(repo, path)?)
        .map_err(|_| Error::MissingRequiredArtifact)?;
    let mut found = false;
    let mut active = false;
    let mut rows = Vec::new();
    for (offset, line) in text.lines().enumerate() {
        let trimmed = line.trim();
        if let Some(heading) = trimmed.strip_prefix("## ") {
            active = heading.trim() == section;
            if active && found {
                return Err(Error::DuplicateCoverageRequirement);
            }
            found |= active;
            continue;
        }
        if !active || trimmed.is_empty() {
            continue;
        }
        let (number, body) = trimmed.split_once('.').unwrap_or((trimmed, ""));
        if !number.is_empty() && number.bytes().all(|byte| byte.is_ascii_digit()) {
            if number != (rows.len() + 1).to_string()
                || !body.starts_with(char::is_whitespace)
                || body.trim().is_empty()
            {
                return Err(Error::InvalidCoverageEvidence);
            }
            rows.push(offset + 1);
        }
    }
    if !found || rows.len() != count {
        return Err(Error::UnmappedCoverageRequirement);
    }
    Ok(rows)
}
