//! `.env.json`, the secret file that `${NAME}` in config falls back to when
//! the process environment has no value (Spec 048 PRD 010 scopes 1 and 2).

use crate::{ConfigError, EnvSource};
use serde::de::{Deserializer, MapAccess, Visitor};
use serde::Deserialize;
use serde_json::error::Category;
use serde_json::{Map, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::{fmt, fs, io};

pub const ENV_FILE_NAME: &str = ".env.json";

/// The `.env.json` in the folder of `config_path`.
pub fn env_file_path(config_path: &Path) -> PathBuf {
    config_path.with_file_name(ENV_FILE_NAME)
}

/// Whether `name` matches `[A-Z_][A-Z0-9_]*`.
pub fn is_env_name(name: &str) -> bool {
    let mut characters = name.chars();
    characters
        .next()
        .is_some_and(|first| first.is_ascii_uppercase() || first == '_')
        && characters.all(|character| {
            character.is_ascii_uppercase() || character.is_ascii_digit() || character == '_'
        })
}

/// The names and values of `.env.json`. `Debug` shows the names only.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct EnvFile {
    values: BTreeMap<String, String>,
}

impl fmt::Debug for EnvFile {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("EnvFile")
            .field("names", &self.values.keys().collect::<Vec<_>>())
            .finish()
    }
}

/// Why `.env.json` could not be used. It never carries the file's names or
/// values; an entry is named by its place in the file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnvFileProblem {
    Unreadable(io::ErrorKind),
    NotJson { line: usize, column: usize },
    NotObject,
    BadName { entry: usize },
    NotText { entry: usize },
    RepeatedName { entry: usize },
}

impl fmt::Display for EnvFileProblem {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unreadable(kind) => write!(formatter, "the file could not be read ({kind})"),
            Self::NotJson { line, column } => {
                write!(formatter, "it is not JSON (line {line}, column {column})")
            }
            Self::NotObject => formatter.write_str("it is not a JSON object"),
            Self::BadName { entry } => write!(
                formatter,
                "entry {entry} has a name outside [A-Z_][A-Z0-9_]*"
            ),
            Self::NotText { entry } => {
                write!(formatter, "entry {entry} has a value that is not text")
            }
            Self::RepeatedName { entry } => {
                write!(formatter, "entry {entry} repeats an earlier name")
            }
        }
    }
}

impl EnvFile {
    /// Reads the `.env.json` next to `config_path`; a missing file is empty.
    pub fn load(config_path: &Path) -> Result<Self, ConfigError> {
        Ok(read_env_file(&env_file_path(config_path))?
            .map(|(file, _)| file)
            .unwrap_or_default())
    }

    /// Parses the file's bytes without putting any of its text in the error.
    pub fn parse(bytes: &[u8]) -> Result<Self, EnvFileProblem> {
        let entries = match serde_json::from_slice::<Entries>(bytes) {
            Ok(Entries(entries)) => entries,
            Err(error) if error.classify() == Category::Data => {
                return Err(EnvFileProblem::NotObject)
            }
            Err(error) => {
                return Err(EnvFileProblem::NotJson {
                    line: error.line(),
                    column: error.column(),
                })
            }
        };
        let mut values = BTreeMap::new();
        for (index, (name, value)) in entries.into_iter().enumerate() {
            let entry = index + 1;
            if !is_env_name(&name) {
                return Err(EnvFileProblem::BadName { entry });
            }
            let Value::String(value) = value else {
                return Err(EnvFileProblem::NotText { entry });
            };
            if values.insert(name, value).is_some() {
                return Err(EnvFileProblem::RepeatedName { entry });
            }
        }
        Ok(Self { values })
    }

    pub fn get(&self, name: &str) -> Option<&str> {
        self.values.get(name).map(String::as_str)
    }

    /// Names and values in name order.
    pub fn entries(&self) -> impl Iterator<Item = (&str, &str)> {
        self.values
            .iter()
            .map(|(name, value)| (name.as_str(), value.as_str()))
    }

    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    pub(crate) fn insert(&mut self, name: String, value: String) {
        self.values.insert(name, value);
    }

    /// The file's text: a JSON object in name order and a final line break.
    pub(crate) fn to_bytes(&self) -> Result<Vec<u8>, ConfigError> {
        let object = self
            .values
            .iter()
            .map(|(name, value)| (name.clone(), Value::String(value.clone())))
            .collect::<Map<_, _>>();
        let mut bytes = serde_json::to_vec_pretty(&Value::Object(object))?;
        bytes.push(b'\n');
        Ok(bytes)
    }
}

/// The file at `path` and its bytes; `None` when it does not exist.
pub(crate) fn read_env_file(path: &Path) -> Result<Option<(EnvFile, Vec<u8>)>, ConfigError> {
    match fs::read(path) {
        Ok(bytes) => EnvFile::parse(&bytes)
            .map(|file| Some((file, bytes)))
            .map_err(ConfigError::EnvFile),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(ConfigError::EnvFile(EnvFileProblem::Unreadable(
            error.kind(),
        ))),
    }
}

/// Where `${NAME}` takes its value: a non-empty value from `first` (the
/// process environment), else `.env.json`, else an empty value from `first`.
pub struct ConfigEnv<'a, E: ?Sized> {
    first: &'a E,
    file: EnvFile,
}

impl<E: EnvSource + ?Sized> EnvSource for ConfigEnv<'_, E> {
    fn var(&self, name: &str) -> Option<String> {
        match self.first.var(name) {
            Some(value) if !value.is_empty() => Some(value),
            first => self.file.get(name).map(str::to_owned).or(first),
        }
    }
}

impl<E: ?Sized> ConfigEnv<'_, E> {
    pub fn env_file(&self) -> &EnvFile {
        &self.file
    }
}

/// The source `${NAME}` in the config at `config_path` resolves from.
pub fn config_env<'a, E: EnvSource + ?Sized>(
    config_path: &Path,
    env: &'a E,
) -> Result<ConfigEnv<'a, E>, ConfigError> {
    Ok(ConfigEnv {
        first: env,
        file: EnvFile::load(config_path)?,
    })
}

/// The entries of a JSON object in file order, so a repeated name is seen.
struct Entries(Vec<(String, Value)>);

impl<'de> Deserialize<'de> for Entries {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_map(EntriesVisitor)
    }
}

struct EntriesVisitor;

impl<'de> Visitor<'de> for EntriesVisitor {
    type Value = Entries;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a JSON object")
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Entries, A::Error> {
        let mut entries = Vec::new();
        while let Some(name) = map.next_key::<String>()? {
            entries.push((name, map.next_value::<Value>()?));
        }
        Ok(Entries(entries))
    }
}

#[cfg(test)]
#[path = "env_file_tests.rs"]
mod tests;
