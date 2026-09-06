use super::super::model::Spec034ReleaseArtifactError;
use super::{validate_locator, ConfinedSourceReader};
use sha2::{Digest, Sha256};
use std::fs::File;
use std::path::Path;

pub(super) struct FixtureBinding {
    rows: Vec<(String, String)>,
}

impl FixtureBinding {
    pub(super) fn capture(
        live_handle: &File,
        controlled_root: &Path,
        fixtures: &[&str],
    ) -> Result<Self, Spec034ReleaseArtifactError> {
        let reader = reader(live_handle)?;
        let mut rows = Vec::with_capacity(fixtures.len());
        for locator in fixtures {
            validate_locator(locator)?;
            let bytes = reader
                .read(locator, 8 * 1024 * 1024)?
                .ok_or(Spec034ReleaseArtifactError::InvalidEvidence)?;
            let target = controlled_root.join(locator);
            let parent = target
                .parent()
                .ok_or(Spec034ReleaseArtifactError::InvalidConfig)?;
            std::fs::create_dir_all(parent).map_err(Spec034ReleaseArtifactError::Io)?;
            match std::fs::read(&target) {
                Ok(existing) if existing == bytes => {}
                Ok(_) => return Err(Spec034ReleaseArtifactError::DigestMismatch),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    super::super::artifacts::durable_write(&target, &bytes)?;
                }
                Err(error) => return Err(Spec034ReleaseArtifactError::Io(error)),
            }
            rows.push((
                (*locator).to_owned(),
                super::super::artifacts::digest_bytes(&bytes),
            ));
        }
        Ok(Self { rows })
    }

    pub(super) fn verify(&self, live_handle: &File) -> Result<(), Spec034ReleaseArtifactError> {
        let reader = reader(live_handle)?;
        for (locator, expected) in &self.rows {
            let bytes = reader
                .read(locator, 8 * 1024 * 1024)?
                .ok_or(Spec034ReleaseArtifactError::DigestMismatch)?;
            if super::super::artifacts::digest_bytes(&bytes) != *expected {
                return Err(Spec034ReleaseArtifactError::DigestMismatch);
            }
        }
        Ok(())
    }

    pub(super) fn digest(&self) -> String {
        let mut digest = Sha256::new();
        digest.update(b"spec034.live-fixture-binding.v1\0");
        for (locator, content_digest) in &self.rows {
            digest.update(locator.as_bytes());
            digest.update([0]);
            digest.update(content_digest.as_bytes());
            digest.update([0]);
        }
        format!("sha256:{:x}", digest.finalize())
    }
}

fn reader(live_handle: &File) -> Result<ConfinedSourceReader, Spec034ReleaseArtifactError> {
    Ok(ConfinedSourceReader::from_root(
        live_handle
            .try_clone()
            .map_err(Spec034ReleaseArtifactError::Io)?,
    ))
}
