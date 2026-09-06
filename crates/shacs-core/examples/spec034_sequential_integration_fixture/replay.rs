use serde::Serialize;
use shacs_core::runtime::{
    replay_recorded_media_evidence, MediaEvidenceReplayDependencies, MediaEvidenceReplaySource,
    RecordedArtifactStatus,
};
use std::cell::Cell;
use std::error::Error;

#[derive(Debug, Serialize)]
pub struct ReplayReport {
    pub probe_counts: [u64; 4],
    pub replay_counts: [u64; 4],
    pub source: &'static str,
    pub artifact_count: usize,
    pub snapshot_id: String,
}

#[derive(Default)]
struct CallableSpies {
    network: Cell<u64>,
    credential: Cell<u64>,
    analyzer: Cell<u64>,
    resource: Cell<u64>,
}

impl CallableSpies {
    fn counts(&self) -> [u64; 4] {
        [
            self.network.get(),
            self.credential.get(),
            self.analyzer.get(),
            self.resource.get(),
        ]
    }

    fn reset(&self) {
        self.network.set(0);
        self.credential.set(0);
        self.analyzer.set(0);
        self.resource.set(0);
    }
}

impl MediaEvidenceReplayDependencies for CallableSpies {
    fn request_network(&self) {
        self.network.set(self.network.get() + 1);
    }

    fn resolve_credential(&self) {
        self.credential.set(self.credential.get() + 1);
    }

    fn invoke_analyzer(&self) {
        self.analyzer.set(self.analyzer.get() + 1);
    }

    fn resolve_resource(&self) {
        self.resource.set(self.resource.get() + 1);
    }
}

pub fn run(recorded: &str) -> Result<ReplayReport, Box<dyn Error>> {
    let spies = CallableSpies::default();
    spies.request_network();
    spies.resolve_credential();
    spies.invoke_analyzer();
    spies.resolve_resource();
    let probe_counts = spies.counts();
    spies.reset();
    let receipt = replay_recorded_media_evidence(recorded, &spies)?;
    let replay_counts = spies.counts();
    if probe_counts != [1, 1, 1, 1]
        || replay_counts != [0, 0, 0, 0]
        || receipt.source != MediaEvidenceReplaySource::RecordedMetadata
        || receipt.artifact_status != RecordedArtifactStatus::Recorded
    {
        return Err("recorded replay touched a live dependency".into());
    }
    Ok(ReplayReport {
        probe_counts,
        replay_counts,
        source: "recorded_metadata",
        artifact_count: receipt.artifact_count,
        snapshot_id: receipt.snapshot.snapshot_id,
    })
}
