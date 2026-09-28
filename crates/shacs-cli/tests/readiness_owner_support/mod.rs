use serde_json::Value;
use shacs_app::app::{AppRegistry, AppRegistryStore};
use shacs_config::{save_config_to_path, Config};
use std::error::Error;
use std::fs;
use std::net::TcpListener;
use std::os::unix::fs::MetadataExt;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

pub type TestResult<T = ()> = Result<T, Box<dyn Error>>;

pub struct Fixture {
    pub root: PathBuf,
    identity: (u64, u64),
    pub config: Config,
    pub port: u16,
    child: Option<Child>,
    export: Option<PathBuf>,
}

impl Fixture {
    pub fn new() -> TestResult<Self> {
        let root = tempfile::Builder::new()
            .prefix("r035.")
            .tempdir_in(fs::canonicalize("/tmp")?)?
            .keep();
        let metadata = fs::symlink_metadata(&root)?;
        let mut fixture = Self {
            identity: (metadata.dev(), metadata.ino()),
            root,
            config: Config::default(),
            port: TcpListener::bind("127.0.0.1:0")?.local_addr()?.port(),
            child: None,
            export: std::env::var_os("SHACS_READINESS_EVIDENCE_ROOT").map(PathBuf::from),
        };
        for directory in ["home", "tmp", "workspace/.shacs-bot/plugins", "plugins"] {
            fs::create_dir_all(fixture.root.join(directory))?;
        }
        fixture.config.agents.defaults.workspace = fixture
            .root
            .join("workspace")
            .to_string_lossy()
            .into_owned();
        fixture.config.agents.defaults.provider = "openai".to_owned();
        fixture.config.providers.clear();
        fixture.config.channels.plugins.clear();
        fixture.config.api.host = "127.0.0.1".to_owned();
        fixture.config.api.port = fixture.port;
        fixture.save_config()?;
        AppRegistryStore::new(&fixture.root).save(&AppRegistry::default())?;
        fixture.record(
            "isolation.json",
            &serde_json::to_vec_pretty(&serde_json::json!({
                "root": fixture.root, "device": fixture.identity.0, "inode": fixture.identity.1,
                "provider_credentials": false, "provider_calls": 0,
                "fixtures": "synthetic controlled descriptors; no plugin/app entrypoint is executed"
            }))?,
        )?;
        Ok(fixture)
    }

    pub fn save_config(&self) -> TestResult {
        save_config_to_path(&self.config, &self.root.join("config.json"))?;
        Ok(())
    }

    pub fn command(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_shacs-bot"));
        command
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", self.root.join("home"))
            .env("TMPDIR", self.root.join("tmp"))
            .arg("--config")
            .arg(self.root.join("config.json"));
        command
    }

    pub fn cli(&self, label: &str, args: &[&str]) -> TestResult<String> {
        let output = self.command().args(args).output()?;
        self.record(&format!("{label}.stdout"), &output.stdout)?;
        self.record(&format!("{label}.stderr"), &output.stderr)?;
        self.record(&format!("{label}.command.json"), &serde_json::to_vec_pretty(&serde_json::json!({
            "args": args, "status": output.status.code(), "config": self.root.join("config.json")
        }))?)?;
        assert!(
            output.status.success(),
            "{label}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        Ok(String::from_utf8(output.stdout)?)
    }

    pub fn start(&mut self) -> TestResult {
        let stdout = fs::File::create(self.root.join("serve.stdout"))?;
        let stderr = fs::File::create(self.root.join("serve.stderr"))?;
        self.child = Some(
            self.command()
                .args([
                    "serve",
                    "--host",
                    "127.0.0.1",
                    "--port",
                    &self.port.to_string(),
                ])
                .stdin(Stdio::null())
                .stdout(stdout)
                .stderr(stderr)
                .spawn()?,
        );
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            if self.get("/health").is_ok() {
                return Ok(());
            }
            if self
                .child
                .as_mut()
                .ok_or("missing serve")?
                .try_wait()?
                .is_some()
            {
                return Err(fs::read_to_string(self.root.join("serve.stderr"))?.into());
            }
            if Instant::now() >= deadline {
                return Err("serve health timeout".into());
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    pub fn get(&self, path: &str) -> TestResult<String> {
        Ok(ureq::get(&format!("http://127.0.0.1:{}{path}", self.port))
            .call()?
            .body_mut()
            .read_to_string()?)
    }

    pub fn capture(&self, label: &str) -> TestResult<Value> {
        let bundle = self.root.join(format!("{label}.zip"));
        let diagnostics = self.cli(
            &format!("{label}-cli"),
            &[
                "runtime",
                "diagnostics",
                "--bundle",
                bundle.to_str().ok_or("bundle path")?,
            ],
        )?;
        let cli: Value = serde_json::from_str(
            diagnostics
                .split_once("\nBundle: ")
                .map_or(diagnostics.as_str(), |(body, _)| body),
        )?;
        let api_raw = self.get("/v1/readiness")?;
        self.record(&format!("{label}-api-readiness.json"), api_raw.as_bytes())?;
        let api: Value = serde_json::from_str(&api_raw)?;
        let api_diagnostics_raw = self.get("/v1/diagnostics")?;
        self.record(
            &format!("{label}-api-diagnostics.json"),
            api_diagnostics_raw.as_bytes(),
        )?;
        let api_diagnostics: Value = serde_json::from_str(&api_diagnostics_raw)?;
        let extracted = Command::new("/usr/bin/unzip")
            .args(["-p"])
            .arg(&bundle)
            .arg("snapshot.json")
            .output()?;
        assert!(extracted.status.success());
        let snapshot: Value = serde_json::from_slice(&extracted.stdout)?;
        self.record(&format!("{label}-bundle.zip"), &fs::read(&bundle)?)?;
        self.record(&format!("{label}-bundle-snapshot.json"), &extracted.stdout)?;
        let expected = &cli["runtime"]["spec031_readiness"];
        assert_eq!(&api, expected);
        assert_eq!(&api_diagnostics["runtime"]["spec031_readiness"], expected);
        assert_eq!(&snapshot["runtime"]["spec031_readiness"], expected);
        let owner: Value = serde_json::from_str(&self.get("/v1/trusted-runtime")?)?;
        assert_eq!(api["trusted_runtime"], owner);
        let inspect = self.cli(&format!("{label}-inspect"), &["runtime", "inspect"])?;
        let aggregates = inspect
            .lines()
            .filter(|line| line.starts_with("Spec031 readiness:"))
            .collect::<Vec<_>>();
        assert_eq!(aggregates.len(), 1, "one authoritative readiness aggregate");
        assert!(aggregates[0].contains(&format!(
            "state={}",
            api["envelope"]["state"].as_str().ok_or("aggregate state")?
        )));
        for (kind, subject) in [
            ("plugin_app", "plugin-app"),
            ("runtime_controls", "runtime-controls"),
            ("resource_disclosure", "resource-disclosure"),
        ] {
            let component = api["components"]
                .as_array()
                .ok_or("components")?
                .iter()
                .find(|component| component["kind"] == kind)
                .ok_or("plugin/app")?;
            let envelope = api["envelope"]["children"]
                .as_array()
                .ok_or("children")?
                .iter()
                .find(|child| {
                    child["lineage"]["subject_ref"] == format!("subject:readiness:{subject}")
                })
                .ok_or("plugin/app envelope")?;
            let line = inspect
                .lines()
                .find(|line| line.starts_with(&format!("Spec031 readiness.{kind}:")))
                .ok_or("plugin/app line")?;
            for (key, value) in [
                ("state", &component["state"]),
                ("severity", &envelope["severity"]),
                ("reason", &component["reason_code"]),
                ("freshness", &component["freshness"]),
            ] {
                assert!(
                    line.contains(&format!(
                        "{key}={}",
                        value.as_str().ok_or("canonical string")?
                    )),
                    "{line}"
                );
            }
            let remediation = &envelope["capability"]["details"]["remediation"];
            assert!(
                line.contains(&format!("remediation={remediation}")),
                "{line}"
            );
            self.record(&format!("{label}-{kind}-comparison.json"), &serde_json::to_vec_pretty(&serde_json::json!({
            "full_readiness_equal": true, "cli_human_fields_equal": true,
            "component": component, "envelope": envelope, "aggregate_state": api["envelope"]["state"],
            "scope": kind, "aggregate_asserted_ready": false
        }))?)?;
        }
        Ok(api)
    }

    pub fn record(&self, name: &str, bytes: &[u8]) -> TestResult {
        if let Some(root) = &self.export {
            fs::write(root.join(name), bytes)?;
        }
        Ok(())
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let stopped = match self.child.take() {
            Some(mut child) => {
                let _ = child.kill();
                child.wait().is_ok()
            }
            None => true,
        };
        for name in ["serve.stdout", "serve.stderr"] {
            if let Ok(bytes) = fs::read(self.root.join(name)) {
                let _ = self.record(name, &bytes);
            }
        }
        let matched = fs::symlink_metadata(&self.root).is_ok_and(|metadata| {
            metadata.is_dir() && (metadata.dev(), metadata.ino()) == self.identity
        });
        let removed = matched && stopped && fs::remove_dir_all(&self.root).is_ok();
        let _ = self.record(
            "cleanup.json",
            serde_json::json!({
                "root": self.root, "identity_matched": matched, "child_reaped": stopped,
                "owned_root_removed": removed, "default_user_data_accessed": false
            })
            .to_string()
            .as_bytes(),
        );
    }
}
