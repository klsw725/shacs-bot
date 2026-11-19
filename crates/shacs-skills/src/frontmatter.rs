//! A structural reader for the supported skill frontmatter subset.
//! Maps retain duplicate keys so credential validation can reject them.

#[derive(Debug)]
pub(crate) enum Node {
    Scalar(String),
    Text(String),
    Map(Vec<(String, Node)>),
    List(Vec<Node>),
    Invalid,
}

impl Node {
    pub(crate) fn text(&self) -> Option<&str> {
        match self {
            Self::Text(value) => Some(value),
            Self::Scalar(value)
                if !value.is_empty()
                    && !matches!(value.as_str(), "true" | "false" | "null" | "~")
                    && value.parse::<f64>().is_err() =>
            {
                Some(value)
            }
            Self::Scalar(_) | Self::Map(_) | Self::List(_) | Self::Invalid => None,
        }
    }
}

pub(crate) fn parse(raw: &str) -> Result<Node, String> {
    let Some(after_open) = raw.strip_prefix("---") else {
        return Ok(Node::Map(Vec::new()));
    };
    let after_open = after_open.trim_start_matches(['\r', '\n']);
    let Some(close_index) = after_open.find("\n---") else {
        return Err("unterminated skill frontmatter".to_owned());
    };
    let lines: Vec<(usize, &str)> = after_open[..close_index]
        .lines()
        .filter(|line| !line.trim().is_empty() && !line.trim_start().starts_with('#'))
        .map(|line| (line.len() - line.trim_start().len(), line.trim()))
        .collect();
    Ok(block(&lines, &mut 0, 0))
}

fn block(lines: &[(usize, &str)], cursor: &mut usize, indent: usize) -> Node {
    let sequence = lines.get(*cursor).is_some_and(|(_, line)| is_item(line));
    let mut fields = Vec::new();
    let mut items = Vec::new();
    while let Some(&(level, line)) = lines.get(*cursor) {
        if level < indent {
            break;
        }
        if sequence && level == indent && !is_item(line) && mapping_pair(line).is_some() {
            break;
        }
        if level != indent || is_item(line) != sequence {
            *cursor += 1;
            if sequence {
                items.push(Node::Invalid);
            } else {
                fields.push((String::new(), Node::Invalid));
            }
            continue;
        }
        *cursor += 1;
        if sequence {
            let value = line.strip_prefix('-').unwrap_or_default().trim_start();
            if let Some((key, rest)) = mapping_pair(value) {
                let mut entry = vec![(key.to_owned(), nested(rest, lines, cursor, indent + 2))];
                if lines.get(*cursor).is_some_and(|(level, _)| *level > indent) {
                    match block(lines, cursor, indent + 2) {
                        Node::Map(extra) => entry.extend(extra),
                        _ => entry.push((String::new(), Node::Invalid)),
                    }
                }
                items.push(Node::Map(entry));
            } else {
                items.push(nested(value, lines, cursor, indent));
            }
        } else if let Some((key, value)) = mapping_pair(line) {
            fields.push((key.to_owned(), nested(value, lines, cursor, indent)));
        } else {
            fields.push((String::new(), Node::Invalid));
        }
    }
    if sequence {
        Node::List(items)
    } else {
        Node::Map(fields)
    }
}

fn is_item(line: &str) -> bool {
    line == "-" || line.starts_with("- ")
}

fn mapping_pair(line: &str) -> Option<(&str, &str)> {
    let (key, value) = line.split_once(':')?;
    if key.starts_with(['[', '{']) {
        return None;
    }
    let key = key.trim();
    let key = key
        .strip_prefix('"')
        .and_then(|v| v.strip_suffix('"'))
        .or_else(|| key.strip_prefix('\'').and_then(|v| v.strip_suffix('\'')))
        .unwrap_or(key);
    Some((key, value.trim()))
}

fn nested(value: &str, lines: &[(usize, &str)], cursor: &mut usize, indent: usize) -> Node {
    if value.is_empty() {
        if let Some(&(level, line)) = lines.get(*cursor) {
            if level > indent || (level == indent && is_item(line)) {
                return block(lines, cursor, level);
            }
        }
    }
    inline(value)
}

pub(crate) fn inline(value: &str) -> Node {
    let value = without_comment(value.trim());
    if value.starts_with(['&', '*', '!']) || matches!(value, "|" | ">") {
        return Node::Invalid;
    }
    if value.starts_with('"') {
        return serde_json::from_str::<String>(value).map_or(Node::Invalid, Node::Text);
    }
    if value.starts_with('\'') {
        return value
            .strip_prefix('\'')
            .and_then(|v| v.strip_suffix('\''))
            .map_or(Node::Invalid, |v| Node::Text(v.replace("''", "'")));
    }
    if value.starts_with(['[', '{']) {
        let (close, mapping) = if value.starts_with('[') {
            (']', false)
        } else {
            ('}', true)
        };
        let Some(inner) = value.get(1..).and_then(|v| v.strip_suffix(close)) else {
            return Node::Invalid;
        };
        let Some(parts) = split_flow(inner) else {
            return Node::Invalid;
        };
        if mapping {
            let mut fields = Vec::new();
            for part in parts {
                let Some((key, value)) = part.split_once(':') else {
                    return Node::Invalid;
                };
                let key = match inline(key) {
                    Node::Text(key) | Node::Scalar(key) => key,
                    _ => return Node::Invalid,
                };
                fields.push((key, inline(value)));
            }
            return Node::Map(fields);
        }
        return Node::List(parts.into_iter().map(inline).collect());
    }
    Node::Scalar(value.to_owned())
}

fn without_comment(value: &str) -> &str {
    let mut quote = None;
    let mut escaped = false;
    let mut previous_space = true;
    for (index, character) in value.char_indices() {
        if let Some(delimiter) = quote {
            if escaped {
                escaped = false;
            } else if character == '\\' && delimiter == '"' {
                escaped = true;
            } else if character == delimiter {
                quote = None;
            }
        } else if character == '#' && previous_space {
            return value[..index].trim_end();
        } else if matches!(character, '\'' | '"') {
            quote = Some(character);
        }
        previous_space = character.is_whitespace();
    }
    value
}

fn split_flow(value: &str) -> Option<Vec<&str>> {
    if value.trim().is_empty() {
        return Some(Vec::new());
    }
    let mut parts = Vec::new();
    let mut quote = None;
    let mut escaped = false;
    let mut depth = 0usize;
    let mut start = 0;
    for (index, character) in value.char_indices() {
        if let Some(delimiter) = quote {
            if escaped {
                escaped = false;
            } else if character == '\\' && delimiter == '"' {
                escaped = true;
            } else if character == delimiter {
                quote = None;
            }
        } else {
            match character {
                '\'' | '"' => quote = Some(character),
                '[' | '{' => depth += 1,
                ']' | '}' => depth = depth.checked_sub(1)?,
                ',' if depth == 0 => {
                    parts.push(value[start..index].trim());
                    start = index + 1;
                }
                _ => {}
            }
        }
    }
    if quote.is_some() || depth != 0 {
        return None;
    }
    if !value[start..].trim().is_empty() {
        parts.push(value[start..].trim());
    }
    Some(parts)
}

pub(crate) fn flatten(node: &Node) -> std::collections::BTreeMap<String, String> {
    fn visit(node: &Node, path: &str, fields: &mut std::collections::BTreeMap<String, String>) {
        match node {
            Node::Text(value) | Node::Scalar(value) => {
                fields.insert(path.to_owned(), value.clone());
            }
            Node::Map(entries) => {
                for (key, value) in entries {
                    let path = if path.is_empty() {
                        key.clone()
                    } else {
                        format!("{path}.{key}")
                    };
                    visit(value, &path, fields);
                }
            }
            Node::List(items) => {
                let values: Vec<&str> = items.iter().filter_map(Node::text).collect();
                fields.insert(path.to_owned(), values.join(", "));
            }
            Node::Invalid => {}
        }
    }
    let mut fields = std::collections::BTreeMap::new();
    visit(node, "", &mut fields);
    fields
}
