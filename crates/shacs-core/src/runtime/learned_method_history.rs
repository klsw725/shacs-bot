use super::learned_method::{
    DecisionKind, LearnedMethod, LearnedMethodState, LearnedMethodVersion, OriginKind,
};
use super::learned_method_decide::replay;

/// The versions and the decisions that made them current tell one story:
/// v1 came from generation; each later version is the user's edit of the
/// version current then; a restore chose an earlier version that was
/// confirmed and not rejected then; the user put out of use only a version
/// confirmed then; every other decision is about the version current when
/// it was made; and the saved current version is where the story ends.
pub(super) fn history_holds(method: &LearnedMethod) -> bool {
    let versions = &method.versions;
    let numbered = versions
        .iter()
        .enumerate()
        .all(|(index, version)| usize::try_from(version.version).is_ok_and(|n| n == index + 1));
    let generated = versions.first().is_some_and(|first| {
        first.origin.kind == OriginKind::ToolLessGeneration && first.origin.edited_from.is_none()
    });
    if !(numbered && generated) {
        return false;
    }
    let mut current = 1;
    let mut made = 1;
    for (index, decision) in method.decisions.iter().enumerate() {
        let before = &method.decisions[..index];
        let fits = match decision.kind {
            DecisionKind::Edited => {
                decision.version == made + 1 && edited(versions, decision.version, current)
            }
            DecisionKind::Restored => {
                let replayed = replay(before, decision.version);
                decision.version != current
                    && decision.version <= made
                    && replayed.ever_confirmed
                    && replayed.state != LearnedMethodState::Rejected
            }
            DecisionKind::Deactivated => {
                decision.version == current
                    && replay(before, current).state == LearnedMethodState::Confirmed
            }
            DecisionKind::Confirmed
            | DecisionKind::Rejected
            | DecisionKind::Verified
            | DecisionKind::Conflicted
            | DecisionKind::Repeated => decision.version == current,
        };
        if !fits {
            return false;
        }
        match decision.kind {
            DecisionKind::Edited => {
                made = decision.version;
                current = decision.version;
            }
            DecisionKind::Restored => current = decision.version,
            _ => {}
        }
    }
    usize::try_from(made).is_ok_and(|made| made == versions.len())
        && current == method.current_version
}

/// Version `number` is the user's edit of version `from`: the same evidence
/// and task, only text changed.
fn edited(versions: &[LearnedMethodVersion], number: u32, from: u32) -> bool {
    let at = |number: u32| {
        usize::try_from(number)
            .ok()
            .and_then(|number| number.checked_sub(1))
            .and_then(|index| versions.get(index))
    };
    let (Some(version), Some(base)) = (at(number), at(from)) else {
        return false;
    };
    version.origin.kind == OriginKind::UserEdit
        && version.origin.edited_from == Some(from)
        && version.origin.session == base.origin.session
        && version.origin.task == base.origin.task
        && version.origin.omitted_evidence == base.origin.omitted_evidence
        && version.evidence == base.evidence
        && version.excluded == base.excluded
}
