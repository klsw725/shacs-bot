//! Parses `permissions.allow`, `permissions.ask` and `permissions.deny`: Claude
//! Code permission rules written with shacs-bot tool names (Spec 048 PRD 000
//! scope 4).

use super::exec_pattern::ExecPattern;
use super::host_pattern::HostPattern;
use super::path_pattern::PathPattern;
use glob::Pattern;
use serde::Serialize;
use shacs_redaction::redact_string;

pub const MAX_RULES_PER_LIST: usize = 200;
pub const MAX_RULE_CHARS: usize = 1_000;

const BUILTIN_TOOLS: &[&str] = &[
    "ask_user",
    "cron",
    "edit_file",
    "exec",
    "glob",
    "grep",
    "image_generate",
    "list_dir",
    "memory_search",
    "message",
    "my",
    "notebook_edit",
    "propose_entrusted_work",
    "propose_web_watch",
    "read_file",
    "spawn",
    "web_fetch",
    "web_search",
    "web_watch_status",
    "write_file",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RuleEffect {
    Allow,
    Ask,
    Deny,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ToolSelector {
    Tool(String),
    ToolGlob(Pattern),
    ReadTools,
    EditTools,
    McpServer(String),
    McpServerTools { server: String, tools: Pattern },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RuleSpecifier {
    Any,
    Exec(ExecPattern),
    Path(PathPattern),
    Domain(HostPattern),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PermissionRule {
    pub(crate) text: String,
    pub(crate) tool: ToolSelector,
    pub(crate) specifier: RuleSpecifier,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RuleProblem {
    Empty,
    TooLong,
    TooMany,
    UnbalancedParentheses,
    UnsupportedSpecifier,
    NegatedAllow,
    UnanchoredAllowGlob,
    UnusableAllowPath,
    UnknownTool,
    WildcardBeforeSubcommand,
}

impl RuleProblem {
    /// A warning keeps the rule; every other problem leaves it out.
    pub fn is_warning(self) -> bool {
        matches!(self, Self::UnknownTool | Self::WildcardBeforeSubcommand)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RuleDiagnostic {
    pub list: RuleEffect,
    pub index: usize,
    pub rule: String,
    pub problem: RuleProblem,
}

#[derive(Debug, Clone, Copy)]
pub struct RawRuleLists<'a> {
    pub allow: &'a [String],
    pub ask: &'a [String],
    pub deny: &'a [String],
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UserPermissionRules {
    pub(crate) allow: Vec<PermissionRule>,
    pub(crate) ask: Vec<PermissionRule>,
    pub(crate) deny: Vec<PermissionRule>,
    pub diagnostics: Vec<RuleDiagnostic>,
}

impl UserPermissionRules {
    pub fn parse(lists: RawRuleLists<'_>, mcp_servers: &[String]) -> Self {
        let mut rules = Self::default();
        for (effect, texts) in [
            (RuleEffect::Allow, lists.allow),
            (RuleEffect::Ask, lists.ask),
            (RuleEffect::Deny, lists.deny),
        ] {
            for (index, text) in texts.iter().enumerate() {
                let parsed = if index >= MAX_RULES_PER_LIST {
                    Err(RuleProblem::TooMany)
                } else {
                    parse_rule(text, effect, mcp_servers)
                };
                let problems = match parsed {
                    Ok((rule, warnings)) => {
                        rules.list_mut(effect).push(rule);
                        warnings
                    }
                    Err(problem) => vec![problem],
                };
                rules
                    .diagnostics
                    .extend(problems.into_iter().map(|problem| RuleDiagnostic {
                        list: effect,
                        index,
                        rule: redact_string(&text.chars().take(120).collect::<String>()),
                        problem,
                    }));
            }
        }
        rules
    }

    pub fn rule_texts(&self, effect: RuleEffect) -> Vec<&str> {
        self.list(effect)
            .iter()
            .map(|rule| rule.text.as_str())
            .collect()
    }

    pub(crate) fn list(&self, effect: RuleEffect) -> &[PermissionRule] {
        match effect {
            RuleEffect::Allow => &self.allow,
            RuleEffect::Ask => &self.ask,
            RuleEffect::Deny => &self.deny,
        }
    }

    fn list_mut(&mut self, effect: RuleEffect) -> &mut Vec<PermissionRule> {
        match effect {
            RuleEffect::Allow => &mut self.allow,
            RuleEffect::Ask => &mut self.ask,
            RuleEffect::Deny => &mut self.deny,
        }
    }
}

fn parse_rule(
    raw: &str,
    effect: RuleEffect,
    mcp_servers: &[String],
) -> Result<(PermissionRule, Vec<RuleProblem>), RuleProblem> {
    let text = raw.trim();
    if text.is_empty() {
        return Err(RuleProblem::Empty);
    }
    if text.chars().count() > MAX_RULE_CHARS {
        return Err(RuleProblem::TooLong);
    }
    let (name, specifier) = match text.find('(') {
        Some(open) => {
            let inner = text[open + 1..]
                .strip_suffix(')')
                .ok_or(RuleProblem::UnbalancedParentheses)?;
            (text[..open].trim(), Some(inner.trim()))
        }
        None if text.ends_with(')') => return Err(RuleProblem::UnbalancedParentheses),
        None => (text, None),
    };
    if name.is_empty() {
        return Err(RuleProblem::Empty);
    }
    let tool = tool_selector(name, effect, mcp_servers)?;
    let specifier = rule_specifier(&tool, specifier, effect)?;
    let mut warnings = Vec::new();
    if let ToolSelector::Tool(name) = &tool {
        if !BUILTIN_TOOLS.contains(&name.as_str()) && !name.starts_with("mcp_") {
            warnings.push(RuleProblem::UnknownTool);
        }
    }
    if let (RuleEffect::Allow, RuleSpecifier::Exec(pattern)) = (effect, &specifier) {
        if pattern.wildcard_before_subcommand() {
            warnings.push(RuleProblem::WildcardBeforeSubcommand);
        }
    }
    let rule = PermissionRule {
        text: text.to_owned(),
        tool,
        specifier,
    };
    Ok((rule, warnings))
}

fn tool_selector(
    name: &str,
    effect: RuleEffect,
    mcp_servers: &[String],
) -> Result<ToolSelector, RuleProblem> {
    match name {
        "read" => return Ok(ToolSelector::ReadTools),
        "edit" => return Ok(ToolSelector::EditTools),
        _ => {}
    }
    if let Some(server) = mcp_servers
        .iter()
        .find(|server| name == format!("mcp_{server}"))
    {
        return Ok(ToolSelector::McpServer(server.clone()));
    }
    if !name.contains(['*', '?', '[']) {
        return Ok(ToolSelector::Tool(name.to_owned()));
    }
    if effect != RuleEffect::Allow {
        return Pattern::new(name)
            .map(ToolSelector::ToolGlob)
            .map_err(|_| RuleProblem::UnsupportedSpecifier);
    }
    mcp_servers
        .iter()
        .filter(|server| !server.contains(['*', '?', '[']))
        .find_map(|server| {
            let tools = name.strip_prefix(&format!("mcp_{server}_"))?;
            let tools = Pattern::new(tools).ok()?;
            Some(ToolSelector::McpServerTools {
                server: server.clone(),
                tools,
            })
        })
        .ok_or(RuleProblem::UnanchoredAllowGlob)
}

fn rule_specifier(
    tool: &ToolSelector,
    specifier: Option<&str>,
    effect: RuleEffect,
) -> Result<RuleSpecifier, RuleProblem> {
    let Some(specifier) = specifier else {
        return Ok(RuleSpecifier::Any);
    };
    match tool {
        ToolSelector::Tool(name) if name == "exec" => {
            if specifier == "*" {
                return Ok(RuleSpecifier::Any);
            }
            ExecPattern::parse(specifier)
                .map(RuleSpecifier::Exec)
                .ok_or(RuleProblem::UnsupportedSpecifier)
        }
        ToolSelector::ReadTools | ToolSelector::EditTools => {
            let pattern = PathPattern::parse(specifier);
            match effect {
                RuleEffect::Allow if pattern.negated => Err(RuleProblem::NegatedAllow),
                RuleEffect::Allow if !pattern.usable() => Err(RuleProblem::UnusableAllowPath),
                _ => Ok(RuleSpecifier::Path(pattern)),
            }
        }
        ToolSelector::Tool(name) if name == "web_fetch" => specifier
            .strip_prefix("domain:")
            .and_then(HostPattern::parse)
            .map(RuleSpecifier::Domain)
            .ok_or(RuleProblem::UnsupportedSpecifier),
        _ => Err(RuleProblem::UnsupportedSpecifier),
    }
}

#[cfg(test)]
#[path = "permission_rule_tests.rs"]
mod tests;
