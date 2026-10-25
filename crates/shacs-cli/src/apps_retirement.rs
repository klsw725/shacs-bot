use super::{default_config_path, ArgParser, CliCommand};
use serde_json::Value;
use shacs_config::config_context;
use std::collections::BTreeMap;
use std::fs;
use std::io::{ErrorKind, Read};
use std::path::{Path, PathBuf};

const JSON_READ_LIMIT: u64 = 1024 * 1024;
const POSSIBLY_RUNNING: [&str; 4] = ["starting", "running", "stopping", "recovery_needed"];
const RECORDED_ONLY: [&str; 3] = ["installed", "stopped", "failed"];
const REGISTRY_STATES: [&str; 5] = [
    "installed",
    "enabled",
    "disabled",
    "unavailable",
    "uninstalling",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppsRetiredOptions {
    pub config_path: Option<PathBuf>,
}

pub(super) fn parse(mut parser: ArgParser, global_config: Option<PathBuf>) -> CliCommand {
    let mut config_path = global_config;
    while let Some(argument) = parser.next() {
        if matches!(argument.as_str(), "--config" | "-c") {
            if let Some(value) = parser.next() {
                config_path = Some(PathBuf::from(value));
            }
        }
    }
    CliCommand::AppsRetired(AppsRetiredOptions { config_path })
}

pub(super) fn notice(options: &AppsRetiredOptions) -> String {
    let config_path = options
        .config_path
        .clone()
        .unwrap_or_else(default_config_path);
    let mut data_dir = config_context(Some(config_path), None).data_dir;
    if data_dir.as_os_str().is_empty() {
        data_dir = PathBuf::from(".");
    }
    let shown_dir = printable(&data_dir.to_string_lossy());
    let leftovers = Leftovers::read(&data_dir);
    let mut lines = vec![
        "shacs-bot apps: the app capability has been retired.".to_owned(),
        "`apps` and `app` no longer create, install, enable, start, stop, recover, or remove anything.".to_owned(),
        "Decision: docs/specs/039-app-capability-disposition/SPEC.md (Spec 039); retirement: docs/specs/043-app-capability-retirement/SPEC.md (Spec 043).".to_owned(),
        format!("Data dir: {shown_dir}"),
        "Leftover app data (read only; nothing was created, changed, or deleted):".to_owned(),
    ];
    lines.extend(leftovers.lines);
    lines.push("Apps found in the registry, supervisor journals, or bundles:".to_owned());
    if leftovers.apps.is_empty() {
        lines.push("- none".to_owned());
    }
    for (id, app) in &leftovers.apps {
        lines.push(app.line(id, leftovers.registry_unreadable));
    }
    lines.push(if leftovers.present {
        "Nothing was deleted. You can delete these files yourself; first check for and stop any app process marked above as possibly running, because shacs-bot cannot.".to_owned()
    } else {
        "No leftover app data was found in this data dir.".to_owned()
    });
    lines.push(format!(
        "To give the assistant a procedure or knowledge, add a skill instead: {shown_dir}/skills/<name>/SKILL.md"
    ));
    lines.join("\n")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Node {
    Absent,
    File,
    Dir,
    Symlink,
    Unreadable,
}

/// Looks up `relative` below `data_dir` one component at a time without
/// following any symlink, so a link that points outside the data dir is never read.
fn node(data_dir: &Path, relative: &str) -> Node {
    let mut path = data_dir.to_path_buf();
    let components: Vec<_> = relative.split('/').collect();
    for (index, component) in components.iter().enumerate() {
        path.push(component);
        let kind = match fs::symlink_metadata(&path) {
            Ok(metadata) => kind_of(&metadata),
            Err(error) if error.kind() == ErrorKind::NotFound => return Node::Absent,
            Err(_) => return Node::Unreadable,
        };
        let last = index + 1 == components.len();
        match kind {
            Node::Dir if !last => {}
            Node::Symlink => return Node::Symlink,
            _ if last => return kind,
            _ => return Node::Absent,
        }
    }
    Node::Absent
}

fn kind_of(metadata: &fs::Metadata) -> Node {
    let file_type = metadata.file_type();
    if file_type.is_symlink() {
        Node::Symlink
    } else if file_type.is_dir() {
        Node::Dir
    } else if file_type.is_file() {
        Node::File
    } else {
        Node::Unreadable
    }
}

fn entries(path: &Path) -> Option<Vec<(String, Node)>> {
    let mut entries: Vec<_> = fs::read_dir(path)
        .ok()?
        .map(|entry| {
            entry.ok().map(|entry| {
                let kind = fs::symlink_metadata(entry.path())
                    .map_or(Node::Unreadable, |metadata| kind_of(&metadata));
                (entry.file_name().to_string_lossy().into_owned(), kind)
            })
        })
        .collect::<Option<_>>()?;
    entries.sort_by(|left, right| left.0.cmp(&right.0));
    Some(entries)
}

#[derive(Debug, Default, Clone, Copy)]
struct Count {
    files: usize,
    symlinks: usize,
    unreadable: bool,
}

impl Count {
    fn of(path: &Path) -> Self {
        let mut count = Self::default();
        count.walk(path);
        count
    }

    fn walk(&mut self, path: &Path) {
        let Some(entries) = entries(path) else {
            self.unreadable = true;
            return;
        };
        for (name, kind) in entries {
            self.add_entry(&path.join(name), kind);
        }
    }

    fn add_entry(&mut self, path: &Path, kind: Node) {
        match kind {
            Node::Dir => self.walk(path),
            Node::Symlink => self.symlinks += 1,
            Node::File => self.files += 1,
            Node::Absent | Node::Unreadable => self.unreadable = true,
        }
    }

    fn summary(self) -> String {
        let mut parts = vec![plural(self.files, "file")];
        if self.symlinks > 0 {
            parts.push(format!("{} not followed", plural(self.symlinks, "symlink")));
        }
        if self.unreadable {
            parts.push("some entries unreadable".to_owned());
        }
        parts.join(", ")
    }
}

#[derive(Debug, Clone, Copy)]
enum Journal {
    State(&'static str),
    Unreadable,
}

#[derive(Debug, Default)]
struct App {
    registry: Option<&'static str>,
    journal: Option<Journal>,
    bundle: Option<Node>,
}

impl App {
    fn line(&self, id: &str, registry_unreadable: bool) -> String {
        let registry = match self.registry {
            Some(state) => state,
            None if registry_unreadable => "unknown (registry unreadable)",
            None => "not registered",
        };
        let journal = match self.journal {
            Some(Journal::State(state)) => state,
            Some(Journal::Unreadable) => "unreadable",
            None => "none",
        };
        let bundle = match self.bundle {
            Some(Node::Dir) => "present",
            Some(Node::Symlink) => "symlink, not followed",
            Some(Node::File | Node::Unreadable) => "unreadable",
            Some(Node::Absent) | None => "absent",
        };
        let note = match self.journal {
            Some(Journal::State(state)) if RECORDED_ONLY.contains(&state) => {
                "This is only the last record; it does not guarantee that no app process is running."
            }
            Some(_) => "Its process may still be running; shacs-bot did not check or stop it.",
            None => "No supervisor journal was found.",
        };
        format!(
            "- {id}: registry={registry}, last journal state={journal}, bundle={bundle}. {note}"
        )
    }
}

struct Leftovers {
    lines: Vec<String>,
    apps: BTreeMap<String, App>,
    present: bool,
    registry_unreadable: bool,
}

impl Leftovers {
    fn read(data_dir: &Path) -> Self {
        let mut leftovers = Self {
            lines: Vec::new(),
            apps: BTreeMap::new(),
            present: false,
            registry_unreadable: false,
        };
        leftovers.registry(data_dir);
        leftovers.bundles(data_dir);
        leftovers.journals(data_dir);
        leftovers.drafts(data_dir);
        for relative in [
            "authoring/flow",
            "snapshots/app-lifecycle",
            "runtime/app-ledger",
        ] {
            let kind = node(data_dir, relative);
            let detail = match kind {
                Node::Dir => format!("present, {}", Count::of(&data_dir.join(relative)).summary()),
                other => label(other).to_owned(),
            };
            leftovers.push(&format!("{relative}/"), kind, detail);
        }
        leftovers
    }

    fn push(&mut self, location: &str, kind: Node, detail: String) {
        self.present |= kind != Node::Absent;
        self.lines.push(format!("- {location}: {detail}"));
    }

    fn push_unreadable(&mut self, location: &str) {
        self.push(
            location,
            Node::Unreadable,
            label(Node::Unreadable).to_owned(),
        );
    }

    fn registry(&mut self, data_dir: &Path) {
        let kind = node(data_dir, "apps/registry.json");
        let parsed = (kind == Node::File)
            .then(|| read_json(&data_dir.join("apps/registry.json")))
            .flatten()
            .and_then(|value| value.get("entries")?.as_object().cloned());
        let detail = match (&parsed, kind) {
            (Some(_), _) => "present, 1 file".to_owned(),
            (None, Node::File) => "present, unreadable".to_owned(),
            (None, other) => label(other).to_owned(),
        };
        self.registry_unreadable = kind != Node::Absent && parsed.is_none();
        for (id, entry) in parsed.unwrap_or_default() {
            let state = entry
                .get("lifecycleState")
                .and_then(Value::as_str)
                .and_then(|state| REGISTRY_STATES.into_iter().find(|known| *known == state))
                .unwrap_or("unrecognized");
            self.apps.entry(app_id_label(&id)).or_default().registry = Some(state);
        }
        self.push("apps/registry.json", kind, detail);
    }

    fn bundles(&mut self, data_dir: &Path) {
        let kind = node(data_dir, "apps");
        let mut count = Count::default();
        let mut total = 0;
        if kind == Node::Dir {
            let root = data_dir.join("apps");
            let Some(found) = entries(&root) else {
                self.push_unreadable("apps/<id>.shacsapp/");
                return;
            };
            for (name, entry_kind) in found {
                let Some(id) = name.strip_suffix(".shacsapp") else {
                    continue;
                };
                total += 1;
                count.add_entry(&root.join(&name), entry_kind);
                self.apps.entry(app_id_label(id)).or_default().bundle = Some(entry_kind);
            }
        }
        let (kind, detail) = match (kind, total) {
            (Node::Dir, 0) => (Node::Absent, "absent".to_owned()),
            (Node::Dir, total) => (
                Node::Dir,
                format!("{}, {}", plural(total, "bundle"), count.summary()),
            ),
            (other, _) => (other, label(other).to_owned()),
        };
        self.push("apps/<id>.shacsapp/", kind, detail);
    }

    fn journals(&mut self, data_dir: &Path) {
        let kind = node(data_dir, "apps/app-supervisor");
        if kind != Node::Dir {
            self.push("apps/app-supervisor/", kind, label(kind).to_owned());
            return;
        }
        let root = data_dir.join("apps/app-supervisor");
        let Some(found) = entries(&root) else {
            self.push_unreadable("apps/app-supervisor/");
            return;
        };
        let mut count = Count::default();
        for (name, entry_kind) in &found {
            count.add_entry(&root.join(name), *entry_kind);
            let journal = if *entry_kind == Node::Dir {
                journal_state(data_dir, name)
            } else {
                Journal::Unreadable
            };
            self.apps.entry(app_id_label(name)).or_default().journal = Some(journal);
        }
        self.push(
            "apps/app-supervisor/",
            kind,
            format!("{}, {}", plural(found.len(), "journal"), count.summary()),
        );
    }

    fn drafts(&mut self, data_dir: &Path) {
        let kind = node(data_dir, "authoring/apps");
        if kind != Node::Dir {
            self.push("authoring/apps/", kind, label(kind).to_owned());
            return;
        }
        let root = data_dir.join("authoring/apps");
        let Some(found) = entries(&root) else {
            self.push_unreadable("authoring/apps/");
            return;
        };
        let mut count = Count::default();
        let mut ids = Vec::new();
        for (name, entry_kind) in found {
            count.add_entry(&root.join(&name), entry_kind);
            if let Some(id) = name.strip_prefix("draft-") {
                ids.push(app_id_label(id));
            }
        }
        let drafts = if ids.is_empty() {
            "no drafts".to_owned()
        } else {
            format!("{} ({})", plural(ids.len(), "draft"), ids.join(", "))
        };
        self.push(
            "authoring/apps/",
            kind,
            format!("{drafts}, {}", count.summary()),
        );
    }
}

fn journal_state(data_dir: &Path, id: &str) -> Journal {
    let relative = format!("apps/app-supervisor/{id}/state.json");
    if node(data_dir, &relative) != Node::File {
        return Journal::Unreadable;
    }
    read_json(&data_dir.join(&relative))
        .and_then(|value| {
            let state = value.get("state")?.as_str()?.to_owned();
            POSSIBLY_RUNNING
                .into_iter()
                .chain(RECORDED_ONLY)
                .find(|known| *known == state)
        })
        .map_or(Journal::Unreadable, Journal::State)
}

fn read_json(path: &Path) -> Option<Value> {
    let mut bytes = Vec::new();
    fs::File::open(path)
        .ok()?
        .take(JSON_READ_LIMIT)
        .read_to_end(&mut bytes)
        .ok()?;
    serde_json::from_slice(&bytes).ok()
}

const fn label(kind: Node) -> &'static str {
    match kind {
        Node::Absent => "absent",
        Node::Symlink => "symlink, not followed",
        Node::File | Node::Dir => "present",
        Node::Unreadable => "present, unreadable",
    }
}

fn app_id_label(value: &str) -> String {
    let valid = !value.is_empty()
        && value.len() <= 128
        && value != "."
        && value != ".."
        && value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'_' | b'.')
        });
    if valid {
        value.to_owned()
    } else {
        "<invalid app id>".to_owned()
    }
}

fn plural(count: usize, noun: &str) -> String {
    if count == 1 {
        format!("1 {noun}")
    } else {
        format!("{count} {noun}s")
    }
}

fn printable(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if character.is_control() {
                '?'
            } else {
                character
            }
        })
        .collect()
}
