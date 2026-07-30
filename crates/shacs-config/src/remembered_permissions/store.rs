use super::canonical::contains_forbidden_raw_field;
use super::{
    RememberedPermissionRule, RememberedPermissionRuleId, RememberedPermissionStoreError,
    RememberedPermissionStoreErrorKind, WorkspacePermissionId, SCHEMA_VERSION_V1,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RememberedPermissionProject {
    rules: Vec<RememberedPermissionRule>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RememberedPermissionStore {
    schema_version: u32,
    projects: BTreeMap<WorkspacePermissionId, RememberedPermissionProject>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RememberedPermissionRemoveByPrefixOutcome {
    Removed(RememberedPermissionRule),
    Missing,
    Ambiguous,
}

impl Default for RememberedPermissionStore {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION_V1,
            projects: BTreeMap::new(),
        }
    }
}

impl RememberedPermissionStore {
    pub const fn schema_version(&self) -> u32 {
        self.schema_version
    }

    pub fn project(
        &self,
        workspace_id: &WorkspacePermissionId,
    ) -> Option<&[RememberedPermissionRule]> {
        self.projects
            .get(workspace_id)
            .map(|project| project.rules.as_slice())
    }

    pub fn upsert_rule(
        &mut self,
        workspace_id: WorkspacePermissionId,
        rule: RememberedPermissionRule,
    ) {
        let rules = &mut self.projects.entry(workspace_id).or_default().rules;
        if let Some(existing) = rules
            .iter_mut()
            .find(|existing| existing.matcher() == rule.matcher())
        {
            *existing = rule;
            return;
        }
        rules.push(rule);
    }

    pub fn remove_rule(
        &mut self,
        workspace_id: &WorkspacePermissionId,
        rule_id: &RememberedPermissionRuleId,
    ) -> bool {
        let Some(project) = self.projects.get_mut(workspace_id) else {
            return false;
        };
        let original_len = project.rules.len();
        project.rules.retain(|rule| rule.id() != rule_id);
        project.rules.len() != original_len
    }

    pub fn remove_rule_by_prefix(
        &mut self,
        workspace_id: &WorkspacePermissionId,
        rule_id_prefix: &str,
    ) -> RememberedPermissionRemoveByPrefixOutcome {
        let Some(project) = self.projects.get_mut(workspace_id) else {
            return RememberedPermissionRemoveByPrefixOutcome::Missing;
        };
        let mut matches = project
            .rules
            .iter()
            .enumerate()
            .filter(|(_index, rule)| rule.id().as_str().starts_with(rule_id_prefix));
        let Some((matched_index, _rule)) = matches.next() else {
            return RememberedPermissionRemoveByPrefixOutcome::Missing;
        };
        if matches.next().is_some() {
            return RememberedPermissionRemoveByPrefixOutcome::Ambiguous;
        }
        RememberedPermissionRemoveByPrefixOutcome::Removed(project.rules.remove(matched_index))
    }

    pub fn mark_rule_used(
        &mut self,
        workspace_id: &WorkspacePermissionId,
        rule_id: &str,
        used_unix_ms: u64,
    ) -> bool {
        let Some(project) = self.projects.get_mut(workspace_id) else {
            return false;
        };
        let Some(rule) = project
            .rules
            .iter_mut()
            .find(|rule| rule.id().as_str() == rule_id)
        else {
            return false;
        };
        rule.mark_used(used_unix_ms);
        true
    }

    pub fn enforce_project_rule_limit(
        &self,
        limit: usize,
    ) -> Result<(), RememberedPermissionStoreError> {
        if self
            .projects
            .values()
            .any(|project| project.rules.len() > limit)
        {
            return Err(RememberedPermissionStoreError::new(
                RememberedPermissionStoreErrorKind::ProjectRuleLimitExceeded,
            ));
        }
        Ok(())
    }

    pub fn from_json_str(input: &str) -> Result<Self, RememberedPermissionStoreError> {
        let value: serde_json::Value = serde_json::from_str(input).map_err(|_| {
            RememberedPermissionStoreError::new(RememberedPermissionStoreErrorKind::Malformed)
        })?;
        if contains_forbidden_raw_field(&value) {
            return Err(RememberedPermissionStoreError::new(
                RememberedPermissionStoreErrorKind::ForbiddenRawField,
            ));
        }
        let schema_version = value
            .get("schemaVersion")
            .and_then(serde_json::Value::as_u64)
            .ok_or_else(|| {
                RememberedPermissionStoreError::new(RememberedPermissionStoreErrorKind::Malformed)
            })?;
        if schema_version != u64::from(SCHEMA_VERSION_V1) {
            return Err(RememberedPermissionStoreError::new(
                RememberedPermissionStoreErrorKind::UnknownSchemaVersion,
            ));
        }
        let store: Self = serde_json::from_value(value).map_err(|_| {
            RememberedPermissionStoreError::new(RememberedPermissionStoreErrorKind::Malformed)
        })?;
        store.validate()?;
        Ok(store)
    }

    pub fn to_json_string(&self) -> Result<String, RememberedPermissionStoreError> {
        self.validate()?;
        serde_json::to_string_pretty(self).map_err(|_| {
            RememberedPermissionStoreError::new(RememberedPermissionStoreErrorKind::Malformed)
        })
    }

    fn validate(&self) -> Result<(), RememberedPermissionStoreError> {
        if self.schema_version != SCHEMA_VERSION_V1 {
            return Err(RememberedPermissionStoreError::new(
                RememberedPermissionStoreErrorKind::UnknownSchemaVersion,
            ));
        }
        for rule in self.projects.values().flat_map(|project| &project.rules) {
            if !rule.has_valid_id() {
                return Err(RememberedPermissionStoreError::new(
                    RememberedPermissionStoreErrorKind::RuleIdMismatch,
                ));
            }
        }
        Ok(())
    }
}
