use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Spec034EvidenceRef {
    pub locator: String,
    pub digest: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Spec034PrimaryPrd {
    Prd000,
    Prd001,
    Prd002,
    Prd003,
    Prd004,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Spec034RequirementSpec {
    pub id: &'static str,
    pub primary_prd: Spec034PrimaryPrd,
}

const fn requirement(id: &'static str, primary_prd: Spec034PrimaryPrd) -> Spec034RequirementSpec {
    Spec034RequirementSpec { id, primary_prd }
}

pub const SPEC034_REQUIREMENTS: [Spec034RequirementSpec; 22] = [
    requirement("034-MH001", Spec034PrimaryPrd::Prd000),
    requirement("034-MH002", Spec034PrimaryPrd::Prd001),
    requirement("034-MH003", Spec034PrimaryPrd::Prd001),
    requirement("034-MH004", Spec034PrimaryPrd::Prd001),
    requirement("034-MH005", Spec034PrimaryPrd::Prd002),
    requirement("034-MH006", Spec034PrimaryPrd::Prd002),
    requirement("034-MH007", Spec034PrimaryPrd::Prd002),
    requirement("034-MH008", Spec034PrimaryPrd::Prd003),
    requirement("034-MH009", Spec034PrimaryPrd::Prd003),
    requirement("034-MH010", Spec034PrimaryPrd::Prd002),
    requirement("034-AC001", Spec034PrimaryPrd::Prd000),
    requirement("034-AC002", Spec034PrimaryPrd::Prd001),
    requirement("034-AC003", Spec034PrimaryPrd::Prd001),
    requirement("034-AC004", Spec034PrimaryPrd::Prd002),
    requirement("034-AC005", Spec034PrimaryPrd::Prd002),
    requirement("034-AC006", Spec034PrimaryPrd::Prd002),
    requirement("034-AC007", Spec034PrimaryPrd::Prd003),
    requirement("034-AC008", Spec034PrimaryPrd::Prd003),
    requirement("034-AC009", Spec034PrimaryPrd::Prd002),
    requirement("034-AC010", Spec034PrimaryPrd::Prd004),
    requirement("034-AC011", Spec034PrimaryPrd::Prd003),
    requirement("034-AC012", Spec034PrimaryPrd::Prd003),
];
