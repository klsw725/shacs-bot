use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use shacs_workflow::{
    workflow_recipe_readiness, WorkflowPattern, WorkflowRecipe, WorkflowRecipeReadiness,
};

pub const BUILTIN_SKILLS_DIR: &str = "builtin_skills";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BuiltinSkill {
    pub name: &'static str,
    pub files: &'static [BuiltinSkillFile],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BuiltinSkillFile {
    pub relative_path: &'static str,
    pub content: &'static [u8],
    pub executable: bool,
}

mod builtins_generated;
use builtins_generated::{BUILTIN_SKILLS, DEFERRED_BUILTIN_SKILLS};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuiltinSkillsSyncOutcome {
    pub created_files: Vec<String>,
    pub created_dirs: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillSourceKind {
    VirtualBuiltin,
    MaterializedBuiltin,
    UserGlobal,
    WorkspaceLegacy,
    WorkspaceLocal,
    PluginProvided,
}

impl SkillSourceKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::VirtualBuiltin => "virtual-builtin",
            Self::MaterializedBuiltin => "materialized-builtin",
            Self::UserGlobal => "user-global",
            Self::WorkspaceLegacy => "workspace-legacy",
            Self::WorkspaceLocal => "workspace-local",
            Self::PluginProvided => "plugin-provided",
        }
    }

    fn precedence(self) -> i32 {
        match self {
            Self::VirtualBuiltin => 10,
            Self::MaterializedBuiltin => 20,
            Self::UserGlobal => 30,
            Self::WorkspaceLegacy => 40,
            Self::WorkspaceLocal => 50,
            Self::PluginProvided => 60,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillRegistryStatus {
    Active,
    Shadowed,
    Conflicted,
    Malformed,
}

impl SkillRegistryStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Shadowed => "shadowed",
            Self::Conflicted => "conflicted",
            Self::Malformed => "malformed",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillDescriptor {
    pub name: String,
    pub description: Option<String>,
    pub source_kind: SkillSourceKind,
    pub source_path: Option<PathBuf>,
    pub body_hash: String,
    pub requirements: Vec<String>,
    pub install_metadata: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillRegistryEntry {
    pub descriptor: SkillDescriptor,
    pub status: SkillRegistryStatus,
    pub diagnostics: Vec<String>,
    pub raw: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillRegistry {
    pub entries: Vec<SkillRegistryEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillBackedWorkflowRecipe {
    pub skill_name: String,
    pub skill_status: SkillRegistryStatus,
    pub source_kind: SkillSourceKind,
    pub source_path: Option<PathBuf>,
    pub body_hash: String,
    pub recipe: WorkflowRecipe,
    pub readiness: WorkflowRecipeReadiness,
    pub diagnostics: Vec<String>,
}

impl SkillRegistry {
    pub fn active_entries(&self) -> Vec<&SkillRegistryEntry> {
        let mut entries = self
            .entries
            .iter()
            .filter(|entry| entry.status == SkillRegistryStatus::Active)
            .collect::<Vec<_>>();
        entries.sort_by(|left, right| left.descriptor.name.cmp(&right.descriptor.name));
        entries
    }

    pub fn find(&self, name: &str) -> Option<&SkillRegistryEntry> {
        self.entries
            .iter()
            .find(|entry| {
                entry.status == SkillRegistryStatus::Active && entry.descriptor.name == name
            })
            .or_else(|| {
                self.entries
                    .iter()
                    .find(|entry| entry.descriptor.name == name)
            })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillRegistryOptions {
    pub workspace: PathBuf,
    pub user_skills_dir: Option<PathBuf>,
    pub plugin_roots: Vec<PathBuf>,
    pub plugin_roots_enabled: bool,
    pub include_virtual_builtins: bool,
}

impl SkillRegistryOptions {
    pub fn new(workspace: impl Into<PathBuf>) -> Self {
        Self {
            workspace: workspace.into(),
            user_skills_dir: None,
            plugin_roots: Vec::new(),
            plugin_roots_enabled: false,
            include_virtual_builtins: true,
        }
    }
}

pub fn builtin_skills() -> &'static [BuiltinSkill] {
    BUILTIN_SKILLS
}

pub fn builtin_skill(name: &str) -> Option<&'static BuiltinSkill> {
    BUILTIN_SKILLS.iter().find(|skill| skill.name == name)
}

pub fn deferred_builtin_skills() -> &'static [&'static str] {
    DEFERRED_BUILTIN_SKILLS
}

pub fn is_deferred_builtin_skill(name: &str) -> bool {
    DEFERRED_BUILTIN_SKILLS.contains(&name)
}

pub fn sync_builtin_skills(workspace: impl AsRef<Path>) -> io::Result<BuiltinSkillsSyncOutcome> {
    let workspace = workspace.as_ref();
    let root = workspace.join(BUILTIN_SKILLS_DIR);
    let mut outcome = BuiltinSkillsSyncOutcome {
        created_files: Vec::new(),
        created_dirs: Vec::new(),
    };
    ensure_dir(&root, BUILTIN_SKILLS_DIR, &mut outcome)?;

    for skill in BUILTIN_SKILLS {
        let skill_dir = root.join(skill.name);
        ensure_dir(
            &skill_dir,
            &format!("{BUILTIN_SKILLS_DIR}/{}", skill.name),
            &mut outcome,
        )?;
        for file in skill.files {
            let destination = skill_dir.join(file.relative_path);
            let relative = format!("{BUILTIN_SKILLS_DIR}/{}/{}", skill.name, file.relative_path);
            if let Some(parent) = destination.parent() {
                ensure_no_symlink_descendant(&root, parent)?;
                let parent_relative = relative_parent(&relative);
                ensure_dir(parent, &parent_relative, &mut outcome)?;
            }
            if existing_regular_file_or_missing(&destination)? {
                continue;
            }
            fs::write(&destination, file.content)?;
            set_executable_if_needed(&destination, file.executable)?;
            outcome.created_files.push(relative);
        }
    }
    Ok(outcome)
}

pub fn discover_skill_registry(options: SkillRegistryOptions) -> io::Result<SkillRegistry> {
    let mut builder = SkillRegistryBuilder::default();

    if options.include_virtual_builtins {
        for skill in BUILTIN_SKILLS {
            builder.push(virtual_builtin_entry(skill));
        }
    }

    discover_root(
        &options.workspace.join(BUILTIN_SKILLS_DIR),
        SkillSourceKind::MaterializedBuiltin,
        &mut builder,
    )?;
    if let Some(user_skills_dir) = options.user_skills_dir.as_ref() {
        discover_root(user_skills_dir, SkillSourceKind::UserGlobal, &mut builder)?;
    }
    discover_root(
        &options.workspace.join(".nanobot").join("skills"),
        SkillSourceKind::WorkspaceLegacy,
        &mut builder,
    )?;
    discover_root(
        &options.workspace.join(".shacs-bot").join("skills"),
        SkillSourceKind::WorkspaceLocal,
        &mut builder,
    )?;
    discover_root(
        &options.workspace.join("skills"),
        SkillSourceKind::WorkspaceLocal,
        &mut builder,
    )?;
    if options.plugin_roots_enabled {
        for root in options.plugin_roots {
            discover_root(&root, SkillSourceKind::PluginProvided, &mut builder)?;
        }
    }

    Ok(builder.finish())
}

pub fn discover_workflow_recipes(registry: &SkillRegistry) -> Vec<SkillBackedWorkflowRecipe> {
    let mut recipes = registry
        .entries
        .iter()
        .filter_map(workflow_recipe_from_entry)
        .collect::<Vec<_>>();
    mark_duplicate_recipe_ids(&mut recipes);
    recipes.sort_by(|left, right| {
        left.recipe
            .recipe_id
            .cmp(&right.recipe.recipe_id)
            .then_with(|| left.skill_name.cmp(&right.skill_name))
    });
    recipes
}

fn workflow_recipe_from_entry(entry: &SkillRegistryEntry) -> Option<SkillBackedWorkflowRecipe> {
    let raw = entry.raw.as_deref()?;
    let frontmatter = parse_skill_frontmatter(raw).ok()?;
    let mut diagnostics = entry.diagnostics.clone();
    let recipe = workflow_recipe_from_frontmatter(&frontmatter, entry, &mut diagnostics)?;
    let readiness = readiness_for_entry(entry, &recipe, &diagnostics);
    Some(SkillBackedWorkflowRecipe {
        skill_name: entry.descriptor.name.clone(),
        skill_status: entry.status,
        source_kind: entry.descriptor.source_kind,
        source_path: entry.descriptor.source_path.clone(),
        body_hash: entry.descriptor.body_hash.clone(),
        recipe,
        readiness,
        diagnostics,
    })
}

fn workflow_recipe_from_frontmatter(
    frontmatter: &BTreeMap<String, String>,
    entry: &SkillRegistryEntry,
    diagnostics: &mut Vec<String>,
) -> Option<WorkflowRecipe> {
    if let Some(recipe) = workflow_recipe_from_json_metadata(frontmatter, entry, diagnostics) {
        return Some(recipe);
    }
    let id = frontmatter
        .get("workflow.recipe.id")
        .or_else(|| frontmatter.get("metadata.shacs.workflow.recipe.id"))?;
    let pattern = parse_workflow_pattern(
        frontmatter
            .get("workflow.recipe.pattern")
            .or_else(|| frontmatter.get("metadata.shacs.workflow.recipe.pattern"))
            .map(String::as_str),
        diagnostics,
    );
    Some(WorkflowRecipe {
        recipe_id: id.trim().to_owned(),
        source_ref: recipe_source_ref(entry),
        pattern,
        prompt_template_ref: frontmatter
            .get("workflow.recipe.prompt_template_ref")
            .or_else(|| frontmatter.get("workflow.recipe.prompt_template"))
            .or_else(|| frontmatter.get("metadata.shacs.workflow.recipe.prompt_template_ref"))
            .map(|value| value.trim().to_owned())
            .unwrap_or_default(),
        rubric_ref: optional_frontmatter(frontmatter, "workflow.recipe.rubric_ref"),
        output_schema_ref: optional_frontmatter(frontmatter, "workflow.recipe.output_schema_ref"),
        suggested_budget_tokens: frontmatter
            .get("workflow.recipe.suggested_budget_tokens")
            .and_then(|value| parse_u64(value, diagnostics)),
        suggested_tool_scope_ref: optional_frontmatter(
            frontmatter,
            "workflow.recipe.suggested_tool_scope_ref",
        ),
        safety_notes: frontmatter
            .get("workflow.recipe.safety_notes")
            .map(|value| split_list(value))
            .unwrap_or_default(),
    })
}

fn workflow_recipe_from_json_metadata(
    frontmatter: &BTreeMap<String, String>,
    entry: &SkillRegistryEntry,
    diagnostics: &mut Vec<String>,
) -> Option<WorkflowRecipe> {
    let metadata = frontmatter.get("metadata")?;
    let value = match serde_json::from_str::<serde_json::Value>(metadata) {
        Ok(value) => value,
        Err(_) => return None,
    };
    let recipe = value
        .get("shacs")
        .and_then(|shacs| shacs.get("workflow_recipe"))
        .or_else(|| value.get("workflow_recipe"))?;
    let id = recipe.get("id").and_then(serde_json::Value::as_str)?;
    let pattern = parse_workflow_pattern(
        recipe.get("pattern").and_then(serde_json::Value::as_str),
        diagnostics,
    );
    Some(WorkflowRecipe {
        recipe_id: id.trim().to_owned(),
        source_ref: recipe_source_ref(entry),
        pattern,
        prompt_template_ref: recipe
            .get("prompt_template_ref")
            .or_else(|| recipe.get("prompt_template"))
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .trim()
            .to_owned(),
        rubric_ref: json_string(recipe, "rubric_ref"),
        output_schema_ref: json_string(recipe, "output_schema_ref"),
        suggested_budget_tokens: recipe
            .get("suggested_budget_tokens")
            .and_then(serde_json::Value::as_u64),
        suggested_tool_scope_ref: json_string(recipe, "suggested_tool_scope_ref"),
        safety_notes: recipe
            .get("safety_notes")
            .and_then(serde_json::Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(serde_json::Value::as_str)
                    .map(str::trim)
                    .filter(|item| !item.is_empty())
                    .map(ToOwned::to_owned)
                    .collect()
            })
            .unwrap_or_default(),
    })
}

fn readiness_for_entry(
    entry: &SkillRegistryEntry,
    recipe: &WorkflowRecipe,
    diagnostics: &[String],
) -> WorkflowRecipeReadiness {
    let mut reasons = match workflow_recipe_readiness(recipe) {
        WorkflowRecipeReadiness::Ready => Vec::new(),
        WorkflowRecipeReadiness::Malformed { reasons } => reasons,
    };
    if entry.status != SkillRegistryStatus::Active {
        reasons.push(format!("source skill is {}", entry.status.label()));
    }
    reasons.extend(
        diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.starts_with("workflow recipe "))
            .cloned(),
    );
    if reasons.is_empty() {
        WorkflowRecipeReadiness::Ready
    } else {
        WorkflowRecipeReadiness::Malformed { reasons }
    }
}

fn mark_duplicate_recipe_ids(recipes: &mut [SkillBackedWorkflowRecipe]) {
    let mut counts = BTreeMap::<String, usize>::new();
    for recipe in recipes.iter() {
        *counts.entry(recipe.recipe.recipe_id.clone()).or_default() += 1;
    }
    for recipe in recipes.iter_mut() {
        if counts
            .get(&recipe.recipe.recipe_id)
            .copied()
            .unwrap_or_default()
            <= 1
        {
            continue;
        }
        let reason = format!("duplicate workflow recipe `{}`", recipe.recipe.recipe_id);
        recipe.diagnostics.push(reason.clone());
        let mut reasons = match &recipe.readiness {
            WorkflowRecipeReadiness::Ready => Vec::new(),
            WorkflowRecipeReadiness::Malformed { reasons } => reasons.clone(),
        };
        reasons.push(reason);
        recipe.readiness = WorkflowRecipeReadiness::Malformed { reasons };
    }
}

fn recipe_source_ref(entry: &SkillRegistryEntry) -> String {
    format!(
        "skill://{}/{}#{}",
        entry.descriptor.source_kind.label(),
        entry.descriptor.name,
        entry.descriptor.body_hash
    )
}

fn optional_frontmatter(frontmatter: &BTreeMap<String, String>, key: &str) -> Option<String> {
    frontmatter
        .get(key)
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn json_string(value: &serde_json::Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn parse_u64(value: &str, diagnostics: &mut Vec<String>) -> Option<u64> {
    match value.trim().parse::<u64>() {
        Ok(value) => Some(value),
        Err(_) => {
            diagnostics.push(format!(
                "workflow recipe suggested budget is not an integer: {}",
                value.trim()
            ));
            None
        }
    }
}

fn parse_workflow_pattern(value: Option<&str>, diagnostics: &mut Vec<String>) -> WorkflowPattern {
    match value.unwrap_or_default().trim() {
        "classify_and_act" => WorkflowPattern::ClassifyAndAct,
        "fan_out_and_synthesize" => WorkflowPattern::FanOutAndSynthesize,
        "adversarial_verification" => WorkflowPattern::AdversarialVerification,
        "generate_and_filter" => WorkflowPattern::GenerateAndFilter,
        "tournament" => WorkflowPattern::Tournament,
        "loop_until_done" => WorkflowPattern::LoopUntilDone,
        "workflow_sequence" => WorkflowPattern::WorkflowSequence,
        "hybrid" => WorkflowPattern::Hybrid,
        "" => {
            diagnostics.push("workflow recipe pattern missing; using hybrid".to_owned());
            WorkflowPattern::Hybrid
        }
        other => {
            diagnostics.push(format!(
                "workflow recipe pattern `{other}` is unsupported; using hybrid"
            ));
            WorkflowPattern::Hybrid
        }
    }
}

fn split_list(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

#[derive(Default)]
struct SkillRegistryBuilder {
    entries: Vec<SkillRegistryEntry>,
    active_by_name: BTreeMap<String, (i32, usize)>,
    conflict_rank_by_name: BTreeMap<String, i32>,
}

impl SkillRegistryBuilder {
    fn push(&mut self, mut entry: SkillRegistryEntry) {
        if entry.status == SkillRegistryStatus::Malformed {
            self.entries.push(entry);
            return;
        }

        let name = entry.descriptor.name.clone();
        let rank = entry.descriptor.source_kind.precedence();

        if self.conflict_rank_by_name.get(&name).copied() == Some(rank) {
            entry.status = SkillRegistryStatus::Conflicted;
            entry
                .diagnostics
                .push(format!("duplicate skill `{name}` at same precedence"));
            self.entries.push(entry);
            return;
        }

        if let Some((existing_rank, existing_index)) = self.active_by_name.get(&name).copied() {
            if rank > existing_rank {
                if let Some(existing) = self.entries.get_mut(existing_index) {
                    existing.status = SkillRegistryStatus::Shadowed;
                    existing.diagnostics.push(format!(
                        "shadowed by `{}` from {}",
                        name,
                        entry.descriptor.source_kind.label()
                    ));
                }
                let new_index = self.entries.len();
                self.active_by_name.insert(name, (rank, new_index));
                self.entries.push(entry);
            } else if rank == existing_rank {
                if let Some(existing) = self.entries.get_mut(existing_index) {
                    existing.status = SkillRegistryStatus::Conflicted;
                    existing
                        .diagnostics
                        .push(format!("duplicate skill `{name}` at same precedence"));
                }
                entry.status = SkillRegistryStatus::Conflicted;
                entry
                    .diagnostics
                    .push(format!("duplicate skill `{name}` at same precedence"));
                self.active_by_name.remove(&name);
                self.conflict_rank_by_name.insert(name, rank);
                self.entries.push(entry);
            } else {
                entry.status = SkillRegistryStatus::Shadowed;
                entry.diagnostics.push(format!(
                    "shadowed by `{}` from higher-precedence source",
                    entry.descriptor.name
                ));
                self.entries.push(entry);
            }
        } else {
            let new_index = self.entries.len();
            self.active_by_name.insert(name, (rank, new_index));
            self.entries.push(entry);
        }
    }

    fn finish(mut self) -> SkillRegistry {
        self.entries.sort_by(|left, right| {
            left.descriptor
                .name
                .cmp(&right.descriptor.name)
                .then_with(|| {
                    left.descriptor
                        .source_kind
                        .cmp(&right.descriptor.source_kind)
                })
                .then_with(|| left.status.label().cmp(right.status.label()))
        });
        SkillRegistry {
            entries: self.entries,
        }
    }
}

fn virtual_builtin_entry(skill: &BuiltinSkill) -> SkillRegistryEntry {
    let raw = skill
        .files
        .iter()
        .find(|file| file.relative_path == "SKILL.md")
        .map(|file| String::from_utf8_lossy(file.content).to_string())
        .unwrap_or_default();
    entry_from_raw(
        raw,
        skill.name.to_owned(),
        SkillSourceKind::VirtualBuiltin,
        None,
    )
}

fn discover_root(
    root: &Path,
    source_kind: SkillSourceKind,
    builder: &mut SkillRegistryBuilder,
) -> io::Result<()> {
    match fs::symlink_metadata(root) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            builder.push(malformed_entry(
                root.file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("skills")
                    .to_owned(),
                source_kind,
                Some(root.to_path_buf()),
                "skill root must not be a symlink".to_owned(),
            ));
            return Ok(());
        }
        Ok(metadata) if !metadata.is_dir() => return Ok(()),
        Ok(_) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    }
    let Ok(entries) = fs::read_dir(root) else {
        return Ok(());
    };
    let mut paths = entries.collect::<Result<Vec<_>, io::Error>>()?;
    paths.sort_by_key(|entry| entry.file_name());

    for entry in paths {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        let metadata = fs::symlink_metadata(&path)?;
        if metadata.file_type().is_symlink() {
            builder.push(malformed_entry(
                name,
                source_kind,
                Some(path),
                "skill directories must not be symlinks".to_owned(),
            ));
            continue;
        }
        if !metadata.is_dir() {
            continue;
        }
        if source_kind == SkillSourceKind::MaterializedBuiltin && is_deferred_builtin_skill(&name) {
            continue;
        }
        let skill_path = path.join("SKILL.md");
        match fs::read_to_string(&skill_path) {
            Ok(raw) => builder.push(entry_from_raw(raw, name, source_kind, Some(skill_path))),
            Err(error) if error.kind() == io::ErrorKind::NotFound => builder.push(malformed_entry(
                name,
                source_kind,
                Some(skill_path),
                "missing SKILL.md".to_owned(),
            )),
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

fn entry_from_raw(
    raw: String,
    fallback_name: String,
    source_kind: SkillSourceKind,
    source_path: Option<PathBuf>,
) -> SkillRegistryEntry {
    match parse_skill_frontmatter(&raw) {
        Ok(frontmatter) => {
            let mut diagnostics = Vec::new();
            let name = frontmatter
                .get("name")
                .filter(|value| !value.trim().is_empty())
                .cloned()
                .unwrap_or_else(|| {
                    diagnostics.push("frontmatter name missing; using directory name".to_owned());
                    fallback_name
                });
            SkillRegistryEntry {
                descriptor: SkillDescriptor {
                    name,
                    description: frontmatter.get("description").cloned(),
                    source_kind,
                    source_path,
                    body_hash: stable_hash_hex(&raw),
                    requirements: extract_requirements(&frontmatter),
                    install_metadata: extract_install_metadata(&frontmatter),
                },
                status: SkillRegistryStatus::Active,
                diagnostics,
                raw: Some(raw),
            }
        }
        Err(error) => malformed_entry(fallback_name, source_kind, source_path, error),
    }
}

fn malformed_entry(
    name: String,
    source_kind: SkillSourceKind,
    source_path: Option<PathBuf>,
    diagnostic: String,
) -> SkillRegistryEntry {
    SkillRegistryEntry {
        descriptor: SkillDescriptor {
            name,
            description: None,
            source_kind,
            source_path,
            body_hash: String::new(),
            requirements: Vec::new(),
            install_metadata: None,
        },
        status: SkillRegistryStatus::Malformed,
        diagnostics: vec![diagnostic],
        raw: None,
    }
}

fn parse_skill_frontmatter(raw: &str) -> Result<BTreeMap<String, String>, String> {
    let Some(after_open) = raw.strip_prefix("---") else {
        return Ok(BTreeMap::new());
    };
    let after_open = after_open.trim_start_matches(['\r', '\n']);
    let Some(close_index) = after_open.find("\n---") else {
        return Err("unterminated skill frontmatter".to_owned());
    };
    let frontmatter = &after_open[..close_index];
    let mut fields = BTreeMap::new();
    for line in frontmatter.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((key, value)) = line.split_once(':') {
            fields.insert(key.trim().to_owned(), trim_frontmatter_value(value));
        }
    }
    Ok(fields)
}

fn trim_frontmatter_value(value: &str) -> String {
    value.trim().trim_matches('"').trim_matches('\'').to_owned()
}

fn extract_requirements(frontmatter: &BTreeMap<String, String>) -> Vec<String> {
    let mut requirements = Vec::new();
    for key in [
        "requires.bins",
        "requires.env",
        "metadata.nanobot.requires.bins",
        "metadata.nanobot.requires.env",
        "metadata.openclaw.requires.bins",
        "metadata.openclaw.requires.env",
    ] {
        if let Some(value) = frontmatter.get(key).filter(|value| !value.is_empty()) {
            requirements.push(format!("{key}: {value}"));
        }
    }
    if let Some(metadata) = frontmatter.get("metadata") {
        requirements.extend(extract_metadata_requirements(metadata));
    }
    requirements.sort();
    requirements.dedup();
    requirements
}

fn extract_install_metadata(frontmatter: &BTreeMap<String, String>) -> Option<String> {
    if let Some(metadata) = frontmatter.get("metadata") {
        if let Some(install) = extract_metadata_install(metadata) {
            return Some(install);
        }
    }
    frontmatter
        .iter()
        .find(|(key, value)| key.contains("install") || value.contains("install"))
        .map(|(key, value)| format!("{key}: {value}"))
}

fn extract_metadata_requirements(metadata: &str) -> Vec<String> {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(metadata) else {
        return Vec::new();
    };
    let mut requirements = Vec::new();
    for namespace in ["nanobot", "openclaw"] {
        let Some(requires) = value
            .get(namespace)
            .and_then(|section| section.get("requires"))
        else {
            continue;
        };
        collect_json_requirement(
            requires.get("bins"),
            &format!("metadata.{namespace}.requires.bins"),
            &mut requirements,
        );
        collect_json_requirement(
            requires.get("env"),
            &format!("metadata.{namespace}.requires.env"),
            &mut requirements,
        );
    }
    requirements
}

fn collect_json_requirement(
    value: Option<&serde_json::Value>,
    label: &str,
    requirements: &mut Vec<String>,
) {
    let Some(value) = value else {
        return;
    };
    match value {
        serde_json::Value::String(item) if !item.trim().is_empty() => {
            requirements.push(format!("{label}: {}", item.trim()));
        }
        serde_json::Value::Array(items) => {
            let values = items
                .iter()
                .filter_map(serde_json::Value::as_str)
                .map(str::trim)
                .filter(|item| !item.is_empty())
                .collect::<Vec<_>>();
            if !values.is_empty() {
                requirements.push(format!("{label}: {}", values.join(", ")));
            }
        }
        _ => {}
    }
}

fn extract_metadata_install(metadata: &str) -> Option<String> {
    let value = serde_json::from_str::<serde_json::Value>(metadata).ok()?;
    for namespace in ["nanobot", "openclaw"] {
        if let Some(install) = value
            .get(namespace)
            .and_then(|section| section.get("install"))
        {
            return Some(format!("metadata.{namespace}.install: {install}"));
        }
    }
    None
}

fn stable_hash_hex(content: &str) -> String {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for byte in content.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}")
}

fn ensure_dir(
    path: &Path,
    relative: &str,
    outcome: &mut BuiltinSkillsSyncOutcome,
) -> io::Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() {
                return Err(symlink_error());
            }
            if !metadata.is_dir() {
                return Err(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    format!("path exists and is not a directory: {}", path.display()),
                ));
            }
            Ok(())
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            fs::create_dir_all(path)?;
            outcome.created_dirs.push(relative.to_owned());
            Ok(())
        }
        Err(error) => Err(error),
    }
}

fn ensure_no_symlink_descendant(root: &Path, path: &Path) -> io::Result<()> {
    let Ok(relative) = path.strip_prefix(root) else {
        return Ok(());
    };
    let mut current = root.to_path_buf();
    for component in relative.components() {
        current.push(component.as_os_str());
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => return Err(symlink_error()),
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => break,
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

fn existing_regular_file_or_missing(path: &Path) -> io::Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() {
                return Err(symlink_error());
            }
            if !metadata.is_file() {
                return Err(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    format!("path exists and is not a file: {}", path.display()),
                ));
            }
            Ok(true)
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

fn symlink_error() -> io::Error {
    io::Error::new(
        io::ErrorKind::PermissionDenied,
        "symlink paths are not allowed for bundled skill sync",
    )
}

fn relative_parent(relative: &str) -> String {
    PathBuf::from(relative)
        .parent()
        .map(|path| path.to_string_lossy().to_string())
        .unwrap_or_else(|| BUILTIN_SKILLS_DIR.to_owned())
}

#[cfg(unix)]
fn set_executable_if_needed(path: &Path, executable: bool) -> io::Result<()> {
    if !executable {
        return Ok(());
    }
    use std::os::unix::fs::PermissionsExt;
    let mut permissions = fs::metadata(path)?.permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions)
}

#[cfg(not(unix))]
fn set_executable_if_needed(_path: &Path, _executable: bool) -> io::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn bundled_catalog_contains_shacs_and_imported_skill_set(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let names = builtin_skills()
            .iter()
            .map(|skill| skill.name)
            .collect::<BTreeSet<_>>();
        for name in [
            "cron",
            "weather",
            "tmux",
            "my",
            "github",
            "skill-creator",
            "clawhub",
            "summarize",
            "memory",
            "test-driven-development",
            "github-pr-workflow",
            "google-workspace",
            "serving-llms-vllm",
        ] {
            assert!(names.contains(name), "missing bundled skill {name}");
        }
        assert!(names.len() >= 77, "bundled skill catalog shrank: {names:?}");

        let Some(skill_creator) = builtin_skill("skill-creator") else {
            return Err("skill-creator is not bundled".into());
        };
        assert!(skill_creator
            .files
            .iter()
            .any(|file| file.relative_path == "scripts/package_skill.py" && file.executable));

        let imported = builtin_skill("test-driven-development")
            .ok_or("test-driven-development is not bundled")?;
        let skill_file = imported
            .files
            .iter()
            .find(|file| file.relative_path == "SKILL.md")
            .ok_or("imported skill missing SKILL.md")?;
        let body = std::str::from_utf8(skill_file.content)?;
        assert!(body.contains("metadata.shacs.imported_from"));
        assert!(body.contains("shacs-bot adaptation"));
        Ok(())
    }

    #[test]
    fn bundled_catalog_files_are_unique_safe_and_valid() -> Result<(), Box<dyn std::error::Error>> {
        let mut names = BTreeSet::new();
        for skill in builtin_skills() {
            assert!(
                names.insert(skill.name),
                "duplicate bundled skill {}",
                skill.name
            );
            assert!(
                skill
                    .files
                    .iter()
                    .any(|file| file.relative_path == "SKILL.md"),
                "{} is missing SKILL.md",
                skill.name
            );
            let mut relative_paths = BTreeSet::new();
            for file in skill.files {
                let relative = std::path::Path::new(file.relative_path);
                assert!(
                    !relative.is_absolute(),
                    "{} has absolute bundled path {}",
                    skill.name,
                    file.relative_path
                );
                assert!(
                    !relative
                        .components()
                        .any(|component| matches!(component, std::path::Component::ParentDir)),
                    "{} has unsafe bundled path {}",
                    skill.name,
                    file.relative_path
                );
                assert!(
                    relative_paths.insert(file.relative_path),
                    "{} has duplicate bundled path {}",
                    skill.name,
                    file.relative_path
                );
                if file.relative_path == "SKILL.md" {
                    std::str::from_utf8(file.content)?;
                }
            }
        }
        Ok(())
    }

    #[test]
    fn sync_builtin_skills_writes_files_without_overwriting(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let workspace = tempfile::tempdir()?;
        let custom_weather = workspace
            .path()
            .join(BUILTIN_SKILLS_DIR)
            .join("weather")
            .join("SKILL.md");
        let Some(parent) = custom_weather.parent() else {
            return Err("custom weather path has no parent".into());
        };
        fs::create_dir_all(parent)?;
        fs::write(&custom_weather, "custom weather")?;

        let outcome = sync_builtin_skills(workspace.path())?;
        assert!(workspace
            .path()
            .join("builtin_skills/cron/SKILL.md")
            .exists());
        assert!(workspace
            .path()
            .join("builtin_skills/tmux/scripts/find-sessions.sh")
            .exists());
        assert_eq!(fs::read_to_string(custom_weather)?, "custom weather");
        assert!(outcome
            .created_files
            .iter()
            .any(|path| path == "builtin_skills/cron/SKILL.md"));

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(
                workspace
                    .path()
                    .join("builtin_skills/tmux/scripts/find-sessions.sh"),
            )?
            .permissions()
            .mode();
            assert_ne!(mode & 0o111, 0);
        }

        Ok(())
    }

    #[test]
    fn registry_exposes_virtual_builtins_without_onboard() -> Result<(), Box<dyn std::error::Error>>
    {
        let workspace = tempfile::tempdir()?;
        let registry = discover_skill_registry(SkillRegistryOptions::new(workspace.path()))?;

        let active_names = registry
            .active_entries()
            .into_iter()
            .map(|entry| entry.descriptor.name.clone())
            .collect::<Vec<_>>();
        assert!(active_names.contains(&"skill-creator".to_owned()));
        assert!(active_names.contains(&"test-driven-development".to_owned()));
        assert!(active_names.contains(&"github-pr-workflow".to_owned()));
        assert!(active_names.contains(&"google-workspace".to_owned()));
        let tdd = registry
            .find("test-driven-development")
            .ok_or("missing imported tdd skill")?;
        assert_eq!(tdd.descriptor.source_kind, SkillSourceKind::VirtualBuiltin);
        assert!(tdd
            .raw
            .as_deref()
            .is_some_and(|raw| raw.contains("shacs-bot adaptation")));
        let clawhub = registry.find("clawhub").ok_or("missing clawhub")?;
        assert_eq!(
            clawhub.descriptor.source_kind,
            SkillSourceKind::VirtualBuiltin
        );
        assert_eq!(clawhub.status, SkillRegistryStatus::Active);
        assert!(clawhub
            .descriptor
            .description
            .as_deref()
            .is_some_and(|description| description.contains("ClawHub")));
        let github = registry.find("github").ok_or("missing github")?;
        assert!(github
            .descriptor
            .requirements
            .iter()
            .any(|requirement| requirement.contains("gh")));
        Ok(())
    }

    #[test]
    fn registry_workspace_skill_shadows_virtual_builtin() -> Result<(), Box<dyn std::error::Error>>
    {
        let workspace = tempfile::tempdir()?;
        let skill_dir = workspace.path().join("skills/weather");
        fs::create_dir_all(&skill_dir)?;
        fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: weather\ndescription: Custom weather\n---\ncustom",
        )?;

        let registry = discover_skill_registry(SkillRegistryOptions::new(workspace.path()))?;
        let weather_entries = registry
            .entries
            .iter()
            .filter(|entry| entry.descriptor.name == "weather")
            .collect::<Vec<_>>();
        assert_eq!(weather_entries.len(), 2);
        assert!(weather_entries.iter().any(|entry| {
            entry.status == SkillRegistryStatus::Active
                && entry.descriptor.source_kind == SkillSourceKind::WorkspaceLocal
        }));
        assert!(weather_entries.iter().any(|entry| {
            entry.status == SkillRegistryStatus::Shadowed
                && entry.descriptor.source_kind == SkillSourceKind::VirtualBuiltin
        }));
        Ok(())
    }

    #[test]
    fn registry_applies_precedence_across_configured_roots(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let workspace = tempfile::tempdir()?;
        let user = tempfile::tempdir()?;
        let plugin = tempfile::tempdir()?;
        let name = "precedence-probe";

        write_test_skill(&workspace.path().join(BUILTIN_SKILLS_DIR), name, "builtin")?;
        write_test_skill(&user.path().join("skills"), name, "user")?;
        write_test_skill(&workspace.path().join(".nanobot/skills"), name, "legacy")?;
        write_test_skill(&workspace.path().join("skills"), name, "workspace")?;
        write_test_skill(plugin.path(), name, "plugin")?;

        let mut options = SkillRegistryOptions::new(workspace.path());
        options.user_skills_dir = Some(user.path().join("skills"));
        options.plugin_roots = vec![plugin.path().to_path_buf()];
        options.plugin_roots_enabled = true;
        let registry = discover_skill_registry(options)?;
        let entries = registry
            .entries
            .iter()
            .filter(|entry| entry.descriptor.name == name)
            .collect::<Vec<_>>();

        assert_eq!(entries.len(), 5);
        assert!(entries.iter().any(|entry| {
            entry.status == SkillRegistryStatus::Active
                && entry.descriptor.source_kind == SkillSourceKind::PluginProvided
        }));
        for source_kind in [
            SkillSourceKind::MaterializedBuiltin,
            SkillSourceKind::UserGlobal,
            SkillSourceKind::WorkspaceLegacy,
            SkillSourceKind::WorkspaceLocal,
        ] {
            assert!(entries.iter().any(|entry| {
                entry.status == SkillRegistryStatus::Shadowed
                    && entry.descriptor.source_kind == source_kind
            }));
        }
        Ok(())
    }

    #[test]
    fn registry_conflicts_duplicate_plugin_roots() -> Result<(), Box<dyn std::error::Error>> {
        let workspace = tempfile::tempdir()?;
        let plugin_a = tempfile::tempdir()?;
        let plugin_b = tempfile::tempdir()?;
        let name = "review";

        write_test_skill(plugin_a.path(), name, "plugin a")?;
        write_test_skill(plugin_b.path(), name, "plugin b")?;

        let mut options = SkillRegistryOptions::new(workspace.path());
        options.plugin_roots = vec![plugin_a.path().to_path_buf(), plugin_b.path().to_path_buf()];
        options.plugin_roots_enabled = true;
        let registry = discover_skill_registry(options)?;
        let entries = registry
            .entries
            .iter()
            .filter(|entry| entry.descriptor.name == name)
            .collect::<Vec<_>>();

        assert_eq!(entries.len(), 2);
        assert!(entries.iter().all(|entry| {
            entry.status == SkillRegistryStatus::Conflicted
                && entry.descriptor.source_kind == SkillSourceKind::PluginProvided
        }));
        assert!(!registry
            .active_entries()
            .into_iter()
            .any(|entry| entry.descriptor.name == name));
        Ok(())
    }

    #[test]
    fn registry_ignores_plugin_roots_by_default() -> Result<(), Box<dyn std::error::Error>> {
        let workspace = tempfile::tempdir()?;
        let plugin = tempfile::tempdir()?;
        write_test_skill(plugin.path(), "plugin-probe", "plugin")?;

        let mut options = SkillRegistryOptions::new(workspace.path());
        options.plugin_roots = vec![plugin.path().to_path_buf()];
        let registry = discover_skill_registry(options)?;

        assert!(registry.find("plugin-probe").is_none());
        Ok(())
    }

    #[test]
    fn bundled_catalog_matches_skill_directories() -> Result<(), Box<dyn std::error::Error>> {
        let crate_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let directory_names = fs::read_dir(crate_dir)?
            .filter_map(Result::ok)
            .filter_map(|entry| {
                let path = entry.path();
                let name = entry.file_name().to_string_lossy().to_string();
                (path.join("SKILL.md").is_file() && !is_deferred_builtin_skill(&name))
                    .then_some(name)
            })
            .collect::<BTreeSet<_>>();
        let catalog_names = builtin_skills()
            .iter()
            .map(|skill| skill.name.to_owned())
            .collect::<BTreeSet<_>>();
        assert_eq!(directory_names, catalog_names);
        Ok(())
    }

    #[test]
    fn deferred_builtins_are_not_bundled_or_materialized() -> Result<(), Box<dyn std::error::Error>>
    {
        assert!(deferred_builtin_skills().contains(&"hermes-agent"));
        for name in deferred_builtin_skills() {
            assert!(
                builtin_skill(name).is_none(),
                "deferred skill {name} is bundled"
            );
        }

        let workspace = tempfile::tempdir()?;
        sync_builtin_skills(workspace.path())?;
        for name in deferred_builtin_skills() {
            assert!(
                !workspace
                    .path()
                    .join(BUILTIN_SKILLS_DIR)
                    .join(name)
                    .exists(),
                "deferred skill {name} was materialized"
            );
        }
        Ok(())
    }

    #[test]
    fn registry_ignores_deferred_materialized_builtins_but_allows_workspace_override(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let workspace = tempfile::tempdir()?;
        write_test_skill(
            &workspace.path().join(BUILTIN_SKILLS_DIR),
            "hermes-agent",
            "stale materialized",
        )?;

        let registry = discover_skill_registry(SkillRegistryOptions::new(workspace.path()))?;
        assert!(registry.find("hermes-agent").is_none());

        write_test_skill(
            &workspace.path().join("skills"),
            "hermes-agent",
            "user override",
        )?;
        let registry = discover_skill_registry(SkillRegistryOptions::new(workspace.path()))?;
        let entry = registry
            .find("hermes-agent")
            .ok_or("workspace override should remain visible")?;
        assert_eq!(entry.status, SkillRegistryStatus::Active);
        assert_eq!(
            entry.descriptor.source_kind,
            SkillSourceKind::WorkspaceLocal
        );
        Ok(())
    }

    #[test]
    fn sync_builtin_skills_materializes_every_catalog_file(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let workspace = tempfile::tempdir()?;
        sync_builtin_skills(workspace.path())?;

        for skill in builtin_skills() {
            for file in skill.files {
                let path = workspace
                    .path()
                    .join(BUILTIN_SKILLS_DIR)
                    .join(skill.name)
                    .join(file.relative_path);
                assert!(path.exists(), "missing synced file {}", path.display());
                assert_eq!(fs::read(&path)?, file.content);
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    let mode = fs::metadata(&path)?.permissions().mode();
                    assert_eq!(
                        mode & 0o111 != 0,
                        file.executable,
                        "executable bit drifted for {}",
                        path.display()
                    );
                }
            }
        }
        Ok(())
    }

    #[test]
    fn registry_exposes_every_virtual_builtin_without_onboard(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let workspace = tempfile::tempdir()?;
        let registry = discover_skill_registry(SkillRegistryOptions::new(workspace.path()))?;

        for skill in builtin_skills() {
            let entry = registry
                .find(skill.name)
                .ok_or_else(|| format!("missing virtual builtin {}", skill.name))?;
            assert_eq!(entry.status, SkillRegistryStatus::Active);
            assert_eq!(
                entry.descriptor.source_kind,
                SkillSourceKind::VirtualBuiltin
            );
            assert!(!entry.descriptor.body_hash.is_empty());
            assert!(entry.raw.as_deref().is_some_and(|raw| !raw.is_empty()));
        }
        Ok(())
    }

    fn write_test_skill(root: &std::path::Path, name: &str, description: &str) -> io::Result<()> {
        let skill_dir = root.join(name);
        fs::create_dir_all(&skill_dir)?;
        fs::write(
            skill_dir.join("SKILL.md"),
            format!("---\nname: {name}\ndescription: {description}\n---\n{description}"),
        )
    }

    #[test]
    fn registry_reports_malformed_and_conflicted_skills() -> Result<(), Box<dyn std::error::Error>>
    {
        let workspace = tempfile::tempdir()?;
        fs::create_dir_all(workspace.path().join("skills/broken"))?;
        fs::create_dir_all(workspace.path().join("skills/dup"))?;
        fs::create_dir_all(workspace.path().join(".shacs-bot/skills/dup"))?;
        fs::write(
            workspace.path().join("skills/dup/SKILL.md"),
            "---\nname: dup\ndescription: one\n---\none",
        )?;
        fs::write(
            workspace.path().join(".shacs-bot/skills/dup/SKILL.md"),
            "---\nname: dup\ndescription: two\n---\ntwo",
        )?;

        let registry = discover_skill_registry(SkillRegistryOptions::new(workspace.path()))?;
        let broken = registry.find("broken").ok_or("missing broken")?;
        assert_eq!(broken.status, SkillRegistryStatus::Malformed);
        assert!(broken
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.contains("missing SKILL.md")));

        let dup_entries = registry
            .entries
            .iter()
            .filter(|entry| entry.descriptor.name == "dup")
            .collect::<Vec<_>>();
        assert_eq!(dup_entries.len(), 2);
        assert!(dup_entries
            .iter()
            .all(|entry| entry.status == SkillRegistryStatus::Conflicted));
        assert!(dup_entries.iter().all(|entry| entry
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.contains("duplicate skill `dup`"))));
        Ok(())
    }

    #[test]
    fn workflow_recipe_discovery_projects_read_only_skill_metadata(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let workspace = tempfile::tempdir()?;
        let skill_dir = workspace.path().join("skills/reviewer");
        fs::create_dir_all(&skill_dir)?;
        fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: reviewer\ndescription: Review workflow\nworkflow.recipe.id: review-fanout\nworkflow.recipe.pattern: fan_out_and_synthesize\nworkflow.recipe.prompt_template_ref: prompts/review.md\nworkflow.recipe.suggested_budget_tokens: 12000\nworkflow.recipe.safety_notes: read-only, no permission grants\n---\nUse the recipe as read-only guidance.",
        )?;

        let registry = discover_skill_registry(SkillRegistryOptions::new(workspace.path()))?;
        let recipes = discover_workflow_recipes(&registry);

        assert_eq!(recipes.len(), 1);
        assert_eq!(recipes[0].recipe.recipe_id, "review-fanout");
        assert_eq!(
            recipes[0].recipe.pattern,
            WorkflowPattern::FanOutAndSynthesize
        );
        assert_eq!(recipes[0].recipe.prompt_template_ref, "prompts/review.md");
        assert_eq!(recipes[0].recipe.suggested_budget_tokens, Some(12000));
        assert_eq!(recipes[0].readiness, WorkflowRecipeReadiness::Ready);
        assert!(recipes[0]
            .recipe
            .source_ref
            .starts_with("skill://workspace-local/reviewer#"));
        Ok(())
    }

    #[test]
    fn workflow_recipe_discovery_marks_malformed_and_duplicate_recipes(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let workspace = tempfile::tempdir()?;
        for name in ["one", "two"] {
            let skill_dir = workspace.path().join("skills").join(name);
            fs::create_dir_all(&skill_dir)?;
            fs::write(
                skill_dir.join("SKILL.md"),
                format!(
                    "---\nname: {name}\nworkflow.recipe.id: duplicate\nworkflow.recipe.pattern: loop_until_done\n---\nbody"
                ),
            )?;
        }

        let registry = discover_skill_registry(SkillRegistryOptions::new(workspace.path()))?;
        let recipes = discover_workflow_recipes(&registry);

        assert_eq!(recipes.len(), 2);
        assert!(recipes
            .iter()
            .all(|recipe| matches!(recipe.readiness, WorkflowRecipeReadiness::Malformed { .. })));
        assert!(recipes.iter().all(|recipe| recipe
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.contains("duplicate workflow recipe"))));
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn registry_rejects_symlink_skill_root() -> Result<(), Box<dyn std::error::Error>> {
        use std::os::unix::fs::symlink;

        let workspace = tempfile::tempdir()?;
        let outside = tempfile::tempdir()?;
        symlink(outside.path(), workspace.path().join("skills"))?;

        let registry = discover_skill_registry(SkillRegistryOptions::new(workspace.path()))?;
        let root_entry = registry
            .entries
            .iter()
            .find(|entry| entry.descriptor.name == "skills")
            .ok_or("missing symlink root diagnostic")?;
        assert_eq!(root_entry.status, SkillRegistryStatus::Malformed);
        assert!(root_entry
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.contains("root must not be a symlink")));
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn sync_builtin_skills_rejects_intermediate_dir_symlink_escape(
    ) -> Result<(), Box<dyn std::error::Error>> {
        use std::os::unix::fs::symlink;

        let workspace = tempfile::tempdir()?;
        let outside = tempfile::tempdir()?;
        let skill_dir = workspace
            .path()
            .join("builtin_skills/baoyu-article-illustrator");
        fs::create_dir_all(&skill_dir)?;
        symlink(outside.path(), skill_dir.join("references"))?;

        let error = sync_builtin_skills(workspace.path())
            .expect_err("intermediate symlink escape is rejected");
        assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
        assert!(!outside.path().join("palettes").exists());
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn sync_builtin_skills_rejects_dangling_file_symlink() -> Result<(), Box<dyn std::error::Error>>
    {
        use std::os::unix::fs::symlink;

        let workspace = tempfile::tempdir()?;
        let outside_target = workspace.path().join("outside-target");
        let skill_dir = workspace.path().join("builtin_skills/cron");
        fs::create_dir_all(&skill_dir)?;
        symlink(&outside_target, skill_dir.join("SKILL.md"))?;

        let error =
            sync_builtin_skills(workspace.path()).expect_err("dangling symlink is rejected");
        assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
        assert!(!outside_target.exists());
        Ok(())
    }
}
