//! Cron jobs' approved lists in the permission decision (Spec 053 PRD 030).

use super::auto_mode::{self, real_path, shell_command, shell_paths};
use super::{
    AutomationRunSource, CronListRequest, PermissionCeilingSnapshot, PermissionEnvironment,
    PermissionedAction, RuntimeBoundaryOrigin,
};
use serde_json::Value;
use shacs_cron::{normalize_permissions, valid_grant, CronAccess, CronGrant, CronPermission};
use std::path::{Path, PathBuf};

/// Commands that only read the files they name: none of them has an option
/// that writes a file (PRD 030 scope 6).
const READ_ONLY_COMMANDS: &[&str] = &[
    "cat",
    "head",
    "tail",
    "wc",
    "ls",
    "stat",
    "file",
    "grep",
    "rg",
    "du",
    "diff",
    "cmp",
    "md5sum",
    "sha1sum",
    "sha256sum",
    "realpath",
    "readlink",
];

/// The approval this cron run's call may use: read from the record file now,
/// so a removal between two calls takes effect on the second (scope 5).
pub(crate) fn current_grant(
    source: Option<&AutomationRunSource>,
    store: Option<&Path>,
) -> Option<CronGrant> {
    let AutomationRunSource::Cron {
        job_id: Some(job_id),
        grant_digest: Some(digest),
        ..
    } = source?
    else {
        return None;
    };
    valid_grant(store?, job_id, digest)
}

/// Whether the approved list covers every workspace-outside target of the
/// call (scope 6).
pub(crate) fn covers(
    grant: &CronGrant,
    action: &PermissionedAction,
    read_only_tool: bool,
    targets: &[String],
    environment: Option<&PermissionEnvironment>,
) -> bool {
    if targets.is_empty() {
        return true;
    }
    if action.tool_name == "exec" {
        let Some(environment) = environment else {
            return false;
        };
        let arguments = action.decision_arguments();
        let Some(command) = arguments.get("command").and_then(Value::as_str) else {
            return false;
        };
        // The folder the command runs in, found as the tool finds it: the
        // path as written, so `link/..` follows the link first.
        let written_folder = arguments
            .get("working_dir")
            .and_then(Value::as_str)
            .map_or_else(
                || environment.workspace.clone(),
                |dir| {
                    shell_command::join_word_as_written(
                        &environment.workspace,
                        dir,
                        &environment.home,
                    )
                },
            );
        let real_working_folder = std::fs::canonicalize(&written_folder)
            .unwrap_or_else(|_| real_path::real_path(&shell_command::normalize(&written_folder)));
        let working_folder = real_working_folder.clone();
        // Relative paths of the command resolve there, unseen, so a working
        // folder outside the workspace always needs a write entry.
        return targets.iter().all(|target| {
            let target_path = Path::new(target);
            let read_only = target_path != working_folder
                && target_path != real_working_folder
                && read_only_target(command, &working_folder, &environment.home, target);
            target != ".."
                && item_covers(
                    grant,
                    target_path,
                    if read_only {
                        CronAccess::Read
                    } else {
                        CronAccess::Write
                    },
                )
        });
    }
    let needed = if read_only_tool {
        CronAccess::Read
    } else {
        CronAccess::Write
    };
    targets
        .iter()
        .all(|target| item_covers(grant, Path::new(target), needed))
}

fn item_covers(grant: &CronGrant, target: &Path, needed: CronAccess) -> bool {
    grant.items.iter().any(|item| match item {
        CronPermission::Path { path, access } => {
            *access >= needed && {
                let item = PathBuf::from(path);
                target.starts_with(&item) || target.starts_with(real_path::real_path(&item))
            }
        }
        CronPermission::Command { .. } => false,
    })
}

/// The target appears only as an argument of a read-only command or as an
/// input redirection, and never where something is written.
fn read_only_target(command: &str, working_folder: &Path, home: &Path, target: &str) -> bool {
    let analysis = shell_command::analyze(command, working_folder, home);
    if !analysis.analyzable {
        return false;
    }
    let target = Path::new(target);
    let same = |path: &PathBuf| path == target || real_path::real_path(path) == target;
    let mut found = false;
    for subcommand in &analysis.subcommands {
        let paths = shell_paths::subcommand_paths(subcommand, home);
        if paths.writes.iter().any(|word| word.paths.iter().any(same)) {
            return false;
        }
        let read_only_command = subcommand
            .command_word()
            .and_then(|word| Path::new(word).file_name()?.to_str())
            .is_some_and(|name| READ_ONLY_COMMANDS.contains(&name));
        for word in &paths.reads {
            if word.paths.iter().any(same) {
                if word.source != shell_paths::PathSource::InputRedirect && !read_only_command {
                    return false;
                }
                found = true;
            }
        }
    }
    found
}

/// Why the command entries of a list cannot be approved: a pattern that is
/// not an `exec` rule pattern, or one auto mode treats as broad (PRD 030
/// scope 1).
pub(crate) fn cron_command_problem(items: &[CronPermission]) -> Option<String> {
    items.iter().find_map(|item| match item {
        CronPermission::Command { command } => match auto_mode::broad_exec_pattern(command) {
            None => Some(format!(
                "command entry `{command}` is not an exec rule pattern"
            )),
            Some(true) => Some(format!(
                "command entry `{command}` is too broad; name the program and fixed arguments"
            )),
            Some(false) => None,
        },
        CronPermission::Path { .. } => None,
    })
}

/// The list a `cron add` asks to approve, checked against what no list may
/// grant: secret files, and writes to protected paths (PRD 030 scope 2).
pub(crate) fn add_request(
    action: &PermissionedAction,
    environment: Option<&PermissionEnvironment>,
) -> Option<CronListRequest> {
    if action.tool_name != "cron" {
        return None;
    }
    let arguments = action.decision_arguments();
    if arguments.get("action").and_then(Value::as_str) != Some("add") {
        return None;
    }
    let raw = arguments
        .get("permissions")
        .filter(|value| !value.is_null())?;
    let items: Vec<CronPermission> = match serde_json::from_value(raw.clone()) {
        Ok(items) => items,
        Err(_) => return Some(CronListRequest::Refused("malformed list".to_owned())),
    };
    if items.is_empty() {
        return None;
    }
    let Some(environment) = environment else {
        return Some(CronListRequest::Refused(
            "the list cannot be checked in this run".to_owned(),
        ));
    };
    let items = match normalize_permissions(&items, &environment.home) {
        Ok(items) => items,
        Err(problem) => return Some(CronListRequest::Refused(problem)),
    };
    if let Some(problem) = cron_command_problem(&items) {
        return Some(CronListRequest::Refused(problem));
    }
    for item in &items {
        if let CronPermission::Path { path, access } = item {
            let path = Path::new(path);
            if environment.secret_files.contains(path) {
                return Some(CronListRequest::Refused(format!(
                    "{} is a secret file",
                    path.display()
                )));
            }
            if *access == CronAccess::Write && environment.protected_paths.contains(path) {
                return Some(CronListRequest::Refused(format!(
                    "{} is a protected path",
                    path.display()
                )));
            }
        }
    }
    let text = |key: &str| arguments.get(key).and_then(Value::as_str);
    let schedule = text("cron_expr")
        .map(|expr| format!("cron `{expr}`"))
        .or_else(|| text("at").map(|at| format!("at {at}")))
        .or_else(|| {
            arguments
                .get("every_seconds")
                .and_then(Value::as_i64)
                .map(|seconds| format!("every {seconds}s"))
        })
        .unwrap_or_else(|| "no schedule".to_owned());
    let listed: Vec<String> = items
        .iter()
        .map(crate::tools::cron_permission_text)
        .collect();
    Some(CronListRequest::Ask(format!(
        "the cron job \"{}\" ({schedule}) will run \"{}\" with nobody watching, and asks you to approve what it may use then: {}.",
        text("name").unwrap_or("unnamed"),
        text("message").unwrap_or_default(),
        listed.join("; ")
    )))
}

/// A cron run queued with an approved list carries the approval reference
/// its ceiling asks for (022 PRD 005, Spec 053 PRD 030 scope 9). Other runs
/// keep their ceiling.
pub(crate) fn ceiling_for_source(
    ceiling: Option<PermissionCeilingSnapshot>,
    source: Option<&AutomationRunSource>,
    store: Option<&Path>,
) -> Option<PermissionCeilingSnapshot> {
    let Some(grant) = current_grant(source, store) else {
        return ceiling;
    };
    let (job_id, digest) = (grant.job_id, grant.digest);
    ceiling.map(|ceiling| PermissionCeilingSnapshot {
        origin: RuntimeBoundaryOrigin::CronWake {
            job_id: Some(job_id.clone()),
            approval_ref: Some(format!("cron-grant:{job_id}:{digest}")),
        },
        ..ceiling
    })
}

#[cfg(test)]
mod tests {
    use super::ceiling_for_source;
    use crate::runtime::{
        evaluate_inherited_ceiling, AutomationRunSource, BoundaryPermissionViolation,
        InheritedPermissionContext, PermissionCeilingSnapshot, PermissionMode,
        RuntimeBoundaryOrigin,
    };

    fn ceiling() -> PermissionCeilingSnapshot {
        PermissionCeilingSnapshot {
            parent_mode: PermissionMode::Default,
            capability_ceiling: Vec::new(),
            approved_scope_refs: vec!["workspace".to_owned()],
            origin: RuntimeBoundaryOrigin::UserTurn,
        }
    }

    fn violations(ceiling: PermissionCeilingSnapshot) -> Vec<BoundaryPermissionViolation> {
        evaluate_inherited_ceiling(&InheritedPermissionContext {
            ceiling,
            requested_mode: PermissionMode::Default,
            requested_capabilities: Vec::new(),
            per_action_evaluation_required: true,
        })
        .violations
    }

    #[test]
    fn a_cron_run_with_a_valid_approved_list_carries_its_approval_reference(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let folder = std::env::temp_dir().join(format!(
            "shacs-cron-ceiling-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|time| time.as_nanos())
                .unwrap_or_default()
        ));
        std::fs::create_dir_all(&folder)?;
        let store = folder.join(shacs_cron::GRANTS_FILE);
        shacs_cron::record_grant(
            &store,
            shacs_cron::CronGrant {
                job_id: "job-1".to_owned(),
                digest: "d1".to_owned(),
                items: Vec::new(),
                approved_at_ms: 1,
                revoked_at_ms: None,
            },
        )?;
        let source = |digest: Option<&str>| AutomationRunSource::Cron {
            job_id: Some("job-1".to_owned()),
            grant_digest: digest.map(str::to_owned),
            work_id: "w".to_owned(),
        };

        let with_grant =
            ceiling_for_source(Some(ceiling()), Some(&source(Some("d1"))), Some(&store));
        let unknown =
            ceiling_for_source(Some(ceiling()), Some(&source(Some("other"))), Some(&store));
        let without_digest = ceiling_for_source(Some(ceiling()), Some(&source(None)), Some(&store));
        shacs_cron::revoke_grants(&store, "job-1", 2)?;
        let revoked = ceiling_for_source(Some(ceiling()), Some(&source(Some("d1"))), Some(&store));

        assert_eq!(
            with_grant.as_ref().map(|ceiling| ceiling.origin.clone()),
            Some(RuntimeBoundaryOrigin::CronWake {
                job_id: Some("job-1".to_owned()),
                approval_ref: Some("cron-grant:job-1:d1".to_owned()),
            })
        );
        assert!(with_grant.is_some_and(|ceiling| violations(ceiling).is_empty()));
        for kept in [unknown, without_digest, revoked] {
            assert_eq!(kept, Some(ceiling()), "other runs keep their ceiling");
        }
        let missing = PermissionCeilingSnapshot {
            origin: RuntimeBoundaryOrigin::CronWake {
                job_id: Some("job-1".to_owned()),
                approval_ref: None,
            },
            ..ceiling()
        };
        assert!(violations(missing).contains(&BoundaryPermissionViolation::MissingApprovalRef));
        Ok(())
    }
}
