use super::{RememberedPermissionProjection, RememberedPermissionRuleProjection};
use shacs_config::RememberedPermissionEffect;

pub fn format_remembered_permission_projection(
    projection: &RememberedPermissionProjection,
) -> String {
    let mut lines = vec![
        "Remembered permissions".to_owned(),
        format!("Status: {}", projection.status),
        format!("Workspace: {}", projection.workspace_digest_prefix),
        format!("Rules: {}", projection.rules.len()),
    ];
    lines.extend(
        projection
            .rules
            .iter()
            .map(format_remembered_permission_rule_line),
    );
    lines.join("\n")
}

pub fn format_remembered_permission_rule(
    heading: &str,
    rule: &RememberedPermissionRuleProjection,
) -> String {
    [
        heading.to_owned(),
        format_remembered_permission_rule_line(rule),
    ]
    .join("\n")
}

fn format_remembered_permission_rule_line(rule: &RememberedPermissionRuleProjection) -> String {
    format!(
        "- {} {} {} {} created={} last_used={} use_count={}",
        rule.rule_id_prefix,
        remembered_effect_label(rule.effect),
        rule.matcher_kind,
        rule.pattern_summary,
        rule.created_unix_ms,
        rule.last_used_unix_ms,
        rule.use_count
    )
}

fn remembered_effect_label(effect: RememberedPermissionEffect) -> &'static str {
    match effect {
        RememberedPermissionEffect::Allow => "allow",
        RememberedPermissionEffect::Deny => "deny",
    }
}
