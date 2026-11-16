//! The trusted source of an automation run, read from the stored work
//! envelope and carried to the tool decision (Spec 053 PRD 010 scope 8).

use super::AutomationSourceEventKind;

/// Where an automation run came from. Only the automation worker sets it, from
/// the stored envelope; message text and metadata cannot. It grants nothing:
/// the decision only uses it to refuse workspace-outside access in unattended
/// runs and to scope the refusal counter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AutomationRunSource {
    Cron {
        job_id: Option<String>,
        /// The approved list's digest the firing was queued with, if any
        /// (Spec 053 PRD 030 scope 4).
        grant_digest: Option<String>,
        work_id: String,
    },
    Heartbeat {
        work_id: String,
    },
    SubagentResult {
        work_id: String,
    },
    Other {
        work_id: String,
    },
}

const CRON_JOB_REF_PREFIX: &str = "cron-job:";

impl AutomationRunSource {
    /// The source of a stored run. Work stored before this source existed
    /// reads the same way, because the envelope already holds its event.
    pub fn from_event(source: &AutomationSourceEventKind, work_id: &str) -> Self {
        let work_id = work_id.to_owned();
        match source {
            AutomationSourceEventKind::Cron {
                approved_automation_rule_ref,
            } => {
                let reference = approved_automation_rule_ref
                    .as_deref()
                    .and_then(|reference| reference.strip_prefix(CRON_JOB_REF_PREFIX));
                let (job_id, grant_digest) = match reference.map(|text| text.split_once('#')) {
                    Some(Some((job_id, digest))) => (Some(job_id), Some(digest)),
                    Some(None) => (reference, None),
                    None => (None, None),
                };
                let present = |text: &&str| !text.trim().is_empty();
                Self::Cron {
                    job_id: job_id.filter(present).map(str::to_owned),
                    grant_digest: grant_digest.filter(present).map(str::to_owned),
                    work_id,
                }
            }
            AutomationSourceEventKind::Heartbeat => Self::Heartbeat { work_id },
            AutomationSourceEventKind::SubagentResult { .. } => Self::SubagentResult { work_id },
            AutomationSourceEventKind::AppTaskResult { .. }
            | AutomationSourceEventKind::ChannelEvent { .. }
            | AutomationSourceEventKind::LocalApiBackground { .. }
            | AutomationSourceEventKind::ManualResume { .. } => Self::Other { work_id },
        }
    }

    pub const fn is_cron(&self) -> bool {
        matches!(self, Self::Cron { .. })
    }

    /// The refusal counter's root: one firing or one run (Spec 053 PRD 010
    /// scope 9).
    pub fn refusal_root(&self) -> String {
        match self {
            Self::Cron {
                job_id, work_id, ..
            } => {
                format!("cron:{}:{work_id}", job_id.as_deref().unwrap_or("unknown"))
            }
            Self::Heartbeat { work_id } => format!("heartbeat:{work_id}"),
            Self::SubagentResult { work_id } => format!("subagent-result:{work_id}"),
            Self::Other { work_id } => format!("automation:{work_id}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::AutomationRunSource;
    use crate::runtime::AutomationSourceEventKind;

    #[test]
    fn cron_job_id_comes_from_the_stored_rule_reference() {
        let source = AutomationRunSource::from_event(
            &AutomationSourceEventKind::Cron {
                approved_automation_rule_ref: Some("cron-job:abc".to_owned()),
            },
            "work-1",
        );
        assert_eq!(
            source,
            AutomationRunSource::Cron {
                job_id: Some("abc".to_owned()),
                grant_digest: None,
                work_id: "work-1".to_owned(),
            },
            "the cron job id is the text after `cron-job:`"
        );
        assert_eq!(source.refusal_root(), "cron:abc:work-1");
        let unknown = AutomationRunSource::from_event(
            &AutomationSourceEventKind::Cron {
                approved_automation_rule_ref: None,
            },
            "work-2",
        );
        assert!(
            unknown.is_cron(),
            "a cron run without a job id stays a cron run"
        );
        assert_eq!(unknown.refusal_root(), "cron:unknown:work-2");
    }

    #[test]
    fn the_approved_list_digest_follows_the_job_id() {
        let source = AutomationRunSource::from_event(
            &AutomationSourceEventKind::Cron {
                approved_automation_rule_ref: Some("cron-job:abc#d1g3st".to_owned()),
            },
            "work-1",
        );
        assert_eq!(
            source,
            AutomationRunSource::Cron {
                job_id: Some("abc".to_owned()),
                grant_digest: Some("d1g3st".to_owned()),
                work_id: "work-1".to_owned(),
            }
        );
        assert_eq!(source.refusal_root(), "cron:abc:work-1");
    }
}
