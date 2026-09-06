use serde::Serialize;
use serde_json::Value;
use shacs_projection::{Spec034PrimaryPrd, SPEC034_REQUIREMENTS};
use std::error::Error;

#[derive(Debug, Clone, Serialize)]
pub struct SubObservation {
    pub name: &'static str,
    pub observed: bool,
}

pub fn sub_observations<const N: usize>(
    observations: [(&'static str, bool); N],
) -> Vec<SubObservation> {
    observations
        .into_iter()
        .map(|(name, observed)| SubObservation { name, observed })
        .collect()
}

pub fn all_observed(observations: &[SubObservation]) -> bool {
    !observations.is_empty() && observations.iter().all(|observation| observation.observed)
}

#[derive(Debug, Serialize)]
pub struct ObservedReceipt {
    pub requirement_id: &'static str,
    pub name: &'static str,
    pub primary_prd: Spec034PrimaryPrd,
    pub production_source: &'static str,
    pub observable: Value,
}

pub struct ReceiptDraft {
    pub requirement_id: &'static str,
    pub name: &'static str,
    pub primary_prd: Spec034PrimaryPrd,
    pub production_source: &'static str,
    pub observable: Value,
    pub observed: bool,
}

impl ObservedReceipt {
    pub fn from_observation(draft: ReceiptDraft) -> Result<Self, Box<dyn Error>> {
        if !draft.observed || draft.observable.is_null() {
            return Err(format!("receipt {} was not observed", draft.name).into());
        }
        Ok(Self {
            requirement_id: draft.requirement_id,
            name: draft.name,
            primary_prd: draft.primary_prd,
            production_source: draft.production_source,
            observable: draft.observable,
        })
    }
}

pub fn validate_catalog_after_observation(
    receipts: &[ObservedReceipt],
) -> Result<usize, Box<dyn Error>> {
    if receipts.len() != SPEC034_REQUIREMENTS.len() {
        return Err("observed receipt count differs from canonical catalog".into());
    }
    for (receipt, canonical) in receipts.iter().zip(SPEC034_REQUIREMENTS) {
        if receipt.requirement_id != canonical.id || receipt.primary_prd != canonical.primary_prd {
            return Err(format!("receipt catalog mismatch: {}", receipt.name).into());
        }
    }
    Ok(receipts.len())
}
