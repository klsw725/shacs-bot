use shacs_config::RememberedPermissionEffect;
use shacs_projection::RememberedPermissionProjection;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RememberedPermissionsView {
    pub title: String,
    pub lines: Vec<String>,
}

impl RememberedPermissionsView {
    pub fn render_plain_text(&self) -> String {
        std::iter::once(self.title.clone())
            .chain(self.lines.clone())
            .collect::<Vec<_>>()
            .join("\n")
    }
}

pub fn remembered_permissions_view(
    projection: &RememberedPermissionProjection,
) -> RememberedPermissionsView {
    let mut lines = vec![
        format!("schema: {}", projection.schema_version),
        format!("status: {}", projection.status),
        format!("rules: {}", projection.rules.len()),
        format!("workspace: {}", projection.workspace_digest_prefix),
    ];
    if let Some(reason) = projection.store_health_reason.as_ref() {
        lines.push(format!("store: {reason}"));
    }
    lines.extend(projection.rules.iter().map(|rule| {
        format!(
            "{} {} {} {} uses={}",
            effect_label(rule.effect),
            rule.matcher_kind,
            rule.rule_id_prefix,
            rule.pattern_summary,
            rule.use_count
        )
    }));

    RememberedPermissionsView {
        title: "remembered permissions".to_owned(),
        lines,
    }
}

const fn effect_label(effect: RememberedPermissionEffect) -> &'static str {
    match effect {
        RememberedPermissionEffect::Allow => "allow",
        RememberedPermissionEffect::Deny => "deny",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shacs_projection::RememberedPermissionRuleProjection;

    const RAW_PATH_SENTINEL: &str = "/Users/alice/secret-workspace";
    const RAW_COMMAND_SENTINEL: &str = "--password sk-secret";
    const RAW_SECRET_SENTINEL: &str = "sk-raw-secret";

    #[test]
    fn remembered_permissions_view_renders_safe_projection_summary_only() {
        let projection = RememberedPermissionProjection {
            schema_version: 1,
            status: "available".to_owned(),
            workspace_digest_prefix: "abcdef123456".to_owned(),
            store_health_reason: Some(
                "remembered permission store is unavailable; inspect the local permission store"
                    .to_owned(),
            ),
            rules: vec![
                RememberedPermissionRuleProjection {
                    rule_id_prefix: "111111111111".to_owned(),
                    effect: RememberedPermissionEffect::Allow,
                    matcher_kind: "exec_prefix".to_owned(),
                    pattern_summary: "exec cargo test *".to_owned(),
                    created_unix_ms: 1,
                    last_used_unix_ms: 2,
                    use_count: 3,
                },
                RememberedPermissionRuleProjection {
                    rule_id_prefix: "222222222222".to_owned(),
                    effect: RememberedPermissionEffect::Deny,
                    matcher_kind: "workspace_path".to_owned(),
                    pattern_summary: "read_file [REDACTED]".to_owned(),
                    created_unix_ms: 4,
                    last_used_unix_ms: 5,
                    use_count: 6,
                },
            ],
        };

        let rendered = remembered_permissions_view(&projection).render_plain_text();

        assert!(rendered.contains("remembered permissions"));
        assert!(rendered.contains("schema: 1"));
        assert!(rendered.contains("status: available"));
        assert!(rendered.contains("rules: 2"));
        assert!(rendered.contains("workspace: abcdef123456"));
        assert!(rendered.contains("allow exec_prefix 111111111111 exec cargo test * uses=3"));
        assert!(rendered.contains("deny workspace_path 222222222222 read_file [REDACTED] uses=6"));
        assert!(rendered.contains("store: remembered permission store is unavailable"));
        assert!(!rendered.contains(RAW_PATH_SENTINEL));
        assert!(!rendered.contains(RAW_COMMAND_SENTINEL));
        assert!(!rendered.contains(RAW_SECRET_SENTINEL));
    }
}
