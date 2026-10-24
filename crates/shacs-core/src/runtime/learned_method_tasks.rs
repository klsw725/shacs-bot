use super::learned_method::{
    DecisionKind, EvidenceKind, LearnedMethodVersion, MethodDecision, MethodEvidence, SourceTrust,
    TaskCheck, TaskIdentity, UserStatement,
};
use super::learned_method_evidence::{digest, Evidence};
use std::collections::BTreeSet;

/// Why a check of `kind` may not follow the `earlier` decisions about
/// `version`: a counted task must be new, a rerun must repeat a task already
/// recorded and show a message or run not recorded yet, and a contradiction
/// must cite feedback no earlier contradiction cited.
pub(super) fn refused(
    version: &LearnedMethodVersion,
    earlier: &[&MethodDecision],
    kind: DecisionKind,
    check: &TaskCheck,
) -> Option<String> {
    let checks: Vec<&TaskCheck> = earlier
        .iter()
        .filter_map(|decision| decision.check.as_ref())
        .collect();
    let repeat = repeats(version, &checks, &check.task);
    let among = |known: &mut dyn Iterator<Item = &MethodEvidence>| {
        let known: Vec<&MethodEvidence> = known.collect();
        check
            .evidence
            .iter()
            .all(|record| known.iter().any(|known| same_source(known, record)))
    };
    let shown = among(
        &mut version
            .evidence
            .iter()
            .chain(checks.iter().flat_map(|earlier| earlier.evidence.iter())),
    );
    let contradicted = among(
        &mut earlier
            .iter()
            .filter(|decision| decision.kind == DecisionKind::Conflicted)
            .filter_map(|decision| decision.check.as_ref())
            .flat_map(|earlier| earlier.evidence.iter()),
    );
    match (kind, repeat) {
        (DecisionKind::Verified, repeat) => repeat.map(str::to_owned),
        (DecisionKind::Repeated, None) => {
            Some("it is not a rerun of a task already recorded for this method".to_owned())
        }
        (DecisionKind::Repeated, Some(repeat)) if shown => {
            Some(format!("{repeat}, and what it shows was already recorded"))
        }
        (DecisionKind::Conflicted, _) if contradicted => {
            Some("the feedback in it that contradicts this method was already recorded".to_owned())
        }
        _ => None,
    }
}

/// Whether two records are of the same message (and call), as saved.
fn same_source(known: &MethodEvidence, record: &MethodEvidence) -> bool {
    let call = |record: &MethodEvidence| {
        record
            .source_ref
            .rsplit_once('#')
            .map(|(_, call)| call.to_owned())
    };
    known.kind == record.kind
        && known.digest == record.digest
        && known.at == record.at
        && call(known) == call(record)
}

/// Why `task` is not a new task for `version` after the earlier `checks`.
pub(super) fn repeats(
    version: &LearnedMethodVersion,
    checks: &[&TaskCheck],
    task: &TaskIdentity,
) -> Option<&'static str> {
    let mut seen = Identity::of(version);
    for check in checks {
        seen.add(&check.task);
    }
    let shared =
        |mine: &[String], known: &BTreeSet<String>| mine.iter().any(|item| known.contains(item));
    if shared(&task.requests, &seen.requests) {
        Some("it repeats a request of a task already recorded for this method")
    } else if shared(&task.messages, &seen.messages) {
        Some("it shares your messages with a task already recorded for this method")
    } else if shared(&task.materials, &seen.materials) {
        Some("it worked on the same material as a task already recorded for this method")
    } else if seen.untrusted && shared(&task.sources, &seen.sources) {
        Some("the method came from untrusted material, and this task read the same source again")
    } else {
        None
    }
}

/// The identity of the task in a window of evidence.
pub(super) fn task_identity(items: &[Evidence]) -> TaskIdentity {
    let distinct = |digests: Vec<String>| -> Vec<String> {
        digests
            .into_iter()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect()
    };
    let mut task = TaskIdentity::default();
    for item in items {
        add_record(&mut task, &item.record);
    }
    TaskIdentity {
        requests: distinct(task.requests),
        messages: distinct(task.messages),
        materials: distinct(task.materials),
        sources: distinct(task.sources),
    }
}

/// What `record` adds to the identity of its task.
fn add_record(task: &mut TaskIdentity, record: &MethodEvidence) {
    let owner = record.kind == EvidenceKind::UserMessage && record.trust == SourceTrust::Owner;
    if owner {
        task.messages.push(message_identity(record));
        if record.classified_as == Some(UserStatement::Request) {
            task.requests.push(record.digest.clone());
        }
    }
    if let Some(material) = &record.material_digest {
        task.materials.push(material.clone());
        if record.kind == EvidenceKind::ToolCall {
            task.sources.push(record.digest.clone());
        }
    }
}

/// A message of the user by its text and the time it was written.
fn message_identity(record: &MethodEvidence) -> String {
    digest(&format!(
        "{}\n{}",
        record.digest,
        record.at.as_deref().unwrap_or_default()
    ))
}

/// The digests of the tasks recorded for a version, starting with the one it
/// came from.
struct Identity {
    requests: BTreeSet<String>,
    messages: BTreeSet<String>,
    materials: BTreeSet<String>,
    sources: BTreeSet<String>,
    /// Whether untrusted material influenced a step.
    untrusted: bool,
}

impl Identity {
    fn of(version: &LearnedMethodVersion) -> Self {
        let records = &version.evidence;
        let mut kept = TaskIdentity::default();
        for record in records {
            add_record(&mut kept, record);
        }
        let mut identity = Self {
            requests: BTreeSet::new(),
            messages: BTreeSet::new(),
            materials: BTreeSet::new(),
            sources: BTreeSet::new(),
            untrusted: version.steps.iter().any(|step| {
                step.evidence.iter().any(|id| {
                    records
                        .iter()
                        .any(|record| &record.id == id && record.trust == SourceTrust::Untrusted)
                })
            }),
        };
        identity.add(&version.origin.task);
        identity.add(&kept);
        identity
    }

    fn add(&mut self, task: &TaskIdentity) {
        self.requests.extend(task.requests.iter().cloned());
        self.messages.extend(task.messages.iter().cloned());
        self.materials.extend(task.materials.iter().cloned());
        self.sources.extend(task.sources.iter().cloned());
    }
}
