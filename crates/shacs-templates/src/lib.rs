use regex::Regex;
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum WorkspaceTemplate {
    Agents,
    Soul,
    User,
    Tools,
    Heartbeat,
    Memory,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum AgentTemplate {
    Identity,
    PlatformPolicy,
    SkillsSection,
    ConsolidatorArchive,
    DreamPhase1,
    DreamPhase2,
    Evaluator,
    MaxIterationsMessage,
    SubagentAnnounce,
    SubagentSystem,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TemplateSpec {
    pub path: &'static str,
    pub content: &'static str,
    pub variables: &'static [&'static str],
    pub includes: &'static [&'static str],
    pub workspace_destination: Option<&'static str>,
    pub strip_default: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceSyncOutcome {
    pub created_files: Vec<String>,
    pub created_dirs: Vec<String>,
}

pub fn template_variables(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs
        .iter()
        .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
        .collect()
}

pub fn workspace_templates() -> &'static [WorkspaceTemplate] {
    WORKSPACE_TEMPLATES
}

pub fn agent_templates() -> &'static [AgentTemplate] {
    AGENT_TEMPLATES
}

pub fn workspace_template_spec(template: WorkspaceTemplate) -> TemplateSpec {
    match template {
        WorkspaceTemplate::Agents => TemplateSpec {
            path: "AGENTS.md",
            content: AGENTS_MD,
            variables: &[],
            includes: &[],
            workspace_destination: Some("AGENTS.md"),
            strip_default: false,
        },
        WorkspaceTemplate::Soul => TemplateSpec {
            path: "SOUL.md",
            content: SOUL_MD,
            variables: &[],
            includes: &[],
            workspace_destination: Some("SOUL.md"),
            strip_default: false,
        },
        WorkspaceTemplate::User => TemplateSpec {
            path: "USER.md",
            content: USER_MD,
            variables: &[],
            includes: &[],
            workspace_destination: Some("USER.md"),
            strip_default: false,
        },
        WorkspaceTemplate::Tools => TemplateSpec {
            path: "TOOLS.md",
            content: TOOLS_MD,
            variables: &[],
            includes: &[],
            workspace_destination: Some("TOOLS.md"),
            strip_default: false,
        },
        WorkspaceTemplate::Heartbeat => TemplateSpec {
            path: "HEARTBEAT.md",
            content: HEARTBEAT_MD,
            variables: &[],
            includes: &[],
            workspace_destination: Some("HEARTBEAT.md"),
            strip_default: false,
        },
        WorkspaceTemplate::Memory => TemplateSpec {
            path: "memory/MEMORY.md",
            content: MEMORY_MD,
            variables: &[],
            includes: &[],
            workspace_destination: Some("memory/MEMORY.md"),
            strip_default: false,
        },
    }
}

pub fn agent_template_spec(template: AgentTemplate) -> TemplateSpec {
    match template {
        AgentTemplate::Identity => TemplateSpec {
            path: "agent/identity.md",
            content: AGENT_IDENTITY_MD,
            variables: &["runtime", "workspace_path", "platform_policy", "channel"],
            includes: &["agent/_snippets/untrusted_content.md"],
            workspace_destination: None,
            strip_default: false,
        },
        AgentTemplate::PlatformPolicy => TemplateSpec {
            path: "agent/platform_policy.md",
            content: AGENT_PLATFORM_POLICY_MD,
            variables: &["system"],
            includes: &[],
            workspace_destination: None,
            strip_default: false,
        },
        AgentTemplate::SkillsSection => TemplateSpec {
            path: "agent/skills_section.md",
            content: AGENT_SKILLS_SECTION_MD,
            variables: &["skills_summary"],
            includes: &[],
            workspace_destination: None,
            strip_default: false,
        },
        AgentTemplate::ConsolidatorArchive => TemplateSpec {
            path: "agent/consolidator_archive.md",
            content: AGENT_CONSOLIDATOR_ARCHIVE_MD,
            variables: &[],
            includes: &[],
            workspace_destination: None,
            strip_default: true,
        },
        AgentTemplate::DreamPhase1 => TemplateSpec {
            path: "agent/dream_phase1.md",
            content: AGENT_DREAM_PHASE1_MD,
            variables: &["stale_threshold_days"],
            includes: &[],
            workspace_destination: None,
            strip_default: true,
        },
        AgentTemplate::DreamPhase2 => TemplateSpec {
            path: "agent/dream_phase2.md",
            content: AGENT_DREAM_PHASE2_MD,
            variables: &["skill_creator_path"],
            includes: &[],
            workspace_destination: None,
            strip_default: true,
        },
        AgentTemplate::Evaluator => TemplateSpec {
            path: "agent/evaluator.md",
            content: AGENT_EVALUATOR_MD,
            variables: &["part", "task_context", "response"],
            includes: &[],
            workspace_destination: None,
            strip_default: false,
        },
        AgentTemplate::MaxIterationsMessage => TemplateSpec {
            path: "agent/max_iterations_message.md",
            content: AGENT_MAX_ITERATIONS_MESSAGE_MD,
            variables: &["max_iterations"],
            includes: &[],
            workspace_destination: None,
            strip_default: true,
        },
        AgentTemplate::SubagentAnnounce => TemplateSpec {
            path: "agent/subagent_announce.md",
            content: AGENT_SUBAGENT_ANNOUNCE_MD,
            variables: &["label", "status_text", "task", "result"],
            includes: &[],
            workspace_destination: None,
            strip_default: false,
        },
        AgentTemplate::SubagentSystem => TemplateSpec {
            path: "agent/subagent_system.md",
            content: AGENT_SUBAGENT_SYSTEM_MD,
            variables: &["time_ctx", "workspace", "skills_summary"],
            includes: &["agent/_snippets/untrusted_content.md"],
            workspace_destination: None,
            strip_default: false,
        },
    }
}

pub fn render_agent_template(
    template: AgentTemplate,
    variables: &BTreeMap<String, String>,
) -> Result<String, String> {
    let spec = agent_template_spec(template);
    render_template_by_name(spec.path, variables, spec.strip_default)
}

pub fn render_workspace_template(template: WorkspaceTemplate) -> String {
    workspace_template_spec(template).content.to_owned()
}

pub fn render_template_by_name(
    name: &str,
    variables: &BTreeMap<String, String>,
    strip: bool,
) -> Result<String, String> {
    let Some(content) = content_by_path(name) else {
        return Err(format!("template not found: {name}"));
    };
    let rendered = render_content(content, variables)?;
    if strip {
        Ok(rendered.trim_end().to_owned())
    } else {
        Ok(rendered)
    }
}

pub fn sync_workspace_templates(
    workspace: impl AsRef<Path>,
) -> std::io::Result<WorkspaceSyncOutcome> {
    let workspace = workspace.as_ref();
    fs::create_dir_all(workspace)?;
    let mut outcome = WorkspaceSyncOutcome {
        created_files: Vec::new(),
        created_dirs: Vec::new(),
    };

    for template in WORKSPACE_TEMPLATES {
        let spec = workspace_template_spec(*template);
        let Some(destination) = spec.workspace_destination else {
            continue;
        };
        let path = workspace.join(destination);
        if let Some(parent) = path.parent() {
            if !parent.exists() {
                fs::create_dir_all(parent)?;
                outcome
                    .created_dirs
                    .push(parent.to_string_lossy().to_string());
            }
        }
        if !path.exists() {
            fs::write(&path, spec.content)?;
            outcome.created_files.push(destination.to_owned());
        }
    }

    let history = workspace.join("memory").join("history.jsonl");
    if !history.exists() {
        fs::write(&history, "")?;
        outcome
            .created_files
            .push("memory/history.jsonl".to_owned());
    }
    let skills = workspace.join("skills");
    if !skills.exists() {
        fs::create_dir_all(&skills)?;
        outcome.created_dirs.push("skills".to_owned());
    }
    Ok(outcome)
}

const WORKSPACE_TEMPLATES: &[WorkspaceTemplate] = &[
    WorkspaceTemplate::Agents,
    WorkspaceTemplate::Soul,
    WorkspaceTemplate::User,
    WorkspaceTemplate::Tools,
    WorkspaceTemplate::Heartbeat,
    WorkspaceTemplate::Memory,
];

const AGENT_TEMPLATES: &[AgentTemplate] = &[
    AgentTemplate::Identity,
    AgentTemplate::PlatformPolicy,
    AgentTemplate::SkillsSection,
    AgentTemplate::ConsolidatorArchive,
    AgentTemplate::DreamPhase1,
    AgentTemplate::DreamPhase2,
    AgentTemplate::Evaluator,
    AgentTemplate::MaxIterationsMessage,
    AgentTemplate::SubagentAnnounce,
    AgentTemplate::SubagentSystem,
];

fn content_by_path(name: &str) -> Option<&'static str> {
    match name {
        "AGENTS.md" => Some(AGENTS_MD),
        "SOUL.md" => Some(SOUL_MD),
        "USER.md" => Some(USER_MD),
        "TOOLS.md" => Some(TOOLS_MD),
        "HEARTBEAT.md" => Some(HEARTBEAT_MD),
        "memory/MEMORY.md" => Some(MEMORY_MD),
        "agent/identity.md" => Some(AGENT_IDENTITY_MD),
        "agent/platform_policy.md" => Some(AGENT_PLATFORM_POLICY_MD),
        "agent/skills_section.md" => Some(AGENT_SKILLS_SECTION_MD),
        "agent/consolidator_archive.md" => Some(AGENT_CONSOLIDATOR_ARCHIVE_MD),
        "agent/dream_phase1.md" => Some(AGENT_DREAM_PHASE1_MD),
        "agent/dream_phase2.md" => Some(AGENT_DREAM_PHASE2_MD),
        "agent/evaluator.md" => Some(AGENT_EVALUATOR_MD),
        "agent/max_iterations_message.md" => Some(AGENT_MAX_ITERATIONS_MESSAGE_MD),
        "agent/subagent_announce.md" => Some(AGENT_SUBAGENT_ANNOUNCE_MD),
        "agent/subagent_system.md" => Some(AGENT_SUBAGENT_SYSTEM_MD),
        "agent/_snippets/untrusted_content.md" => Some(AGENT_UNTRUSTED_CONTENT_MD),
        _ => None,
    }
}

fn render_content(content: &str, variables: &BTreeMap<String, String>) -> Result<String, String> {
    let raw = content.replace("{% raw %}", "").replace("{% endraw %}", "");
    let included = render_includes(&raw, variables)?;
    let conditioned = render_conditionals(&included, variables)?;
    render_variables(&conditioned, variables)
}

fn render_includes(content: &str, variables: &BTreeMap<String, String>) -> Result<String, String> {
    let include_re = Regex::new(r#"\{%\s*include\s+['\"]([^'\"]+)['\"]\s*%\}"#)
        .map_err(|error| error.to_string())?;
    let mut rendered = String::new();
    let mut last = 0;
    for captures in include_re.captures_iter(content) {
        let Some(whole) = captures.get(0) else {
            continue;
        };
        let Some(name) = captures.get(1) else {
            continue;
        };
        rendered.push_str(&content[last..whole.start()]);
        rendered.push_str(&render_template_by_name(name.as_str(), variables, false)?);
        last = whole.end();
    }
    rendered.push_str(&content[last..]);
    Ok(rendered)
}

fn render_conditionals(
    content: &str,
    variables: &BTreeMap<String, String>,
) -> Result<String, String> {
    let mut stack = Vec::<ConditionalFrame>::new();
    let mut output = Vec::new();
    for line in content.lines() {
        let trimmed = line.trim();
        if let Some(condition) = trimmed
            .strip_prefix("{% if ")
            .and_then(|value| value.strip_suffix(" %}"))
        {
            let active = eval_condition(condition, variables);
            stack.push(ConditionalFrame {
                matched: active,
                active,
            });
            continue;
        }
        if let Some(condition) = trimmed
            .strip_prefix("{% elif ")
            .and_then(|value| value.strip_suffix(" %}"))
        {
            let Some(frame) = stack.last_mut() else {
                return Err("template elif without if".to_owned());
            };
            if frame.matched {
                frame.active = false;
            } else {
                frame.active = eval_condition(condition, variables);
                frame.matched = frame.active;
            }
            continue;
        }
        if trimmed == "{% else %}" {
            let Some(frame) = stack.last_mut() else {
                return Err("template else without if".to_owned());
            };
            frame.active = !frame.matched;
            frame.matched = true;
            continue;
        }
        if trimmed == "{% endif %}" {
            if stack.pop().is_none() {
                return Err("template endif without if".to_owned());
            }
            continue;
        }
        if stack.iter().all(|frame| frame.active) {
            output.push(line.to_owned());
        }
    }
    if !stack.is_empty() {
        return Err("template if block was not closed".to_owned());
    }
    let mut rendered = output.join("\n");
    if content.ends_with('\n') {
        rendered.push('\n');
    }
    Ok(rendered)
}

fn render_variables(content: &str, variables: &BTreeMap<String, String>) -> Result<String, String> {
    let variable_re =
        Regex::new(r"\{\{\s*([A-Za-z_][A-Za-z0-9_]*)\s*\}\}").map_err(|error| error.to_string())?;
    Ok(variable_re
        .replace_all(content, |captures: &regex::Captures<'_>| {
            variables.get(&captures[1]).cloned().unwrap_or_default()
        })
        .into_owned())
}

fn eval_condition(condition: &str, variables: &BTreeMap<String, String>) -> bool {
    condition
        .split(" or ")
        .any(|term| eval_condition_term(term.trim(), variables))
}

fn eval_condition_term(term: &str, variables: &BTreeMap<String, String>) -> bool {
    if let Some((key, expected)) = term.split_once("==") {
        let key = key.trim();
        let expected = expected.trim().trim_matches('"').trim_matches('\'');
        return variables.get(key).is_some_and(|value| value == expected);
    }
    variables.get(term).is_some_and(|value| !value.is_empty())
}

#[derive(Debug, Clone, Copy)]
struct ConditionalFrame {
    matched: bool,
    active: bool,
}

const AGENTS_MD: &str = include_str!("../templates/AGENTS.md");
const SOUL_MD: &str = include_str!("../templates/SOUL.md");
const USER_MD: &str = include_str!("../templates/USER.md");
const TOOLS_MD: &str = include_str!("../templates/TOOLS.md");
const HEARTBEAT_MD: &str = include_str!("../templates/HEARTBEAT.md");
const MEMORY_MD: &str = include_str!("../templates/memory/MEMORY.md");
const AGENT_UNTRUSTED_CONTENT_MD: &str =
    include_str!("../templates/agent/_snippets/untrusted_content.md");
const AGENT_IDENTITY_MD: &str = include_str!("../templates/agent/identity.md");
const AGENT_PLATFORM_POLICY_MD: &str = include_str!("../templates/agent/platform_policy.md");
const AGENT_SKILLS_SECTION_MD: &str = include_str!("../templates/agent/skills_section.md");
const AGENT_CONSOLIDATOR_ARCHIVE_MD: &str =
    include_str!("../templates/agent/consolidator_archive.md");
const AGENT_DREAM_PHASE1_MD: &str = include_str!("../templates/agent/dream_phase1.md");
const AGENT_DREAM_PHASE2_MD: &str = include_str!("../templates/agent/dream_phase2.md");
const AGENT_EVALUATOR_MD: &str = include_str!("../templates/agent/evaluator.md");
const AGENT_MAX_ITERATIONS_MESSAGE_MD: &str =
    include_str!("../templates/agent/max_iterations_message.md");
const AGENT_SUBAGENT_ANNOUNCE_MD: &str = include_str!("../templates/agent/subagent_announce.md");
const AGENT_SUBAGENT_SYSTEM_MD: &str = include_str!("../templates/agent/subagent_system.md");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_identity_includes_conditionals_raw_and_variables() -> Result<(), String> {
        let platform_policy = render_agent_template(
            AgentTemplate::PlatformPolicy,
            &template_variables(&[("system", "Linux")]),
        )?;
        let rendered = render_agent_template(
            AgentTemplate::Identity,
            &template_variables(&[
                ("runtime", "Rust shacs-core"),
                ("workspace_path", "/tmp/work"),
                ("platform_policy", &platform_policy),
                ("channel", "telegram"),
            ]),
        )?;
        assert!(rendered.contains("Rust shacs-core"));
        assert!(rendered.contains("/tmp/work/skills/{skill-name}/SKILL.md"));
        assert!(rendered.contains("Format Hint"));
        assert!(rendered.contains("untrusted external data"));
        Ok(())
    }

    #[test]
    fn renders_evaluator_branches_and_dream_variables() -> Result<(), String> {
        let system = render_agent_template(
            AgentTemplate::Evaluator,
            &template_variables(&[("part", "system")]),
        )?;
        assert!(system.contains("notification gate"));
        assert!(!system.contains("Original task"));
        let user = render_agent_template(
            AgentTemplate::Evaluator,
            &template_variables(&[
                ("part", "user"),
                ("task_context", "check backup"),
                ("response", "done"),
            ]),
        )?;
        assert!(user.contains("check backup"));
        let dream = render_agent_template(
            AgentTemplate::DreamPhase1,
            &template_variables(&[("stale_threshold_days", "14")]),
        )?;
        assert!(dream.contains("N>14"));
        Ok(())
    }

    #[test]
    fn catalog_covers_all_nanobot_markdown_templates() {
        let workspace_paths = workspace_templates()
            .iter()
            .map(|template| workspace_template_spec(*template).path)
            .collect::<Vec<_>>();
        assert_eq!(
            workspace_paths,
            [
                "AGENTS.md",
                "SOUL.md",
                "USER.md",
                "TOOLS.md",
                "HEARTBEAT.md",
                "memory/MEMORY.md"
            ]
        );

        let agent_paths = agent_templates()
            .iter()
            .map(|template| agent_template_spec(*template).path)
            .collect::<Vec<_>>();
        assert_eq!(
            agent_paths,
            [
                "agent/identity.md",
                "agent/platform_policy.md",
                "agent/skills_section.md",
                "agent/consolidator_archive.md",
                "agent/dream_phase1.md",
                "agent/dream_phase2.md",
                "agent/evaluator.md",
                "agent/max_iterations_message.md",
                "agent/subagent_announce.md",
                "agent/subagent_system.md"
            ]
        );

        for path in workspace_paths.into_iter().chain(agent_paths) {
            assert!(content_by_path(path).is_some(), "missing template: {path}");
        }
        assert!(content_by_path("agent/_snippets/untrusted_content.md").is_some());
        assert!(HEARTBEAT_MD.ends_with("\n\n"));
    }

    #[test]
    fn workspace_sync_creates_templates_without_overwrite() -> Result<(), Box<dyn std::error::Error>>
    {
        let root = tempfile::tempdir()?;
        let agents = root.path().join("AGENTS.md");
        fs::write(&agents, "custom")?;
        let outcome = sync_workspace_templates(root.path())?;
        assert_eq!(fs::read_to_string(&agents)?, "custom");
        assert!(outcome.created_files.iter().any(|file| file == "SOUL.md"));
        assert!(root.path().join("memory/MEMORY.md").is_file());
        assert!(root.path().join("memory/history.jsonl").is_file());
        assert!(root.path().join("skills").is_dir());
        assert!(fs::read_to_string(root.path().join("SOUL.md"))?.contains("shacs-bot"));
        assert!(!fs::read_to_string(root.path().join("SOUL.md"))?.contains("nanobot"));
        Ok(())
    }
}
