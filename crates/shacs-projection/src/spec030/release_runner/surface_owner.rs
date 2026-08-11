use super::model::{
    Spec030ReleaseArtifactError, Spec030SurfaceOwnerEvidence, Spec030SurfaceOwnerReadiness,
    Spec030SurfaceOwnerShutdown, Spec030SurfaceOwnerSpawnSpec,
};
use super::source_manifest::sha256_bytes;
use crate::release_evidence::EvidenceWriter;
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

pub(super) struct ProductionOwner {
    child: Option<Child>,
    owner_pid: u32,
    port: u16,
    spawn: Spec030SurfaceOwnerSpawnSpec,
    argv: Vec<String>,
    temp_root: OwnerTempRoot,
    stdout_temp: PathBuf,
    stderr_temp: PathBuf,
}

struct OwnerTempRoot {
    path: Option<PathBuf>,
}

impl OwnerTempRoot {
    fn create(path: PathBuf) -> Result<Self, Spec030ReleaseArtifactError> {
        std::fs::create_dir(&path).map_err(|_| Spec030ReleaseArtifactError::Io)?;
        Ok(Self { path: Some(path) })
    }

    fn remove(&mut self) -> Result<(), Spec030ReleaseArtifactError> {
        let path = self
            .path
            .take()
            .ok_or(Spec030ReleaseArtifactError::InvalidSurfaceEvidence)?;
        std::fs::remove_dir_all(path).map_err(|_| Spec030ReleaseArtifactError::Io)
    }
}

impl Drop for OwnerTempRoot {
    fn drop(&mut self) {
        if let Some(path) = self.path.take() {
            let _ = std::fs::remove_dir_all(path);
        }
    }
}

impl ProductionOwner {
    pub(super) fn start(
        config: &super::model::Spec030ReleaseRunnerConfig,
        port: u16,
    ) -> Result<Self, Spec030ReleaseArtifactError> {
        let temp_root = config.evidence_root.join("surface/owner-tmp");
        let temp_root_guard = OwnerTempRoot::create(temp_root.clone())?;
        let stdout_temp = temp_root.join("stdout");
        let stderr_temp = temp_root.join("stderr");
        let stdout =
            std::fs::File::create(&stdout_temp).map_err(|_| Spec030ReleaseArtifactError::Io)?;
        let stderr =
            std::fs::File::create(&stderr_temp).map_err(|_| Spec030ReleaseArtifactError::Io)?;
        let spawn = super::surface_owner_spawn::spawn_spec(config, port);
        if !std::path::Path::new(&spawn.executable).is_file() {
            return Err(Spec030ReleaseArtifactError::InvalidSurfaceEvidence);
        }
        let argv = spawn.argv();
        let child = Command::new(&spawn.executable)
            .args(&argv[1..])
            .env(
                "SHACS_DEBUG_FAKE_PROVIDER_RESPONSES",
                config.evidence_root.join("surface/provider-responses.json"),
            )
            .env(
                super::surface_runner::OWNER_CREDENTIAL_ENV,
                super::surface_runner::OWNER_CREDENTIAL_VALUE,
            )
            .current_dir(&config.repo_root)
            .stdin(Stdio::null())
            .stdout(Stdio::from(stdout))
            .stderr(Stdio::from(stderr))
            .spawn()
            .map_err(|_| Spec030ReleaseArtifactError::Io)?;
        let owner_pid = child.id();
        Ok(Self {
            child: Some(child),
            owner_pid,
            port,
            spawn,
            argv,
            temp_root: temp_root_guard,
            stdout_temp,
            stderr_temp,
        })
    }

    pub(super) fn wait_until_ready(
        &mut self,
        timeout: Duration,
    ) -> Result<(), Spec030ReleaseArtifactError> {
        let deadline = Instant::now() + timeout;
        loop {
            if self.exited()? {
                return Err(Spec030ReleaseArtifactError::InvalidSurfaceEvidence);
            }
            if super::surface_owner_http::status(
                self.port,
                "GET",
                "/v1/trusted-runtime?schema_version=1",
                None,
                Duration::from_millis(250),
            ) == Ok(200)
            {
                return Ok(());
            }
            if Instant::now() >= deadline {
                return Err(Spec030ReleaseArtifactError::InvalidSurfaceEvidence);
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    pub(super) fn exercise_runtime(
        &mut self,
        config: &super::model::Spec030ReleaseRunnerConfig,
        timeout: Duration,
    ) -> Result<(), Spec030ReleaseArtifactError> {
        super::surface_owner_http::exercise(self.port, config, timeout)?;
        if self.exited()? {
            return Err(Spec030ReleaseArtifactError::InvalidSurfaceEvidence);
        }
        Ok(())
    }

    pub(super) fn stop(
        mut self,
        config: &super::model::Spec030ReleaseRunnerConfig,
        writer: &EvidenceWriter,
    ) -> Result<Spec030SurfaceOwnerEvidence, Spec030ReleaseArtifactError> {
        let binary = config
            .repo_root
            .join("crates/target/debug")
            .join(format!("shacs-bot{}", std::env::consts::EXE_SUFFIX));
        let status = Command::new(binary)
            .args(["runtime", "stop", "--config"])
            .arg(config.evidence_root.join("surface/config.json"))
            .arg("--workspace")
            .arg(config.evidence_root.join("surface/workspace"))
            .current_dir(&config.repo_root)
            .status()
            .map_err(|_| Spec030ReleaseArtifactError::Io)?;
        if !status.success() {
            return Err(Spec030ReleaseArtifactError::InvalidSurfaceEvidence);
        }
        let deadline = Instant::now() + config.command_timeout;
        loop {
            if self.exited()? {
                break;
            }
            if Instant::now() >= deadline {
                return Err(Spec030ReleaseArtifactError::InvalidSurfaceEvidence);
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        self.child.take();
        let stdout =
            std::fs::read(&self.stdout_temp).map_err(|_| Spec030ReleaseArtifactError::Io)?;
        let stderr =
            std::fs::read(&self.stderr_temp).map_err(|_| Spec030ReleaseArtifactError::Io)?;
        writer
            .write_new("surface/owner.stdout", &stdout)
            .map_err(|_| Spec030ReleaseArtifactError::Io)?;
        writer
            .write_new("surface/owner.stderr", &stderr)
            .map_err(|_| Spec030ReleaseArtifactError::Io)?;
        self.temp_root.remove()?;
        super::surface_owner_evidence::signed(Spec030SurfaceOwnerEvidence {
            schema: super::surface_owner_evidence::SURFACE_OWNER_SCHEMA.to_owned(),
            production_owner: true,
            owner_pid: self.owner_pid,
            spawn: self.spawn.clone(),
            argv: self.argv.clone(),
            bind_host: "127.0.0.1".to_owned(),
            requested_port: 0,
            bound_port: self.port,
            readiness: Spec030SurfaceOwnerReadiness::Observed,
            shutdown: Spec030SurfaceOwnerShutdown::Reaped,
            temp_root: "surface/owner-tmp".to_owned(),
            temp_root_removed: true,
            stdout_path: "surface/owner.stdout".to_owned(),
            stderr_path: "surface/owner.stderr".to_owned(),
            stdout_sha256: sha256_bytes(&stdout),
            stderr_sha256: sha256_bytes(&stderr),
            receipt_sha256: String::new(),
        })
    }

    #[cfg(all(test, target_os = "linux"))]
    pub(super) const fn owner_pid(&self) -> u32 {
        self.owner_pid
    }

    fn exited(&mut self) -> Result<bool, Spec030ReleaseArtifactError> {
        self.child
            .as_mut()
            .ok_or(Spec030ReleaseArtifactError::InvalidSurfaceEvidence)?
            .try_wait()
            .map(|status| status.is_some())
            .map_err(|_| Spec030ReleaseArtifactError::Io)
    }
}

impl Drop for ProductionOwner {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

pub(super) fn ephemeral_port() -> Result<u16, Spec030ReleaseArtifactError> {
    let listener = TcpListener::bind("127.0.0.1:0")
        .map_err(|_| Spec030ReleaseArtifactError::InvalidSurfaceEvidence)?;
    listener
        .local_addr()
        .map(|address| address.port())
        .map_err(|_| Spec030ReleaseArtifactError::InvalidSurfaceEvidence)
}
