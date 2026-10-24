use super::*;
use crate::runtime::learned_method::LearnedMethod;
use crate::runtime::learned_method_evidence::digest;
use crate::runtime::learned_method_recheck::{recheck, recheck_check, Sources};
use crate::runtime::learned_method_render::Checked;
use crate::runtime::learned_method_store::{LearnedMethodStore, LearnedMethodStoreError};
use shacs_session::SessionSummary;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::{fs, io};

/// Finds sessions by key digest. A session is taken as deleted only when
/// every session file could be read: an unreadable one may be the session
/// asked for.
#[derive(Default)]
struct Origins {
    keys: Option<HashMap<String, String>>,
    unreadable: Vec<String>,
}

impl Origins {
    fn load(&mut self, sessions: &SessionManager, origin: &str) -> io::Result<Option<Session>> {
        if self.keys.is_none() {
            let summaries = sessions.list_sessions()?;
            self.unreadable = unlisted(sessions.sessions_dir(), &summaries)?;
            self.keys = Some(
                summaries
                    .into_iter()
                    .map(|summary| (digest(&summary.key), summary.key))
                    .collect(),
            );
        }
        match self.keys.as_ref().and_then(|keys| keys.get(origin)) {
            Some(key) => sessions.load_existing(key).map(Some).ok_or_else(|| {
                io::Error::other(format!(
                    "session {} could not be read",
                    origin.get(7..19).unwrap_or_default()
                ))
            }),
            None if !self.unreadable.is_empty() => Err(io::Error::other(format!(
                "session file(s) {} could not be read",
                self.unreadable.join(", ")
            ))),
            None => Ok(None),
        }
    }
}

/// The session entries in `dir` that a listing of it skipped: unreadable,
/// or not a regular file (a link or a directory, say).
fn unlisted(dir: &Path, listed: &[SessionSummary]) -> io::Result<Vec<String>> {
    let mut names = Vec::new();
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        if path.extension().and_then(|ext| ext.to_str()) == Some("jsonl")
            && !listed.iter().any(|summary| summary.path == path)
        {
            names.push(
                path.file_name()
                    .map_or_else(String::new, |name| name.to_string_lossy().into_owned()),
            );
        }
    }
    Ok(names)
}

/// The messages of the session whose key digest is `session`, indexed once.
fn sources<'a, 'm>(
    indexed: &'m mut HashMap<String, Sources<'a>>,
    (viewer, loaded): (&'a Session, &'a HashMap<String, Option<Session>>),
    session: &str,
) -> Option<&'m mut Sources<'a>> {
    let messages = if digest(&viewer.key) == session {
        Some(viewer.messages.as_slice())
    } else {
        loaded
            .get(session)
            .and_then(Option::as_ref)
            .map(|session: &Session| session.messages.as_slice())
    }?;
    Some(
        indexed
            .entry(session.to_owned())
            .or_insert_with(|| Sources::new(messages)),
    )
}

impl AgentLoop<'_> {
    /// Every saved method, rechecked against the sessions it and its checks
    /// came from as they are now (cleared, trimmed or deleted since, maybe).
    pub(super) fn checked(
        &self,
        viewer: &Session,
    ) -> Result<Vec<Checked>, LearnedMethodStoreError> {
        self.check(
            viewer,
            LearnedMethodStore::new(&self.config.workspace).list()?,
        )
    }

    /// Each session the methods cite is read and indexed once.
    pub(super) fn check(
        &self,
        viewer: &Session,
        methods: Vec<LearnedMethod>,
    ) -> Result<Vec<Checked>, LearnedMethodStoreError> {
        let viewer_digest = digest(&viewer.key);
        let mut cited = BTreeSet::new();
        for method in &methods {
            cited.extend(
                method
                    .current()
                    .map(|version| version.origin.session.clone()),
            );
            cited.extend(
                method
                    .decisions
                    .iter()
                    .filter(|decision| decision.check.is_some())
                    .map(|decision| decision.session.clone()),
            );
        }
        let mut origins = Origins::default();
        let mut loaded = HashMap::new();
        for session in cited
            .into_iter()
            .filter(|session| *session != viewer_digest)
        {
            let found = origins
                .load(&self.sessions, &session)
                .map_err(|error| LearnedMethodStoreError::Unchecked(error.to_string()))?;
            loaded.insert(session, found);
        }
        let mut indexed: HashMap<String, Sources<'_>> = HashMap::new();
        let mut checked = Vec::new();
        for method in methods {
            let Some(version) = method.current() else {
                checked.push(Checked {
                    method,
                    origin_exists: false,
                    seen: BTreeMap::new(),
                    checks: BTreeMap::new(),
                });
                continue;
            };
            let origin = sources(&mut indexed, (viewer, &loaded), &version.origin.session);
            let origin_exists = origin.is_some();
            let seen = recheck(version, origin).map_err(LearnedMethodStoreError::Invalid)?;
            let mut checks = BTreeMap::new();
            for (index, decision) in method.decisions.iter().enumerate() {
                let Some(check) = &decision.check else {
                    continue;
                };
                let Some(checked_version) = method
                    .versions
                    .iter()
                    .find(|candidate| candidate.version == decision.version)
                else {
                    continue;
                };
                let found = sources(&mut indexed, (viewer, &loaded), &decision.session);
                let seen = recheck_check(
                    check,
                    checked_version,
                    (decision.kind, &decision.session),
                    found,
                )
                .map_err(LearnedMethodStoreError::Invalid)?;
                checks.insert(index, seen);
            }
            checked.push(Checked {
                method,
                origin_exists,
                seen,
                checks,
            });
        }
        Ok(checked)
    }
}
