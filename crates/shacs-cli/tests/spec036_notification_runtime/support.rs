use shacs_core::runtime::EntrustedObservationSnapshot;
use std::fs;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub type TestResult = Result<(), Box<dyn std::error::Error>>;
pub const OWNER: &str = "email:owner@example.test";

pub struct Runtime {
    pub root: PathBuf,
    pub workspace: PathBuf,
    pub data: PathBuf,
    pub work_id: String,
    child: Option<Child>,
}

impl Runtime {
    pub fn new(port: u16) -> Result<Self, Box<dyn std::error::Error>> {
        let root = tempfile::Builder::new()
            .prefix("shacs-spec036-notification-")
            .tempdir()?
            .keep();
        let workspace = root.join("workspace");
        let data = root.join("data");
        fs::create_dir_all(&data)?;
        fs::create_dir_all(&workspace)?;
        fs::write(
            data.join("config.json"),
            serde_json::to_vec_pretty(&serde_json::json!({
                "agents":{"defaults":{"workspace":workspace,"provider":"openai","model":"gpt-4o"}},
                "providers":{"openai":{"apiKey":"local-qa-not-a-secret","apiBase":"http://127.0.0.1:1/v1"}},
                "channels":{"websocket":{"enabled":false},"email":{"enabled":true,"consentGranted":true,"smtp":{"host":"127.0.0.1","port":port,"from":"bot@example.test","security":"plain","timeoutSeconds":2}},"sendMaxRetries":1}
            }))?,
        )?;
        let mut runtime = Self {
            root,
            workspace,
            data,
            work_id: String::new(),
            child: None,
        };
        runtime.run(&["onboard"])?;
        runtime.run(&["session", "create", "--session", OWNER])?;
        let created: serde_json::Value = serde_json::from_str(&runtime.run(&[
            "work",
            "create",
            "watch local integer",
            "--session",
            OWNER,
            "--consent",
            "explicit-request",
            "--evidence-ref",
            "cli:local-qa",
            "--instructions",
            "notify when integer <= 100",
        ])?)?;
        runtime.work_id = created["work"]["work_id"]
            .as_str()
            .ok_or("work id")?
            .to_owned();
        runtime.source(120)?;
        runtime.work(&[
            "observe",
            &runtime.work_id,
            "--source",
            "source.json",
            "--cadence-seconds",
            "5",
            "--expected-revision",
            "1",
            "--at-most",
            "100",
            "--max-age-seconds",
            "3600",
        ])?;
        println!("QA_ROOT={}", runtime.root.display());
        Ok(runtime)
    }

    fn command(&self, args: &[&str]) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_shacs-bot"));
        command
            .arg("--config")
            .arg(self.data.join("config.json"))
            .args(args)
            .arg("--workspace")
            .arg(&self.workspace);
        command
    }

    pub fn run(&self, args: &[&str]) -> Result<String, Box<dyn std::error::Error>> {
        let result = self.command(args).output()?;
        if !result.status.success() {
            return Err(format!("{args:?}: {}", String::from_utf8_lossy(&result.stderr)).into());
        }
        Ok(String::from_utf8(result.stdout)?)
    }

    pub fn work(&self, args: &[&str]) -> Result<String, Box<dyn std::error::Error>> {
        let mut arguments = vec!["work"];
        arguments.extend_from_slice(args);
        arguments.extend(["--session", OWNER]);
        self.run(&arguments)
    }

    pub fn start(&mut self) -> TestResult {
        let log_path = self.root.join("runtime.log");
        let previous_plans = match fs::read_to_string(&log_path) {
            Ok(log) => log.matches("Channel runtime plan").count(),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => 0,
            Err(error) => return Err(error.into()),
        };
        let log = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.root.join("runtime.log"))?;
        self.child = Some(
            self.command(&["runtime", "start"])
                .stdout(Stdio::from(log.try_clone()?))
                .stderr(Stdio::from(log))
                .spawn()?,
        );
        let deadline = Instant::now() + Duration::from_secs(15);
        while fs::read_to_string(&log_path)?
            .matches("Channel runtime plan")
            .count()
            == previous_plans
        {
            if let Some(status) = self.child.as_mut().ok_or("runtime child")?.try_wait()? {
                return Err(format!(
                    "startup exited {status}: {}",
                    fs::read_to_string(&log_path)?
                )
                .into());
            }
            if Instant::now() > deadline {
                return Err("runtime startup timeout".into());
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        Ok(())
    }

    pub fn stop(&mut self) -> TestResult {
        self.run(&["runtime", "stop"])?;
        let deadline = Instant::now() + Duration::from_secs(15);
        if let Some(child) = self.child.as_mut() {
            loop {
                if let Some(status) = child.try_wait()? {
                    assert!(status.success(), "runtime exited {status}");
                    break;
                }
                if Instant::now() > deadline {
                    return Err("runtime stop timeout".into());
                }
                std::thread::sleep(Duration::from_millis(50));
            }
        }
        self.child = None;
        Ok(())
    }

    pub fn start_api(&mut self) -> TestResult {
        let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
        let port = listener.local_addr()?.port().to_string();
        drop(listener);
        let log = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.root.join("runtime.log"))?;
        self.child = Some(
            self.command(&["serve", "--host", "127.0.0.1", "--port", &port])
                .stdout(Stdio::from(log.try_clone()?))
                .stderr(Stdio::from(log))
                .spawn()?,
        );
        Ok(())
    }

    pub fn source(&self, value: i64) -> TestResult {
        let timestamp = u64::try_from(SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis())?;
        let temporary = self.workspace.join("source.next");
        fs::write(
            &temporary,
            serde_json::to_vec(
                &serde_json::json!({"schema":"integer_v1","value":value,"as_of_ms":timestamp}),
            )?,
        )?;
        fs::rename(temporary, self.workspace.join("source.json"))?;
        Ok(())
    }

    pub fn snapshot(&self) -> Result<EntrustedObservationSnapshot, Box<dyn std::error::Error>> {
        Ok(serde_json::from_str(&self.work(&[
            "observations",
            &self.work_id,
            "--data-dir",
            self.data.to_str().ok_or("data path")?,
        ])?)?)
    }

    pub fn wait(
        &mut self,
        label: &str,
        predicate: impl Fn(&EntrustedObservationSnapshot) -> bool,
    ) -> Result<EntrustedObservationSnapshot, Box<dyn std::error::Error>> {
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            if let Some(child) = self.child.as_mut() {
                if let Some(status) = child.try_wait()? {
                    return Err(format!(
                        "runtime exited {status}: {}",
                        fs::read_to_string(self.root.join("runtime.log"))?
                    )
                    .into());
                }
            }
            let snapshot = self.snapshot()?;
            if predicate(&snapshot) {
                let json = serde_json::to_string_pretty(&snapshot)?;
                fs::write(self.root.join(format!("{label}.json")), &json)?;
                println!(
                    "{label}: {}",
                    serde_json::to_string(
                        &serde_json::json!({"history":snapshot.history.len(),"verdict":snapshot.current_decision.as_ref().map(|decision| decision.verdict),"notifications":snapshot.notifications})
                    )?
                );
                return Ok(snapshot);
            }
            if Instant::now() > deadline {
                return Err(
                    format!("{label} timed out: {}", serde_json::to_string(&snapshot)?).into(),
                );
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }
}

impl Drop for Runtime {
    fn drop(&mut self) {
        if let Some(child) = self.child.as_mut() {
            if let Err(error) = child.kill() {
                eprintln!("QA child cleanup: {error}");
            }
            if let Err(error) = child.wait() {
                eprintln!("QA child wait: {error}");
            }
        }
    }
}
