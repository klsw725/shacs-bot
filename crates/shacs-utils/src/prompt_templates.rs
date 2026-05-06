use std::collections::BTreeMap;
use std::fs;
use std::path::{Component, Path, PathBuf};

use regex::Regex;

pub trait PromptRenderer {
    fn render_template(
        &self,
        name: &str,
        variables: &BTreeMap<String, String>,
    ) -> Result<String, String>;
}

#[derive(Debug, Default, Clone)]
pub struct StaticPromptRenderer {
    templates: BTreeMap<String, String>,
}

#[derive(Debug, Clone)]
pub struct FilePromptRenderer {
    root: PathBuf,
}

impl FilePromptRenderer {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn render_template_strip(
        &self,
        name: &str,
        variables: &BTreeMap<String, String>,
        strip: bool,
    ) -> Result<String, String> {
        let rendered = self.render_file(name, variables, 0)?;
        if strip {
            Ok(rendered.trim_end().to_owned())
        } else {
            Ok(rendered)
        }
    }

    fn render_file(
        &self,
        name: &str,
        variables: &BTreeMap<String, String>,
        depth: usize,
    ) -> Result<String, String> {
        if depth > 16 {
            return Err("template include depth exceeded".to_owned());
        }
        let path = safe_template_path(&self.root, name)?;
        let template = fs::read_to_string(&path).map_err(|error| error.to_string())?;
        let included = self.render_includes(&template, variables, depth)?;
        render_content(&included, variables)
    }

    fn render_includes(
        &self,
        template: &str,
        variables: &BTreeMap<String, String>,
        depth: usize,
    ) -> Result<String, String> {
        let include_re = Regex::new(r#"\{%\s*include\s+['\"]([^'\"]+)['\"]\s*%\}"#)
            .map_err(|error| error.to_string())?;
        let mut rendered = String::new();
        let mut last = 0;
        for captures in include_re.captures_iter(template) {
            let Some(whole) = captures.get(0) else {
                continue;
            };
            let Some(name) = captures.get(1) else {
                continue;
            };
            rendered.push_str(&template[last..whole.start()]);
            rendered.push_str(&self.render_file(name.as_str(), variables, depth + 1)?);
            last = whole.end();
        }
        rendered.push_str(&template[last..]);
        Ok(rendered)
    }
}

impl PromptRenderer for FilePromptRenderer {
    fn render_template(
        &self,
        name: &str,
        variables: &BTreeMap<String, String>,
    ) -> Result<String, String> {
        self.render_template_strip(name, variables, false)
    }
}

impl StaticPromptRenderer {
    pub fn new(templates: BTreeMap<String, String>) -> Self {
        Self { templates }
    }
}

impl PromptRenderer for StaticPromptRenderer {
    fn render_template(
        &self,
        name: &str,
        variables: &BTreeMap<String, String>,
    ) -> Result<String, String> {
        let rendered = self
            .templates
            .get(name)
            .cloned()
            .ok_or_else(|| format!("template not found: {name}"))?;
        Ok(render_content(&rendered, variables)?.trim().to_owned())
    }
}

fn render_content(template: &str, variables: &BTreeMap<String, String>) -> Result<String, String> {
    let raw = template
        .replace("{% raw %}", "")
        .replace("{% endraw %}", "");
    let conditioned = render_conditionals(&raw, variables)?;
    render_variables(&conditioned, variables)
}

fn render_variables(
    template: &str,
    variables: &BTreeMap<String, String>,
) -> Result<String, String> {
    let variable_re =
        Regex::new(r"\{\{\s*([A-Za-z_][A-Za-z0-9_]*)\s*\}\}").map_err(|error| error.to_string())?;
    Ok(variable_re
        .replace_all(template, |captures: &regex::Captures<'_>| {
            variables.get(&captures[1]).cloned().unwrap_or_default()
        })
        .into_owned())
}

fn render_conditionals(
    template: &str,
    variables: &BTreeMap<String, String>,
) -> Result<String, String> {
    let mut stack = Vec::<ConditionalFrame>::new();
    let mut output = Vec::new();
    for line in template.lines() {
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
    if template.ends_with('\n') {
        rendered.push('\n');
    }
    Ok(rendered)
}

#[derive(Debug, Clone, Copy)]
struct ConditionalFrame {
    matched: bool,
    active: bool,
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

fn safe_template_path(root: &Path, name: &str) -> Result<PathBuf, String> {
    if name.contains('\\') || name.contains('\0') {
        return Err(format!("invalid template path: {name}"));
    }
    let relative = Path::new(name);
    if relative.is_absolute()
        || relative
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(format!("invalid template path: {name}"));
    }
    Ok(root.join(relative))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn static_renderer_replaces_basic_variables() -> Result<(), String> {
        let renderer = StaticPromptRenderer::new(BTreeMap::from([(
            "hello.md".to_owned(),
            "Hello {{ name }}".to_owned(),
        )]));
        let rendered = renderer.render_template(
            "hello.md",
            &BTreeMap::from([("name".to_owned(), "shacs-bot".to_owned())]),
        )?;
        assert_eq!(rendered, "Hello shacs-bot");
        Ok(())
    }

    #[test]
    fn file_renderer_handles_includes_variables_and_strip() -> Result<(), String> {
        let root = std::env::temp_dir().join(format!(
            "shacs-utils-template-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_err(|error| error.to_string())?
                .as_nanos()
        ));
        fs::create_dir_all(root.join("agent/_snippets")).map_err(|error| error.to_string())?;
        fs::write(root.join("agent/_snippets/name.md"), "{{ name }}")
            .map_err(|error| error.to_string())?;
        fs::write(
            root.join("agent/identity.md"),
            "Hello {% include 'agent/_snippets/name.md' %}\n{% if channel == 'telegram' %}\nTG\n{% elif channel %}\nOther\n{% else %}\nNone\n{% endif %}\n{% raw %}{{ literal }}{% endraw %}\n",
        )
        .map_err(|error| error.to_string())?;

        let renderer = FilePromptRenderer::new(&root);
        let rendered = renderer.render_template_strip(
            "agent/identity.md",
            &BTreeMap::from([("name".to_owned(), "shacs-bot".to_owned())]),
            true,
        )?;
        assert_eq!(rendered, "Hello shacs-bot\nNone");
        let rendered = renderer.render_template_strip(
            "agent/identity.md",
            &BTreeMap::from([
                ("name".to_owned(), "shacs-bot".to_owned()),
                ("channel".to_owned(), "web".to_owned()),
            ]),
            true,
        )?;
        assert!(rendered.contains("Other"));
        assert!(renderer
            .render_template("../secret.md", &BTreeMap::new())
            .is_err());
        assert!(renderer
            .render_template(r"agent\..\secret.md", &BTreeMap::new())
            .is_err());
        Ok(())
    }
}
