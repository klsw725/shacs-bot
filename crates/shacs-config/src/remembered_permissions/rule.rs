use super::canonical::rule_id;
use super::{RememberedPermissionEffect, RememberedPermissionMatcher};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RememberedPermissionRuleId(pub(super) String);

impl RememberedPermissionRuleId {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RememberedPermissionRule {
    id: RememberedPermissionRuleId,
    effect: RememberedPermissionEffect,
    matcher: RememberedPermissionMatcher,
    created_unix_ms: u64,
    last_used_unix_ms: u64,
    use_count: u64,
}

impl RememberedPermissionRule {
    pub fn new(
        effect: RememberedPermissionEffect,
        matcher: RememberedPermissionMatcher,
        created_unix_ms: u64,
    ) -> Self {
        let id = rule_id(effect, &matcher);
        Self {
            id,
            effect,
            matcher,
            created_unix_ms,
            last_used_unix_ms: created_unix_ms,
            use_count: 0,
        }
    }

    pub fn id(&self) -> &RememberedPermissionRuleId {
        &self.id
    }

    pub const fn effect(&self) -> RememberedPermissionEffect {
        self.effect
    }

    pub fn matcher(&self) -> &RememberedPermissionMatcher {
        &self.matcher
    }

    pub const fn created_unix_ms(&self) -> u64 {
        self.created_unix_ms
    }

    pub const fn last_used_unix_ms(&self) -> u64 {
        self.last_used_unix_ms
    }

    pub const fn use_count(&self) -> u64 {
        self.use_count
    }

    pub fn mark_used(&mut self, used_unix_ms: u64) {
        self.last_used_unix_ms = used_unix_ms;
        self.use_count = self.use_count.saturating_add(1);
    }

    pub(super) fn has_valid_id(&self) -> bool {
        self.id == rule_id(self.effect, &self.matcher)
    }
}
