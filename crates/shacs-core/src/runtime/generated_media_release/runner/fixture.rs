use super::super::artifacts::digest_bytes;
use super::super::catalog;
use super::super::model::{DigestRow, Spec034ReleaseArtifactError};
use super::super::source::{self, ConfinedSourceReader};
use std::path::Path;

const MAX_FIXTURE_BYTES: u64 = 8 * 1024 * 1024;

pub(super) fn digests(repo: &Path) -> Result<Vec<DigestRow>, Spec034ReleaseArtifactError> {
    digests_with(repo, |_| {})
}

pub(super) fn digests_from_source(
    source: &source::SourceSnapshot,
) -> Result<Vec<DigestRow>, Spec034ReleaseArtifactError> {
    catalog::FIXTURES
        .iter()
        .map(|locator| {
            let bytes = source
                .bytes(locator)
                .ok_or(Spec034ReleaseArtifactError::InvalidEvidence)?;
            Ok(DigestRow {
                locator: (*locator).to_owned(),
                digest: digest_bytes(bytes),
            })
        })
        .collect()
}

fn digests_with(
    repo: &Path,
    after_file_open: impl FnMut(&str),
) -> Result<Vec<DigestRow>, Spec034ReleaseArtifactError> {
    digests_with_hooks(repo, || {}, after_file_open)
}

fn digests_with_hooks(
    repo: &Path,
    after_root_open: impl FnOnce(),
    mut after_file_open: impl FnMut(&str),
) -> Result<Vec<DigestRow>, Spec034ReleaseArtifactError> {
    let root = repo
        .canonicalize()
        .map_err(Spec034ReleaseArtifactError::Io)?;
    let reader = ConfinedSourceReader::open(&root)?;
    after_root_open();
    catalog::FIXTURES
        .iter()
        .map(|locator| {
            source::validate_locator(locator)?;
            let bytes = reader
                .read_with_hooks(
                    locator,
                    MAX_FIXTURE_BYTES,
                    || {},
                    || after_file_open(locator),
                )?
                .ok_or(Spec034ReleaseArtifactError::InvalidEvidence)?;
            Ok(DigestRow {
                locator: (*locator).to_owned(),
                digest: digest_bytes(&bytes),
            })
        })
        .collect()
}

pub(super) fn validate(
    repo: &Path,
    expected: &[DigestRow],
) -> Result<(), Spec034ReleaseArtifactError> {
    (digests(repo)? == expected)
        .then_some(())
        .ok_or(Spec034ReleaseArtifactError::InvalidEvidence)
}

#[cfg(test)]
#[path = "fixture_test.rs"]
mod tests;
