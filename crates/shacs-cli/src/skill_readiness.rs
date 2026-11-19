use crate::CliError;
use shacs_config::{self, ConfigBundle, ConfigError, ProcessEnv};
use shacs_skills::{self, SkillCredentialSources, SkillCredentialStatus, SkillRegistry};
use std::collections::BTreeMap;
use std::path::Path;

pub(crate) fn statuses(
    bundle: &ConfigBundle,
    registry: &SkillRegistry,
) -> Result<Vec<SkillCredentialStatus>, CliError> {
    let source = shacs_config::config_env(&bundle.context.config_path, &ProcessEnv)?;
    let mut explicit = BTreeMap::new();
    let settings = bundle
        .config
        .env
        .iter()
        .chain(&bundle.config.tools.exec.env);
    for (name, value) in settings {
        let value =
            shacs_config::interpolate_env_with_source(value, &source).map_err(ConfigError::Env)?;
        explicit.insert(name.clone(), value);
    }
    Ok(registry
        .entries
        .iter()
        .map(|entry| {
            shacs_skills::resolve_skill_credentials(
                entry,
                &bundle.config.agents.defaults.disabled_skills,
                SkillCredentialSources {
                    process: &ProcessEnv,
                    file: source.env_file(),
                    explicit: &explicit,
                },
            )
            .status
        })
        .collect())
}

pub(crate) fn notice(config_path: &Path) -> String {
    format!(
        "Credential file: {}\nCurrent file/process readiness only; running runtimes require a restart. Edit the file directly; never paste secret values into chat or ask tools to read the secret file. Values of keys declared by active skills are passed to exec commands.",
        shacs_config::env_file_path(config_path).display()
    )
}
