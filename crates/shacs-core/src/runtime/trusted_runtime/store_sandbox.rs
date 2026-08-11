use super::{Spec030FactStore, Spec030FactStoreError};
use crate::runtime::sandbox_adapter::SandboxExecutionFact;

impl Spec030FactStore {
    pub fn record_sandbox_execution(
        &self,
        fact: &SandboxExecutionFact,
    ) -> Result<(), Spec030FactStoreError> {
        self.update_sandbox(fact.observation())
    }
}
