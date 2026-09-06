use super::super::artifacts::ArtifactSnapshot;
use super::super::model::{CommandEvidence, ResultsDocument, Spec034ReleaseArtifactError};
use super::super::tools::ResolvedToolchain;
use super::{command_specs, command_validation, generation};

pub(super) struct FreshExecutionAttestation {
    source_digest: String,
    commands: Vec<CommandEvidence>,
    linker_digest: String,
}

impl FreshExecutionAttestation {
    pub(super) fn from_commands(
        source_digest: &str,
        commands: &[CommandEvidence],
        linker_digest: String,
    ) -> Result<Self, Spec034ReleaseArtifactError> {
        if commands.len() != command_specs::COMMAND_SPECS.len()
            || commands.iter().any(|command| {
                !generation::command_passed(command) || !command.portable_process_receipt.reaped
            })
        {
            return Err(Spec034ReleaseArtifactError::CommandFailed);
        }
        Ok(Self {
            source_digest: source_digest.to_owned(),
            commands: commands.to_vec(),
            linker_digest,
        })
    }

    pub(super) fn verify_snapshot(
        &self,
        source_digest: &str,
        snapshot: &ArtifactSnapshot,
        toolchain: &ResolvedToolchain,
    ) -> Result<(), Spec034ReleaseArtifactError> {
        let results: ResultsDocument = snapshot.json("results.json")?;
        command_validation::validate_resolved(snapshot, &results.commands, source_digest, toolchain)?;
        let linker_digest = toolchain.linker_attestation_digest()?;
        (self.source_digest == source_digest
            && self.commands == results.commands
            && self.linker_digest == linker_digest)
            .then_some(())
            .ok_or(Spec034ReleaseArtifactError::InvalidEvidence)
    }
}
