use super::{Spec030FactStore, Spec030FactStoreError};
use crate::controlled_child::ControlledChildAdapter;
use crate::runtime::trusted_resources::TrustedResourceInspection;

impl Spec030FactStore {
    pub fn record_resource_inspection(
        &self,
        inspection: &TrustedResourceInspection,
    ) -> Result<(), Spec030FactStoreError> {
        for receipt in inspection
            .resources
            .iter()
            .filter_map(|fact| fact.receipt.as_ref())
            .filter(|receipt| receipt.adapter == ControlledChildAdapter::PackageCommand)
        {
            self.record_controlled_child_receipt(receipt)?;
        }
        self.update_resources(
            inspection
                .resources
                .iter()
                .map(|fact| {
                    let mut projection = fact.projection.clone();
                    let diagnostics = inspection
                        .diagnostics
                        .iter()
                        .filter(|diagnostic| diagnostic.resource_ref == projection.resource_ref)
                        .map(|diagnostic| diagnostic.projection())
                        .filter(|diagnostic| !projection.diagnostics.contains(diagnostic))
                        .collect::<Vec<_>>();
                    projection.diagnostics.extend(diagnostics);
                    projection
                })
                .collect(),
        )
    }
}
