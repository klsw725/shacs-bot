use super::spec035_catalog::catalog;
use super::spec035_evidence_io::sha256;
use super::spec035_execution_probe::probe;
use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

pub(super) const ROOT: &str = ".omo/evidence/spec035/prd000-009";
pub(super) const OWNERS: [(&str, &[&str]); 6] = [
    ("029", &["durable-work-child", "recovery-delivery"]),
    (
        "030",
        &[
            "resource-disclosure",
            "runtime-controls",
            "credential-status",
        ],
    ),
    ("031", &["config-auth-declarations", "execution-snapshot"]),
    ("032", &["registry-intent", "app-lifecycle"]),
    ("033", &["automation-delivery", "goal-accounting"]),
    ("034", &["stored-video", "generated-media-analyzer"]),
];
pub(super) const GATES: [(&str, &str); 19] = [
    ("focused", "focused_test"),
    ("workspace", "workspace_test"),
    ("format", "format"),
    ("lint", "lint"),
    ("build-cli", "build"),
    ("build-tui", "build"),
    ("surface-cli", "surface"),
    ("surface-tui", "surface"),
    ("surface-repl", "surface"),
    ("surface-onboard", "surface"),
    ("surface-api", "surface"),
    ("surface-websocket", "surface"),
    ("surface-channel", "surface"),
    ("lifecycle", "surface"),
    ("failure-injection", "focused_test"),
    ("visual", "review"),
    ("correction", "review"),
    ("redaction", "review"),
    ("documentation", "review"),
];

pub(super) struct Fixture {
    pub(super) repo: tempfile::TempDir,
    pub(super) root: PathBuf,
    pub(super) manifest: Value,
}

impl Fixture {
    pub(super) fn new() -> Self {
        let repo = tempfile::tempdir().expect("owned synthetic repository");
        fs::write(repo.path().join(".gitignore"), ".omo/\n").expect("fixture ignore");
        assert!(Command::new("git")
            .args(["init", "--quiet"])
            .current_dir(repo.path())
            .status()
            .expect("git init")
            .success());
        let root = repo.path().join(ROOT);
        fs::create_dir_all(root.join("receipts")).expect("evidence root");
        fs::create_dir_all(root.join("commands")).expect("command evidence");
        fs::create_dir_all(repo.path().join("crates")).expect("source root");
        fs::write(repo.path().join("crates/probe.rs"), &probe().source).expect("producer");
        fs::write(repo.path().join("crates/Cargo.toml"), &probe().manifest)
            .expect("probe manifest copy");
        fs::write(repo.path().join("crates/Cargo.lock"), &probe().lock)
            .expect("Cargo-generated lock copy");
        let actual = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .expect("actual repo");
        for row in catalog() {
            let (path, _) = row.source_locator.rsplit_once(':').expect("locator");
            let target = repo.path().join(path);
            fs::create_dir_all(target.parent().expect("parent")).expect("authority directory");
            fs::copy(actual.join(path), target).expect("authority fixture copies");
        }
        let files = source_files(repo.path());
        write_json(&root.join("source-before.json"), &json!(files));
        fs::copy(
            root.join("source-before.json"),
            root.join("source-after.json"),
        )
        .expect("stable source snapshot");
        let source_hash = digest(&root.join("source-before.json"));
        fs::write(root.join("test.stdout"), &probe().stdout)
            .expect("actual Cargo probe transcript");
        fs::write(root.join("empty.stderr"), "").expect("stderr");
        write_json(
            &root.join("observation.json"),
            &json!({"constructed_test_data":true,"observed":true}),
        );
        let commands: Vec<_> = GATES.iter().map(|(id, kind)| {
            let (argv, package, filter, tests) = match *kind {
                "focused_test" => (json!(["cargo","test","--manifest-path","crates/Cargo.toml","--locked","-p","probe","evidence_probe"]), json!("probe"), json!("evidence_probe"), json!({"tests_run":1,"tests_failed":0})),
                "workspace_test" => (json!(["cargo","test","--manifest-path","crates/Cargo.toml","--locked","--workspace"]), Value::Null, Value::Null, json!({"tests_run":1,"tests_failed":0})),
                "format" => (json!(["cargo","fmt","--manifest-path","crates/Cargo.toml","--all","--","--check"]), Value::Null, Value::Null, Value::Null),
                "lint" => (json!(["cargo","clippy","--manifest-path","crates/Cargo.toml","--locked","--workspace","--all-targets","--","-D","warnings"]), Value::Null, Value::Null, Value::Null),
                "build" => (json!(["cargo","build","--manifest-path","crates/Cargo.toml","--locked","-p", if *id == "build-cli" { "shacs-cli" } else { "shacs-tui" }]), Value::Null, Value::Null, Value::Null),
                _ => (json!(["constructed-surface-driver",id]), Value::Null, Value::Null, Value::Null),
            };
            let stdout = format!("commands/{id}.stdout");
            let stderr = format!("commands/{id}.stderr");
            fs::copy(root.join(if tests.is_null(){"observation.json"}else{"test.stdout"}), root.join(&stdout)).expect("constructed command transcript");
            fs::copy(root.join("empty.stderr"),root.join(&stderr)).expect("command stderr");
            if *kind == "workspace_test" {
                fs::write(root.join(&stderr), &probe().stderr).expect("actual target headers");
            }
            json!({"id":id,"kind":kind,"argv":argv,"package":package,"filter":filter,"tests":tests,"exit_code":0,"run_id":"constructed-execution","source_sha256":source_hash,"stdout":file_ref(&root,&stdout),"stderr":file_ref(&root,&stderr)})
        }).collect();
        let requirements: Vec<_> = catalog()
            .into_iter()
            .map(|authority| {
                let receipt = receipt(&root, &source_hash, &authority.id, "focused");
                json!({"authority":authority,"receipt":receipt})
            })
            .collect();
        let owners: Vec<_> = OWNERS
            .iter()
            .map(|(owner, facts)| {
                let facts: Vec<_> = facts
                    .iter()
                    .map(|fact| {
                        let id = format!("spec{owner}:{fact}");
                        json!({"id":id,"receipt":receipt(&root,&source_hash,&id,"focused")})
                    })
                    .collect();
                json!({"owner":owner,"facts":facts})
            })
            .collect();
        let gates: Vec<_> = GATES
            .iter()
            .map(|(id, _)| json!({"id":id,"receipt":receipt(&root,&source_hash,id,id)}))
            .collect();
        write_json(
            &root.join("absence.json"),
            &json!({"run_id":"constructed-execution","source_sha256":source_hash,"resource_id":"owned-probe","absent":true}),
        );
        write_json(
            &root.join("cleanup.json"),
            &json!({"run_id":"constructed-execution","source_sha256":source_hash,"verdict":"PASS","resources":[{"id":"owned-probe","disposition":"removed","proof":file_ref(&root,"absence.json")}]}),
        );
        let incidents = [
            "default-config-rewrite",
            "missing-spec034-fixture",
            "ambiguous-spec034-temp-roots",
        ];
        let resolutions: Vec<_> = incidents
            .iter()
            .map(|id| json!({"id":id,"receipt":receipt(&root,&source_hash,id,"correction")}))
            .collect();
        let manifest = json!({"schema":"spec035.prd000_009_closure_execution.v1","run_id":"constructed-execution","source":{"before":file_ref(&root,"source-before.json"),"after":file_ref(&root,"source-after.json")},"inventory":null,"requirements":requirements,"owners":owners,"commands":commands,"gates":gates,"resources":["owned-probe"],"cleanup":file_ref(&root,"cleanup.json"),"incidents":resolutions});
        let mut fixture = Self {
            repo,
            root,
            manifest,
        };
        fixture.save();
        fixture
    }

    pub(super) fn save(&mut self) {
        let mut paths: Vec<_> = fs::read_dir(&self.root)
            .expect("fixture files")
            .map(|entry| entry.expect("entry").path())
            .filter(|path| {
                path.is_file()
                    && !matches!(
                        path.file_name().and_then(|name| name.to_str()),
                        Some("manifest.json" | "inventory.json")
                    )
            })
            .collect();
        for directory in ["receipts", "commands"] {
            paths.extend(
                fs::read_dir(self.root.join(directory))
                    .expect("evidence directory")
                    .map(|entry| entry.expect("entry").path()),
            );
        }
        paths.sort();
        let inventory: Vec<_> = paths
            .iter()
            .map(|path| {
                file_ref(
                    &self.root,
                    path.strip_prefix(&self.root)
                        .expect("relative")
                        .to_str()
                        .expect("UTF8"),
                )
            })
            .collect();
        write_json(&self.root.join("inventory.json"), &json!(inventory));
        self.manifest["inventory"] = file_ref(&self.root, "inventory.json");
        write_json(&self.root.join("manifest.json"), &self.manifest);
    }
}

pub(super) fn write_json(path: &Path, value: &Value) {
    fs::write(path, serde_json::to_vec_pretty(value).expect("serialize")).expect("write JSON");
}
pub(super) fn file_ref(root: &Path, path: &str) -> Value {
    json!({"path":path,"sha256":digest(&root.join(path))})
}
fn digest(path: &Path) -> String {
    sha256(&fs::read(path).expect("hash fixture"))
}
fn source_files(repo: &Path) -> Vec<Value> {
    let output = Command::new("git")
        .args([
            "ls-files",
            "--cached",
            "--others",
            "--exclude-standard",
            "-z",
        ])
        .current_dir(repo)
        .output()
        .expect("source list");
    assert!(output.status.success());
    let mut paths: Vec<_> = output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|path| !path.is_empty())
        .map(|path| std::str::from_utf8(path).expect("UTF8"))
        .collect();
    paths.sort();
    paths.dedup();
    paths.into_iter().map(|path| file_ref(repo, path)).collect()
}
fn receipt(root: &Path, source: &str, subject: &str, command: &str) -> Value {
    let path = format!("receipts/{}.json", subject.replace(':', "-"));
    write_json(
        &root.join(&path),
        &json!({"run_id":"constructed-execution","source_sha256":source,"subject":subject,"verdict":"PASS","checks":[{"id":"observed","verdict":"PASS","producer":"crates/probe.rs:1","commands":[command],"artifacts":[file_ref(root,"observation.json")]}]}),
    );
    file_ref(root, &path)
}
