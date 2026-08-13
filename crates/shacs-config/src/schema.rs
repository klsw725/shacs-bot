use serde_json::Value;

pub const CURRENT_CONFIG_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigSchemaState {
    Legacy,
    Current,
    FutureUnsupported { found: u32 },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigSchemaError {
    Invalid,
}

pub fn classify_config_schema(value: &Value) -> Result<ConfigSchemaState, ConfigSchemaError> {
    let Some(version) = value.get("schemaVersion") else {
        return Ok(ConfigSchemaState::Legacy);
    };
    let version = version.as_u64().ok_or(ConfigSchemaError::Invalid)?;
    let version = u32::try_from(version).map_err(|_| ConfigSchemaError::Invalid)?;
    Ok(match version {
        0 => ConfigSchemaState::Legacy,
        CURRENT_CONFIG_SCHEMA_VERSION => ConfigSchemaState::Current,
        found => ConfigSchemaState::FutureUnsupported { found },
    })
}

pub(crate) const fn default_config_schema_version() -> u32 {
    CURRENT_CONFIG_SCHEMA_VERSION
}
