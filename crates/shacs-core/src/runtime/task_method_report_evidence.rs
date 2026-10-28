use super::task_method_calc::bounded;
use super::task_method_report::{
    ReportEvidence, ReportOutput, ReportSource, ReportVerdict, SourceStatus, CLAIM_LIMIT,
    LOCATOR_LIMIT, SOURCE_LIMIT,
};

impl ReportVerdict {
    /// Every distinct conclusion line, cited or not, for comparing versions.
    pub fn conclusions(&self) -> Vec<String> {
        let mut texts: Vec<String> = Vec::new();
        for text in self
            .claims
            .iter()
            .filter(|claim| claim.statement)
            .map(|claim| &claim.text)
            .chain(&self.uncited)
        {
            if !texts.contains(text) {
                texts.push(text.clone());
            }
        }
        texts
    }
}

impl ReportEvidence {
    /// After a method change that keeps the report part, the sources usable in
    /// the revision it leaves (read there, or already carried) stay usable as
    /// carried sources: they are re-checked, not inherited. Older ones do not.
    pub(crate) fn carry_sources(&mut self, revision: u64) {
        for source in &mut self.sources {
            source.carried = source.status == SourceStatus::Read
                && (source.carried || source.selection_revision == revision);
        }
    }

    /// After a method change that drops the report part, nothing carried stays
    /// usable: if the part comes back later, it starts over.
    pub(crate) fn drop_carried(&mut self) {
        for source in &mut self.sources {
            source.carried = false;
        }
    }

    pub(crate) fn push_source(&mut self, mut source: ReportSource, revision: u64) {
        self.next_seq = self.next_seq.saturating_add(1);
        source.seq = self.next_seq;
        source.selection_revision = revision;
        source.version = self.version;
        self.sources.push(source);
        let removed = self.sources.len().saturating_sub(SOURCE_LIMIT);
        self.sources.drain(..removed);
        self.omitted_sources = self.omitted_sources.saturating_add(removed);
    }

    pub(crate) fn push_output(&mut self, mut output: ReportOutput, revision: u64) {
        output.selection_revision = revision;
        self.outputs.retain(|existing| {
            existing.kind != output.kind
                || existing.key != output.key
                || existing.selection_revision != revision
        });
        self.outputs.push(output);
        let removed = self.outputs.len().saturating_sub(SOURCE_LIMIT);
        let dropped = self
            .outputs
            .drain(..removed)
            .filter(|output| output.selection_revision == revision)
            .count();
        // Outputs of this revision dropped by the limit stay visible as unchecked.
        self.dropped_outputs
            .extend(std::iter::repeat_n(revision, dropped));
        let excess = self.dropped_outputs.len().saturating_sub(64);
        self.dropped_outputs.drain(..excess);
    }

    pub(crate) fn next_version(previous: &ReportEvidence, request_id: &str, goal: &str) -> Self {
        let mut next = Self {
            version: previous.version.saturating_add(1),
            revision_of: Some(bounded(request_id, LOCATOR_LIMIT)),
            purpose: Some(previous.purpose.clone().unwrap_or_else(|| goal.to_owned())),
            previous_claims: previous
                .verdict
                .iter()
                .flat_map(|verdict| verdict.conclusions())
                .take(CLAIM_LIMIT * 3)
                .collect(),
            ..Self::default()
        };
        // Only sources the last judged revision could use carry over.
        let last = previous
            .verdict
            .as_ref()
            .map(|verdict| verdict.selection_revision);
        for source in previous.sources.iter().filter(|source| {
            source.status == SourceStatus::Read
                && (source.carried || Some(source.selection_revision) == last)
        }) {
            let mut carried = source.clone();
            carried.carried = true;
            next.next_seq = next.next_seq.saturating_add(1);
            carried.seq = next.next_seq;
            next.sources.push(carried);
        }
        next.sources.truncate(SOURCE_LIMIT);
        next
    }
}
