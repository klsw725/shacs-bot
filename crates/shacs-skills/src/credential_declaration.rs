use crate::frontmatter::{self, Node};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

const ENV_PATHS: [&str; 4] = [
    "prerequisites.env_vars",
    "requires.env",
    "metadata.nanobot.requires.env",
    "metadata.openclaw.requires.env",
];
const HERMES: &str = "required_environment_variables";

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillCredentialDeclaration {
    pub required: Vec<String>,
    pub reserved: Vec<String>,
    pub diagnostics: Vec<String>,
}

impl SkillCredentialDeclaration {
    pub(crate) fn from_frontmatter(root: &Node) -> Self {
        let mut declaration = Self::default();
        let mut seen = BTreeSet::new();
        declaration.visit(root, "", &mut seen);
        if !declaration.diagnostics.is_empty() {
            declaration.required.clear();
            declaration.reserved.clear();
        }
        declaration.required.sort();
        declaration.required.dedup();
        declaration.reserved.sort();
        declaration.reserved.dedup();
        declaration
    }

    fn visit(&mut self, node: &Node, path: &str, seen: &mut BTreeSet<String>) {
        if ENV_PATHS.contains(&path) {
            self.names(node, path);
            return;
        }
        if path == HERMES {
            self.hermes(node);
            return;
        }
        // Legacy metadata may be a quoted JSON object.
        if path == "metadata" {
            if let Node::Text(text) = node {
                self.visit(&frontmatter::inline(text), path, seen);
                return;
            }
        }
        let Node::Map(fields) = node else {
            self.problem(path, None, "expected mapping");
            return;
        };
        for (key, value) in fields {
            let child = if path.is_empty() {
                key.clone()
            } else {
                format!("{path}.{key}")
            };
            let relevant = ENV_PATHS
                .iter()
                .chain([&HERMES])
                .any(|target| *target == child || target.starts_with(&format!("{child}.")));
            if key.is_empty() {
                self.problem(path, None, "unsupported mapping syntax");
            }
            if key == "<<" {
                self.problem(path, None, "unsupported merge key");
            }
            if !relevant {
                continue;
            }
            if !seen.insert(child.clone()) {
                self.problem(&child, None, "duplicate field");
            }
            self.visit(value, &child, seen);
        }
    }

    fn names(&mut self, node: &Node, path: &str) {
        match node {
            Node::List(items) => {
                let indexed_items = items.iter().enumerate();
                for (index, item) in indexed_items {
                    self.name(item, path, Some(index + 1));
                }
            }
            Node::Text(_) | Node::Scalar(_) if path != "prerequisites.env_vars" => {
                if let Some(text) = node.text() {
                    let names = text.split(',').enumerate();
                    for (index, name) in names {
                        self.name(&Node::Text(name.trim().to_owned()), path, Some(index + 1));
                    }
                } else {
                    self.problem(path, None, "expected string");
                }
            }
            Node::Text(_) | Node::Scalar(_) | Node::Map(_) | Node::Invalid => {
                self.problem(path, None, "expected string list");
            }
        }
    }

    fn hermes(&mut self, node: &Node) {
        let Node::List(items) = node else {
            self.problem(HERMES, None, "expected mapping list");
            return;
        };
        let indexed_items = items.iter().enumerate();
        for (index, item) in indexed_items {
            let item_number = Some(index + 1);
            let Node::Map(fields) = item else {
                self.problem(HERMES, item_number, "expected mapping");
                continue;
            };
            let mut seen = BTreeSet::new();
            let mut has_name = false;
            for (key, value) in fields {
                if !seen.insert(key) {
                    self.problem(HERMES, item_number, "duplicate field");
                }
                match key.as_str() {
                    "name" => {
                        has_name = true;
                        self.name(value, HERMES, item_number);
                    }
                    "prompt" | "help" if value.text().is_none() => {
                        self.problem(HERMES, item_number, "expected description string");
                    }
                    "<<" => self.problem(HERMES, item_number, "unsupported merge key"),
                    _ => {
                        if unsupported(value) {
                            self.problem(HERMES, item_number, "unsupported syntax");
                        }
                    }
                }
            }
            if !has_name {
                self.problem(HERMES, item_number, "missing name");
            }
        }
    }

    fn name(&mut self, node: &Node, path: &str, item: Option<usize>) {
        let Some(name) = node.text() else {
            self.problem(path, item, "expected string");
            return;
        };
        if !shacs_config::is_env_name(name) {
            self.problem(path, item, "invalid environment name");
        } else if matches!(name, "HOME" | "PATH" | "LANG" | "TERM") {
            self.reserved.push(name.to_owned());
        } else {
            self.required.push(name.to_owned());
        }
    }

    fn problem(&mut self, path: &str, item: Option<usize>, kind: &str) {
        let item = item
            .map(|index| format!(" item {index}"))
            .unwrap_or_default();
        self.diagnostics.push(format!(
            "credential declaration invalid: {path}{item}: {kind}"
        ));
    }
}

fn unsupported(node: &Node) -> bool {
    match node {
        Node::Invalid => true,
        Node::Map(fields) => fields
            .iter()
            .any(|(key, value)| key == "<<" || unsupported(value)),
        Node::List(items) => items.iter().any(unsupported),
        Node::Text(_) | Node::Scalar(_) => false,
    }
}
