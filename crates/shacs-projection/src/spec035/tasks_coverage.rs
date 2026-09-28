use super::Spec035TaskOwnerKind;
use crate::Spec031Count;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "availability", rename_all = "snake_case", deny_unknown_fields)]
pub enum Spec035TaskCount {
    Available { count: Spec031Count },
    Unavailable,
}

impl Spec035TaskCount {
    pub const fn available(count: u64) -> Self {
        Self::Available {
            count: Spec031Count::new(count),
        }
    }

    pub const fn count(self) -> Option<Spec031Count> {
        match self {
            Self::Available { count } => Some(count),
            Self::Unavailable => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct Spec035TasksCoverage {
    pub goal: Spec035TaskCount,
    pub child: Spec035TaskCount,
    pub workflow: Spec035TaskCount,
    pub automation: Spec035TaskCount,
    pub app: Spec035TaskCount,
    pub recovery: Spec035TaskCount,
}

impl Spec035TasksCoverage {
    pub const fn all_available(count: u64) -> Self {
        let count = Spec035TaskCount::available(count);
        Self {
            goal: count,
            child: count,
            workflow: count,
            automation: count,
            app: count,
            recovery: count,
        }
    }

    pub const fn all_unavailable() -> Self {
        Self {
            goal: Spec035TaskCount::Unavailable,
            child: Spec035TaskCount::Unavailable,
            workflow: Spec035TaskCount::Unavailable,
            automation: Spec035TaskCount::Unavailable,
            app: Spec035TaskCount::Unavailable,
            recovery: Spec035TaskCount::Unavailable,
        }
    }

    pub const fn for_kind(&self, kind: Spec035TaskOwnerKind) -> Spec035TaskCount {
        match kind {
            Spec035TaskOwnerKind::Goal => self.goal,
            Spec035TaskOwnerKind::Child => self.child,
            Spec035TaskOwnerKind::Workflow => self.workflow,
            Spec035TaskOwnerKind::Automation => self.automation,
            Spec035TaskOwnerKind::App => self.app,
            Spec035TaskOwnerKind::Recovery => self.recovery,
        }
    }
}
