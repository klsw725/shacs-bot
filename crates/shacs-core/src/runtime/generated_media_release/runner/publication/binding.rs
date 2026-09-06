use super::super::super::model::{
    CleanupReceipt, DigestRow, SourceManifest, Spec034ReleaseArtifactError,
};
use super::super::super::source::SourceRootContext;
use sha2::{Digest, Sha256};

pub(in crate::runtime::generated_media_release::runner) enum FinalSourceBinding {
    Runner {
        source_root: Box<SourceRootContext>,
        source: SourceManifest,
        fixture_digests: Vec<DigestRow>,
        toolchain: Box<super::super::super::tools::RetiredToolchain>,
        cleanup: super::super::isolation::CompletedIsolationCleanup,
        cleanup_receipt: Box<CleanupReceipt>,
    },
    #[cfg(test)]
    Fixture,
    #[cfg(test)]
    ToolchainFixture(Box<super::super::super::tools::ResolvedToolchain>),
}

impl FinalSourceBinding {
    pub(in crate::runtime::generated_media_release::runner) fn runner(
        source_root: SourceRootContext,
        source: SourceManifest,
        fixture_digests: Vec<DigestRow>,
        toolchain: super::super::super::tools::RetiredToolchain,
        cleanup: super::super::isolation::CompletedIsolationCleanup,
        cleanup_receipt: CleanupReceipt,
    ) -> Self {
        Self::Runner {
            source_root: Box::new(source_root),
            source,
            fixture_digests,
            toolchain: Box::new(toolchain),
            cleanup,
            cleanup_receipt: Box::new(cleanup_receipt),
        }
    }

    pub(super) fn verify(&self) -> Result<(), Spec034ReleaseArtifactError> {
        match self {
            Self::Runner {
                source_root,
                source,
                fixture_digests,
                cleanup,
                cleanup_receipt,
                toolchain: _,
            } => {
                cleanup.verify_receipt(cleanup_receipt)?;
                source_root.verify()?;
                let _ = source;
                super::super::fixture::validate(source_root.root(), fixture_digests)?;
                source_root.verify()
            }
            #[cfg(test)]
            Self::Fixture => Ok(()),
            #[cfg(test)]
            Self::ToolchainFixture(toolchain) => toolchain.verify(),
        }
    }

    pub(super) fn capture_digest(&self) -> Result<String, Spec034ReleaseArtifactError> {
        self.verify()?;
        let mut digest = Sha256::new();
        match self {
            Self::Runner {
                source_root,
                source,
                fixture_digests,
                toolchain,
                cleanup,
                cleanup_receipt,
                ..
            } => {
                digest.update(b"runner\0");
                digest.update(
                    serde_json::to_vec(&(source, fixture_digests, cleanup_receipt))
                        .map_err(Spec034ReleaseArtifactError::Json)?,
                );
                digest.update(source_root.binding_digest().as_bytes());
                digest.update(toolchain.binding_digest().as_bytes());
                digest.update(cleanup.binding_digest().as_bytes());
            }
            #[cfg(test)]
            Self::Fixture => digest.update(b"fixture\0"),
            #[cfg(test)]
            Self::ToolchainFixture(toolchain) => {
                digest.update(b"toolchain-fixture\0");
                digest.update(toolchain.binding_digest()?.as_bytes());
            }
        }
        Ok(format!("sha256:{:x}", digest.finalize()))
    }

    #[cfg(test)]
    pub(super) const fn fixture() -> Self {
        Self::Fixture
    }

    #[cfg(test)]
    pub(super) fn toolchain_fixture(
        toolchain: super::super::super::tools::ResolvedToolchain,
    ) -> Self {
        Self::ToolchainFixture(Box::new(toolchain))
    }
}
