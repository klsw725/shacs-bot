//! The permission lists users approve for cron jobs and their approval
//! records (Spec 053 PRD 030 scopes 1, 3 and 5).
//!
//! A job's stored list is only what `list` shows. A run uses the list kept in
//! the approval record, and only while that record is not revoked.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Component, Path, PathBuf};
use std::{fs, io};

/// The approval records file next to `jobs.json`.
pub const GRANTS_FILE: &str = "approvals.json";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CronAccess {
    Read,
    /// Writing includes reading.
    Write,
}

/// One entry of a job's list: a path outside the workspace, or a command
/// pattern in the `exec(<pattern>)` rule syntax.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(untagged)]
pub enum CronPermission {
    Path { path: String, access: CronAccess },
    Command { command: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CronGrant {
    pub job_id: String,
    pub digest: String,
    pub items: Vec<CronPermission>,
    pub approved_at_ms: i64,
    #[serde(default)]
    pub revoked_at_ms: Option<i64>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
struct GrantsFile {
    #[serde(default)]
    grants: Vec<CronGrant>,
}

/// The list as stored and compared: `~` expanded, paths normalized, sorted
/// and without repeats. A relative path or one with `..` is refused.
pub fn normalize_permissions(
    items: &[CronPermission],
    home: &Path,
) -> Result<Vec<CronPermission>, String> {
    let mut normalized = items
        .iter()
        .map(|item| match item {
            CronPermission::Path { path, access } => Ok(CronPermission::Path {
                path: normalize_path(path, home)?,
                access: *access,
            }),
            CronPermission::Command { command } => {
                let command = command.trim();
                if command.is_empty() {
                    Err("a command entry needs a command pattern".to_owned())
                } else {
                    Ok(CronPermission::Command {
                        command: command.to_owned(),
                    })
                }
            }
        })
        .collect::<Result<Vec<_>, _>>()?;
    normalized.sort();
    normalized.dedup();
    Ok(normalized)
}

fn normalize_path(path: &str, home: &Path) -> Result<String, String> {
    let expanded = if path == "~" {
        home.to_path_buf()
    } else if let Some(rest) = path.strip_prefix("~/") {
        home.join(rest)
    } else {
        PathBuf::from(path)
    };
    if !expanded.is_absolute() {
        return Err(format!(
            "path entry {path} must be absolute or start with ~"
        ));
    }
    let mut normalized = PathBuf::new();
    for component in expanded.components() {
        match component {
            Component::ParentDir => {
                return Err(format!("path entry {path} must not contain .."));
            }
            Component::CurDir => {}
            other => normalized.push(other),
        }
    }
    Ok(normalized.display().to_string())
}

pub fn permissions_digest(items: &[CronPermission]) -> String {
    let bytes = serde_json::to_vec(items).unwrap_or_default();
    format!("{:x}", Sha256::digest(bytes))
}

pub fn grants_path(cron_store_path: &Path) -> PathBuf {
    cron_store_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(GRANTS_FILE)
}

fn load(path: &Path) -> io::Result<GrantsFile> {
    match fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes).map_err(io::Error::other),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(GrantsFile::default()),
        Err(error) => Err(error),
    }
}

fn save(path: &Path, file: &GrantsFile) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let temporary = path.with_extension("json.tmp");
    fs::write(
        &temporary,
        serde_json::to_vec_pretty(file).map_err(io::Error::other)?,
    )?;
    fs::rename(temporary, path)
}

/// Reads, changes and saves the records with an exclusive lock on a file
/// beside them, held across processes, so a revocation saved by one writer
/// is never overwritten with an older copy by another.
fn change_locked(path: &Path, change: impl FnOnce(&mut GrantsFile) -> bool) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let lock = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(path.with_extension("json.lock"))?;
    lock.lock()?;
    let mut file = load(path)?;
    if change(&mut file) {
        save(path, &file)?;
    }
    Ok(())
}

/// Keeps the approval of `items` for `job_id`. Only an approved `cron add`
/// calls this.
pub fn record_grant(path: &Path, grant: CronGrant) -> io::Result<()> {
    change_locked(path, |file| {
        file.grants.retain(|existing| {
            !(existing.job_id == grant.job_id && existing.digest == grant.digest)
        });
        file.grants.push(grant);
        true
    })
}

/// Marks every approval of `job_id` revoked. A user removal calls this
/// before it reports the job removed.
pub fn revoke_grants(path: &Path, job_id: &str, now_ms: i64) -> io::Result<()> {
    change_locked(path, |file| {
        let mut changed = false;
        for grant in &mut file.grants {
            if grant.job_id == job_id && grant.revoked_at_ms.is_none() {
                grant.revoked_at_ms = Some(now_ms);
                changed = true;
            }
        }
        changed
    })
}

/// The approval of `job_id` with `digest`, read from disk now, when it is
/// not revoked. A file that cannot be read approves nothing.
pub fn valid_grant(path: &Path, job_id: &str, digest: &str) -> Option<CronGrant> {
    load(path).ok()?.grants.into_iter().find(|grant| {
        grant.job_id == job_id && grant.digest == digest && grant.revoked_at_ms.is_none()
    })
}

/// The latest approval of `job_id` that is not revoked.
pub fn current_grant(path: &Path, job_id: &str) -> Option<CronGrant> {
    load(path)
        .ok()?
        .grants
        .into_iter()
        .filter(|grant| grant.job_id == job_id && grant.revoked_at_ms.is_none())
        .max_by_key(|grant| grant.approved_at_ms)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AddCronJob, CronSchedule, CronService, PersistentCronService, RemoveJobResult};
    use std::time::{SystemTime, UNIX_EPOCH};

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    fn temp_folder(name: &str) -> io::Result<PathBuf> {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|time| time.as_nanos())
            .unwrap_or_default();
        let folder = std::env::temp_dir().join(format!(
            "shacs-cron-grants-{name}-{}-{nanos}",
            std::process::id()
        ));
        fs::create_dir_all(&folder)?;
        Ok(folder)
    }

    fn read(path: &str) -> CronPermission {
        CronPermission::Path {
            path: path.to_owned(),
            access: CronAccess::Read,
        }
    }

    fn job(permissions: Vec<CronPermission>, delete_after_run: bool) -> AddCronJob {
        AddCronJob {
            permissions,
            name: "transcripts".to_owned(),
            schedule: if delete_after_run {
                CronSchedule::at(1)
            } else {
                CronSchedule::every(60_000)
            },
            message: "summarize".to_owned(),
            deliver: false,
            channel: Some("cli".to_owned()),
            to: Some("direct".to_owned()),
            delete_after_run,
            channel_meta: serde_json::Value::Null,
            session_key: None,
        }
    }

    #[test]
    fn a_list_is_normalized_before_it_is_approved() -> TestResult {
        let home = Path::new("/home/me");
        let items = vec![
            read("/tmp/./b"),
            read("~/notes"),
            read("/tmp/b"),
            CronPermission::Command {
                command: " python3 /x.py * ".to_owned(),
            },
        ];

        let normalized = normalize_permissions(&items, home)?;

        assert_eq!(
            normalized,
            vec![
                read("/home/me/notes"),
                read("/tmp/b"),
                CronPermission::Command {
                    command: "python3 /x.py *".to_owned()
                },
            ]
        );
        assert_eq!(
            permissions_digest(&normalized),
            permissions_digest(&normalize_permissions(&normalized, home)?)
        );
        for refused in ["relative/path", "/tmp/../etc"] {
            assert!(
                normalize_permissions(&[read(refused)], home).is_err(),
                "{refused}"
            );
        }
        Ok(())
    }

    #[test]
    fn a_grant_holds_until_it_is_revoked_and_only_for_its_digest() -> TestResult {
        let path = temp_folder("record")?.join(GRANTS_FILE);
        let items = vec![read("/tmp")];
        let digest = permissions_digest(&items);
        record_grant(
            &path,
            CronGrant {
                job_id: "job-1".to_owned(),
                digest: digest.clone(),
                items: items.clone(),
                approved_at_ms: 1,
                revoked_at_ms: None,
            },
        )?;

        assert!(valid_grant(&path, "job-1", &digest).is_some());
        assert!(valid_grant(&path, "job-1", "other").is_none());
        assert!(valid_grant(&path, "job-2", &digest).is_none());
        assert_eq!(
            current_grant(&path, "job-1").map(|grant| grant.digest),
            Some(digest.clone())
        );
        revoke_grants(&path, "job-1", 2)?;
        assert!(valid_grant(&path, "job-1", &digest).is_none());
        assert!(current_grant(&path, "job-1").is_none());
        Ok(())
    }

    #[test]
    fn a_user_removal_revokes_for_every_service_instance_but_a_one_shot_cleanup_does_not(
    ) -> TestResult {
        let folder = temp_folder("remove")?;
        let store = folder.join("jobs.json");
        let scheduler = PersistentCronService::new(&store)?;
        scheduler.start()?;
        let tool = PersistentCronService::new(&store)?;
        let grants = tool.grants_path().ok_or("no grants path")?;
        let items = vec![read("/tmp")];
        let digest = permissions_digest(&items);
        let approve = |job_id: &str| {
            record_grant(
                &grants,
                CronGrant {
                    job_id: job_id.to_owned(),
                    digest: digest.clone(),
                    items: items.clone(),
                    approved_at_ms: 1,
                    revoked_at_ms: None,
                },
            )
        };
        let recurring = tool.add_job(job(items.clone(), false));
        approve(&recurring.id)?;
        let one_shot = tool.add_job(job(items.clone(), true));
        approve(&one_shot.id)?;

        // When: the user removes one job through the other instance, and the
        // one-shot job retires itself after running.
        assert_eq!(
            scheduler.remove_job(&recurring.id),
            RemoveJobResult::Removed
        );
        let executor = |_: &crate::CronJob| Ok::<Option<String>, String>(None);
        scheduler.run_job(&one_shot.id, true, &executor)?;

        // Then
        assert!(valid_grant(&grants, &recurring.id, &digest).is_none());
        assert!(valid_grant(&grants, &one_shot.id, &digest).is_some());
        assert!(
            tool.get_job(&one_shot.id).is_none(),
            "the one-shot job retired"
        );
        Ok(())
    }

    #[test]
    fn concurrent_records_and_revocations_lose_nothing() -> TestResult {
        // Given: one writer keeps approving jobs while another revokes
        // `job-0`, both through the file, from separate threads.
        let path = temp_folder("concurrent")?.join(GRANTS_FILE);
        let items = vec![read("/tmp")];
        let digest = permissions_digest(&items);
        let grant = |job_id: String| CronGrant {
            job_id,
            digest: digest.clone(),
            items: items.clone(),
            approved_at_ms: 1,
            revoked_at_ms: None,
        };
        record_grant(&path, grant("job-0".to_owned()))?;
        let start = std::sync::Arc::new(std::sync::Barrier::new(2));

        // When
        std::thread::scope(|scope| -> TestResult {
            let records = scope.spawn(|| -> io::Result<()> {
                start.wait();
                for index in 1..=40 {
                    record_grant(&path, grant(format!("job-{index}")))?;
                }
                Ok(())
            });
            start.wait();
            revoke_grants(&path, "job-0", 2)?;
            records.join().map_err(|_| "recording thread panicked")??;
            Ok(())
        })?;

        // Then
        assert!(
            valid_grant(&path, "job-0", &digest).is_none(),
            "the revocation held"
        );
        for index in 1..=40 {
            assert!(
                valid_grant(&path, &format!("job-{index}"), &digest).is_some(),
                "job-{index}"
            );
        }
        Ok(())
    }

    #[test]
    fn a_job_stored_without_a_list_reads_as_an_empty_list() -> TestResult {
        // Given: a job saved the way jobs were saved before lists existed.
        let folder = temp_folder("old")?;
        let store = folder.join("jobs.json");
        let service = PersistentCronService::new(&store)?;
        service.start()?;
        let added = service.add_job(job(Vec::new(), false));
        let saved = fs::read_to_string(&store)?;
        assert!(!saved.contains("permissions"), "{saved}");

        // When
        let reopened = PersistentCronService::new(&store)?;

        // Then
        let old = reopened.get_job(&added.id).ok_or("old job not read")?;
        assert!(old.permissions.is_empty());
        Ok(())
    }
}
