use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;

/// One execution's resolved declarations. Never serialized or printed as values.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct SkillEnvironment(Arc<BTreeMap<String, String>>);

impl SkillEnvironment {
    pub fn new(values: BTreeMap<String, String>) -> Self {
        Self(Arc::new(values))
    }

    pub fn values(&self) -> &BTreeMap<String, String> {
        &self.0
    }
}

impl fmt::Debug for SkillEnvironment {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SkillEnvironment")
            .field("names", &self.0.keys().collect::<Vec<_>>())
            .finish()
    }
}
