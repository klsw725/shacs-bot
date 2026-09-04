use super::*;

pub(in crate::runtime::spec034_release) struct RetiredToolchain {
    cargo: PortableToolIdentity,
    rustc: PortableToolIdentity,
    binding_digest: String,
}

impl ResolvedToolchain {
    pub fn cargo_identity(&self) -> &PortableToolIdentity {
        self.cargo.identity()
    }

    pub fn rustc_identity(&self) -> &PortableToolIdentity {
        self.rustc.identity()
    }

    pub fn verify(&self) -> Result<(), Spec034ReleaseArtifactError> {
        self.ledger.verify()?;
        self.cargo.verify()?;
        self.rustc.verify()?;
        self.rustdoc.verify()?;
        for seal in &self.linker_seals {
            seal.verify()?;
        }
        if let Some(binding) = &self.cache_binding {
            binding.verify()?;
        }
        self.execution.verify()?;
        self.linker_receipts.verify()?;
        self.ledger.verify()
    }

    #[cfg(not(test))]
    pub(in crate::runtime::spec034_release) fn verify_execution_ledger(&self) -> Result<(), Spec034ReleaseArtifactError> {
        self.linker_receipts.verify()?;
        self.ledger.verify()
    }

    #[cfg(not(test))]
    pub(in crate::runtime::spec034_release) fn verify_descendant_identity(
        &self,
        identity: &super::spawn::ProcessIdentity,
    ) -> Result<(), Spec034ReleaseArtifactError> {
        self.linker_receipts.verify_identity(identity)
    }

    pub(in crate::runtime::spec034_release) fn linker_attestation_digest(&self) -> Result<String, Spec034ReleaseArtifactError> {
        self.linker_receipts.attestation_digest()
    }

    pub(in crate::runtime::spec034_release) fn binding_digest(&self) -> Result<String, Spec034ReleaseArtifactError> {
        self.verify()?;
        let bytes = serde_json::to_vec(&(
            self.cargo.identity(),
            self.rustc.identity(),
            self.rustdoc.identity(),
            &self.cargo.runtime_inventory,
            &self.rustc.runtime_inventory,
            &self.rustdoc.runtime_inventory,
            self.cache_binding.as_ref().map(|binding| (
                &binding.source_closure_digest,
                &binding.manifest_digest,
                &binding.tree_digest,
            )),
            self.linker_attestation_digest()?,
            #[cfg(not(test))]
            self.vendor_binding.as_ref().map(|binding| (
                &binding.lock_digest,
                &binding.tree_digest,
                &binding.inventory_digest,
            )),
        ))
        .map_err(Spec034ReleaseArtifactError::Json)?;
        Ok(digest_bytes(&bytes))
    }

    pub(in crate::runtime::spec034_release) fn retire(
        self,
    ) -> Result<RetiredToolchain, Spec034ReleaseArtifactError> {
        let retired = RetiredToolchain {
            cargo: self.cargo_identity().clone(),
            rustc: self.rustc_identity().clone(),
            binding_digest: self.binding_digest()?,
        };
        drop(self);
        Ok(retired)
    }
}

impl RetiredToolchain {
    pub(in crate::runtime::spec034_release) fn cargo_identity(&self) -> &PortableToolIdentity {
        &self.cargo
    }

    pub(in crate::runtime::spec034_release) fn rustc_identity(&self) -> &PortableToolIdentity {
        &self.rustc
    }

    pub(in crate::runtime::spec034_release) fn binding_digest(&self) -> &str {
        &self.binding_digest
    }
}
