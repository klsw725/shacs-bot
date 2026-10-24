use super::learned_method::{
    LearnedMethod, LearnedMethodState, LearnedMethodVersion, LEARNED_METHOD_SCHEMA,
};
use super::learned_method_decisions::decisions_hold;
use super::learned_method_validate::{consistent, tiny, within_limits};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

const STORE_DIR: &str = ".shacs-methods";
const STORE_FILE: &str = "methods.json";
const LOCK_FILE: &str = ".lock";
const MAX_METHODS: usize = 200;
const MAX_STORE_BYTES: u64 = 16 * 1024 * 1024;
pub(super) const MAX_VERSIONS: usize = 64;

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoreFile {
    schema: String,
    next_id: u64,
    methods: Vec<LearnedMethod>,
}

#[derive(Debug)]
pub enum LearnedMethodStoreError {
    Io(io::Error),
    Invalid(&'static str),
    /// The sessions the methods cite could not be read, so the methods could
    /// not be checked against them.
    Unchecked(String),
    Full,
}

impl std::fmt::Display for LearnedMethodStoreError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "learned method store I/O failed: {error}"),
            Self::Invalid(reason) => write!(
                formatter,
                "learned method store is invalid ({reason}) and was left unchanged"
            ),
            Self::Unchecked(reason) => write!(
                formatter,
                "learned methods could not be checked against their sessions ({reason}); nothing was changed"
            ),
            Self::Full => write!(
                formatter,
                "learned method store already holds {MAX_METHODS} methods"
            ),
        }
    }
}

impl std::error::Error for LearnedMethodStoreError {}

impl From<io::Error> for LearnedMethodStoreError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

/// User-owned learned methods of one workspace, kept apart from memory
/// (`memory/`, `USER.md`) and from skill files.
pub struct LearnedMethodStore {
    dir: PathBuf,
}

impl LearnedMethodStore {
    pub fn new(workspace: &Path) -> Self {
        Self {
            dir: workspace.join(STORE_DIR),
        }
    }

    pub fn list(&self) -> Result<Vec<LearnedMethod>, LearnedMethodStoreError> {
        Ok(self.read()?.map(|file| file.methods).unwrap_or_default())
    }

    pub fn add_candidate(
        &self,
        version: LearnedMethodVersion,
    ) -> Result<LearnedMethod, LearnedMethodStoreError> {
        reject_symlink(&self.dir)?;
        fs::create_dir_all(&self.dir)?;
        let _lock = self.lock()?;
        let mut file = self.read()?.unwrap_or(StoreFile {
            schema: LEARNED_METHOD_SCHEMA.to_owned(),
            next_id: 1,
            methods: Vec::new(),
        });
        if file.methods.len() >= MAX_METHODS {
            return Err(LearnedMethodStoreError::Full);
        }
        let method_id = format!("method-{}", file.next_id);
        file.next_id = file
            .next_id
            .checked_add(1)
            .ok_or(LearnedMethodStoreError::Invalid("method id sequence"))?;
        if file
            .methods
            .iter()
            .any(|method| method.method_id == method_id)
        {
            return Err(LearnedMethodStoreError::Invalid("method id sequence"));
        }
        let method = LearnedMethod {
            method_id,
            state: LearnedMethodState::Candidate,
            revision: 1,
            current_version: version.version,
            created_at: version.created_at.clone(),
            versions: vec![version],
            decisions: Vec::new(),
        };
        file.methods.push(method.clone());
        self.write(&file)?;
        Ok(method)
    }

    /// Changes one method as it is stored now, under the store lock, and
    /// saves it with its revision raised. `change` refuses with a message by
    /// returning `Err`; then nothing is written.
    pub fn update(
        &self,
        method_id: &str,
        change: impl FnOnce(&mut LearnedMethod) -> Result<(), String>,
    ) -> Result<Result<LearnedMethod, String>, LearnedMethodStoreError> {
        reject_symlink(&self.dir)?;
        fs::create_dir_all(&self.dir)?;
        let _lock = self.lock()?;
        let mut file = self.read()?;
        let method = file.as_mut().and_then(|file| {
            file.methods
                .iter_mut()
                .find(|method| method.method_id == method_id)
        });
        let Some(method) = method else {
            return Ok(Err(format!("No learned method {method_id}.")));
        };
        if let Err(reason) = change(method) {
            return Ok(Err(reason));
        }
        method.revision = method
            .revision
            .checked_add(1)
            .ok_or(LearnedMethodStoreError::Invalid("revision"))?;
        let changed = method.clone();
        if let Some(file) = &file {
            self.write(file)?;
        }
        Ok(Ok(changed))
    }

    fn read(&self) -> Result<Option<StoreFile>, LearnedMethodStoreError> {
        let path = self.dir.join(STORE_FILE);
        reject_symlink(&self.dir)?;
        reject_symlink(&path)?;
        match fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.len() > MAX_STORE_BYTES => {
                return Err(LearnedMethodStoreError::Invalid("store file is too large"))
            }
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.into()),
        }
        let bytes = fs::read(&path)?;
        let file: StoreFile = serde_json::from_slice(&bytes)
            .map_err(|_| LearnedMethodStoreError::Invalid("unreadable store file"))?;
        validate(&file)?;
        Ok(Some(file))
    }

    /// Writes only what `read` would accept again, so a saved candidate
    /// cannot make the store unreadable after a restart.
    fn write(&self, file: &StoreFile) -> Result<(), LearnedMethodStoreError> {
        validate(file)?;
        let bytes = serde_json::to_vec_pretty(file).map_err(io::Error::other)?;
        if u64::try_from(bytes.len()).map_or(true, |len| len > MAX_STORE_BYTES) {
            return Err(LearnedMethodStoreError::Invalid("store file is too large"));
        }
        let mut temp = tempfile::NamedTempFile::new_in(&self.dir)?;
        temp.write_all(&bytes)?;
        temp.as_file().sync_all()?;
        temp.persist(self.dir.join(STORE_FILE))
            .map_err(|error| error.error)?;
        File::open(&self.dir)?.sync_all()?;
        Ok(())
    }

    fn lock(&self) -> Result<File, LearnedMethodStoreError> {
        let path = self.dir.join(LOCK_FILE);
        reject_symlink(&path)?;
        let lock = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)?;
        lock.lock_exclusive()?;
        Ok(lock)
    }
}

fn validate(file: &StoreFile) -> Result<(), LearnedMethodStoreError> {
    if file.schema != LEARNED_METHOD_SCHEMA {
        return Err(LearnedMethodStoreError::Invalid("unknown store schema"));
    }
    if file.methods.len() > MAX_METHODS {
        return Err(LearnedMethodStoreError::Invalid("too many methods"));
    }
    let mut ids = BTreeSet::new();
    for method in &file.methods {
        let numbered = method
            .method_id
            .strip_prefix("method-")
            .is_some_and(|number| !number.is_empty() && number.bytes().all(|b| b.is_ascii_digit()));
        let versions: BTreeSet<u32> = method
            .versions
            .iter()
            .map(|version| version.version)
            .collect();
        if !ids.insert(method.method_id.as_str())
            || !numbered
            || method.revision == 0
            || !(1..=MAX_VERSIONS).contains(&method.versions.len())
            || versions.len() != method.versions.len()
            || method.current().is_none()
            || !tiny(&method.created_at)
            || !method
                .versions
                .iter()
                .all(|version| within_limits(version) && consistent(version))
            || !decisions_hold(method)
        {
            return Err(LearnedMethodStoreError::Invalid("malformed method record"));
        }
    }
    Ok(())
}

fn reject_symlink(path: &Path) -> Result<(), LearnedMethodStoreError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(LearnedMethodStoreError::Invalid(
            "store path is a symbolic link",
        )),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}
