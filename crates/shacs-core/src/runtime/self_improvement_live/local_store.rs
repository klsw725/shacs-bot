use super::local_owner::state_path;
use super::{
    LocalApplyReceipt, LocalImprovementBlock, LocalImprovementProposal, LocalImprovementStatus,
    LocalRollbackCandidate, LocalRollbackReceipt,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct LocalRecord {
    #[serde(default)]
    pub recorded_sequence: u64,
    pub proposal: LocalImprovementProposal,
    pub apply: Option<LocalApplyReceipt>,
    pub verification_passed: Option<bool>,
    pub verification_evidence_id: Option<String>,
    pub rollback_candidate: Option<LocalRollbackCandidate>,
    pub rollback: Option<LocalRollbackReceipt>,
    pub checkpoint: Option<Vec<u8>>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct LocalDocument {
    schema_version: u32,
    #[serde(default)]
    next_sequence: u64,
    records: BTreeMap<String, LocalRecord>,
}

#[derive(Debug)]
pub struct LocalImprovementStore {
    path: PathBuf,
    state_root: Option<PathBuf>,
    document: Mutex<LocalDocument>,
}

impl LocalImprovementStore {
    pub(crate) fn has_durable_document(&self) -> bool {
        self.path.exists()
    }
    pub fn open(path: impl AsRef<Path>) -> Result<Self, LocalImprovementBlock> {
        let path = path.as_ref().to_path_buf();
        Self::open_path(path, None)
    }

    pub(crate) fn open_state(root: &Path) -> Result<Self, LocalImprovementBlock> {
        Self::open_path(state_path(root, "store.json")?, Some(root.to_path_buf()))
    }

    fn open_path(
        path: PathBuf,
        state_root: Option<PathBuf>,
    ) -> Result<Self, LocalImprovementBlock> {
        let document = match fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(LocalImprovementBlock::UnsafeTarget);
            }
            Ok(metadata) if metadata.is_file() => {
                serde_json::from_slice(&fs::read(&path).map_err(|_| LocalImprovementBlock::Io)?)
                    .map_err(|_| LocalImprovementBlock::Io)?
            }
            Ok(_) => return Err(LocalImprovementBlock::Io),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => LocalDocument {
                schema_version: 1,
                next_sequence: 0,
                records: BTreeMap::new(),
            },
            Err(_) => return Err(LocalImprovementBlock::Io),
        };
        if document.schema_version != 1 {
            return Err(LocalImprovementBlock::Io);
        }
        Ok(Self {
            path,
            state_root,
            document: Mutex::new(document),
        })
    }

    pub fn proposal(&self, id: &str) -> Option<LocalImprovementProposal> {
        lock(&self.document)
            .records
            .get(id)
            .map(|record| record.proposal.clone())
    }
    pub fn apply_receipt(&self, id: &str) -> Option<LocalApplyReceipt> {
        lock(&self.document)
            .records
            .get(id)
            .and_then(|record| record.apply.clone())
    }
    pub fn rollback_candidate(&self, id: &str) -> Option<LocalRollbackCandidate> {
        lock(&self.document)
            .records
            .get(id)
            .and_then(|record| record.rollback_candidate.clone())
    }

    pub fn record_rollback_candidate(
        &self,
        id: &str,
        evidence_id: &str,
    ) -> Result<(), LocalImprovementBlock> {
        self.mutate(|document| {
            let record = document
                .records
                .get_mut(id)
                .ok_or(LocalImprovementBlock::ProposalNotFound)?;
            match &record.rollback_candidate {
                Some(candidate) if candidate.verify_failure_ref() == evidence_id => Ok(()),
                Some(_) => Err(LocalImprovementBlock::RollbackUnavailable),
                None => {
                    record.rollback_candidate = Some(LocalRollbackCandidate {
                        verify_failure_id: evidence_id.to_owned(),
                    });
                    Ok(())
                }
            }
        })
    }

    pub fn latest_status(&self) -> Option<LocalImprovementStatus> {
        let document = lock(&self.document);
        let record = document
            .records
            .values()
            .max_by_key(|record| record.recorded_sequence)?;
        Some(LocalImprovementStatus {
            proposal: record.proposal.clone(),
            applied: record.apply.is_some(),
            verification_passed: record.verification_passed,
            verification_evidence_id: record.verification_evidence_id.clone(),
            rollback_candidate: record.rollback_candidate.clone(),
            rolled_back: record.rollback.is_some(),
        })
    }

    pub(crate) fn status(&self, id: &str) -> Result<LocalImprovementStatus, LocalImprovementBlock> {
        let document = lock(&self.document);
        let record = document
            .records
            .get(id)
            .ok_or(LocalImprovementBlock::ProposalNotFound)?;
        Ok(LocalImprovementStatus {
            proposal: record.proposal.clone(),
            applied: record.apply.is_some(),
            verification_passed: record.verification_passed,
            verification_evidence_id: record.verification_evidence_id.clone(),
            rollback_candidate: record.rollback_candidate.clone(),
            rolled_back: record.rollback.is_some(),
        })
    }

    pub(crate) fn insert(
        &self,
        proposal: LocalImprovementProposal,
    ) -> Result<(), LocalImprovementBlock> {
        self.mutate(|document| {
            if document.records.contains_key(proposal.proposal_id()) {
                return Err(LocalImprovementBlock::DuplicateProposal);
            }
            document.next_sequence = document.next_sequence.saturating_add(1);
            document.records.insert(
                proposal.proposal_id().to_owned(),
                LocalRecord {
                    recorded_sequence: document.next_sequence,
                    proposal,
                    apply: None,
                    verification_passed: None,
                    verification_evidence_id: None,
                    rollback_candidate: None,
                    rollback: None,
                    checkpoint: None,
                },
            );
            Ok(())
        })
    }

    pub(crate) fn record(&self, id: &str) -> Option<LocalRecord> {
        lock(&self.document).records.get(id).cloned()
    }

    pub(crate) fn reload(&self) -> Result<(), LocalImprovementBlock> {
        let path = self.validated_path()?;
        let bytes = fs::read(path).map_err(|_| LocalImprovementBlock::Io)?;
        let document: LocalDocument =
            serde_json::from_slice(&bytes).map_err(|_| LocalImprovementBlock::Io)?;
        if document.schema_version != 1 {
            return Err(LocalImprovementBlock::Io);
        }
        *lock(&self.document) = document;
        Ok(())
    }

    pub(crate) fn record_apply(
        &self,
        id: &str,
        receipt: LocalApplyReceipt,
        checkpoint: Vec<u8>,
    ) -> Result<(), LocalImprovementBlock> {
        self.mutate(|document| {
            let record = document
                .records
                .get_mut(id)
                .ok_or(LocalImprovementBlock::ProposalNotFound)?;
            if record.apply.is_some() {
                return Err(LocalImprovementBlock::AlreadyApplied);
            }
            record.apply = Some(receipt);
            record.checkpoint = Some(checkpoint);
            Ok(())
        })
    }

    pub(crate) fn record_verification(
        &self,
        id: &str,
        passed: bool,
        evidence_id: &str,
    ) -> Result<(), LocalImprovementBlock> {
        self.mutate(|document| {
            let record = document
                .records
                .get_mut(id)
                .ok_or(LocalImprovementBlock::ProposalNotFound)?;
            if record.apply.is_none() {
                return Err(LocalImprovementBlock::NotApplied);
            }
            record.verification_passed = Some(passed);
            record.verification_evidence_id = Some(evidence_id.to_owned());
            record.rollback_candidate = (!passed).then(|| LocalRollbackCandidate {
                verify_failure_id: evidence_id.to_owned(),
            });
            Ok(())
        })
    }

    pub(crate) fn record_rollback(
        &self,
        id: &str,
        receipt: LocalRollbackReceipt,
    ) -> Result<(), LocalImprovementBlock> {
        self.mutate(|document| {
            let record = document
                .records
                .get_mut(id)
                .ok_or(LocalImprovementBlock::ProposalNotFound)?;
            if record.rollback.is_some() {
                return Err(LocalImprovementBlock::AlreadyRolledBack);
            }
            record.rollback = Some(receipt);
            Ok(())
        })
    }

    fn mutate<T>(
        &self,
        action: impl FnOnce(&mut LocalDocument) -> Result<T, LocalImprovementBlock>,
    ) -> Result<T, LocalImprovementBlock> {
        let mut document = lock(&self.document);
        let output = action(&mut document)?;
        save(&self.validated_path()?, &document)?;
        Ok(output)
    }

    fn validated_path(&self) -> Result<PathBuf, LocalImprovementBlock> {
        match &self.state_root {
            Some(root) => state_path(root, "store.json"),
            None => Ok(self.path.clone()),
        }
    }
}

fn save(path: &Path, document: &LocalDocument) -> Result<(), LocalImprovementBlock> {
    let parent = path.parent().ok_or(LocalImprovementBlock::Io)?;
    fs::create_dir_all(parent).map_err(|_| LocalImprovementBlock::Io)?;
    let mut temporary =
        tempfile::NamedTempFile::new_in(parent).map_err(|_| LocalImprovementBlock::Io)?;
    serde_json::to_writer_pretty(&mut temporary, document)
        .map_err(|_| LocalImprovementBlock::Io)?;
    temporary
        .write_all(b"\n")
        .map_err(|_| LocalImprovementBlock::Io)?;
    temporary
        .as_file()
        .sync_all()
        .map_err(|_| LocalImprovementBlock::Io)?;
    temporary
        .persist(path)
        .map_err(|_| LocalImprovementBlock::Io)?;
    fs::File::open(parent)
        .and_then(|directory| directory.sync_all())
        .map_err(|_| LocalImprovementBlock::Io)
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}
