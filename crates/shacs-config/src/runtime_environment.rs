use crate::{Config, ConfigError, EnvFile, EnvSource};
use std::collections::BTreeMap;
use std::fmt;
use std::path::Path;

/// Startup inputs, kept outside the serializable configuration.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct RuntimeEnvironment {
    pub file: EnvFile,
    process: BTreeMap<String, String>,
}

impl RuntimeEnvironment {
    pub fn resolve(
        config: &mut Config,
        config_path: &Path,
        source: &impl EnvSource,
    ) -> Result<Self, ConfigError> {
        let process = source.snapshot();
        let file = crate::resolve_config_with_env_file(config, config_path, &process)?;
        Ok(Self { file, process })
    }
}

impl EnvSource for RuntimeEnvironment {
    fn var(&self, name: &str) -> Option<String> {
        self.process.get(name).cloned()
    }

    fn snapshot(&self) -> BTreeMap<String, String> {
        self.process.clone()
    }
}

impl fmt::Debug for RuntimeEnvironment {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RuntimeEnvironment")
            .field("file", &self.file)
            .field("process_names", &self.process.keys().collect::<Vec<_>>())
            .finish()
    }
}
