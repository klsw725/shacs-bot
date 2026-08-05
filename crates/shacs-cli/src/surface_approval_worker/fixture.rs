use super::*;
use std::error::Error;
use std::fs;
use std::sync::atomic::{AtomicUsize, Ordering};

#[path = "fixture/support.rs"]
mod support;

use support::{adapter, create_pending, write_owner_marker};

pub struct FixtureRuntime {
    adapter: Arc<AgentLoopChatCompletionAdapter>,
    calls: Arc<AtomicUsize>,
    data_dir: PathBuf,
    owner_id: String,
    owner_started_at_ms: u64,
    worker: Option<SurfaceApprovalWorker>,
    workspace: PathBuf,
}

impl FixtureRuntime {
    pub fn start(config_path: PathBuf, workspace: PathBuf) -> Result<Self, Box<dyn Error>> {
        let data_dir = config_path
            .parent()
            .map(Path::to_path_buf)
            .ok_or("config path has no parent")?;
        fs::create_dir_all(&workspace)?;
        fs::create_dir_all(data_dir.join("media").join("api"))?;
        fs::write(
            &config_path,
            serde_json::to_vec(&json!({"permissions": {"mode": "auto"}}))?,
        )?;
        let calls = Arc::new(AtomicUsize::new(0));
        let adapter = Arc::new(adapter(&config_path, &workspace, calls.clone()));
        create_pending(&adapter, "run cargo test")?;
        let now = now_millis();
        let owner_started_at_ms = now;
        let owner_id = write_owner_marker(&data_dir, std::process::id(), owner_started_at_ms, now)?;
        let worker =
            start_surface_approval_worker(adapter.clone(), data_dir.clone(), owner_id.clone())?;
        Ok(Self {
            adapter,
            calls,
            data_dir,
            owner_id,
            owner_started_at_ms,
            worker: Some(worker),
            workspace,
        })
    }

    pub fn owner_id(&self) -> &str {
        &self.owner_id
    }

    pub fn execution_count(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }

    pub fn pending_lineage(&self) -> Result<Option<String>, Box<dyn Error>> {
        let raw = SessionManager::new(&self.workspace)?.read_session_file("cli:surface-approval");
        Ok(raw.and_then(|value| {
            value["metadata"]["pending_permission_approval"]["approval_request_id"]
                .as_str()
                .map(str::to_owned)
        }))
    }

    pub fn create_pending(&self) -> Result<String, Box<dyn Error>> {
        create_pending(&self.adapter, "run cargo test")?;
        self.pending_lineage()?
            .ok_or_else(|| "missing pending approval".into())
    }

    pub fn replace_owner_generation(&mut self) -> Result<String, Box<dyn Error>> {
        let now = now_millis().saturating_add(10);
        self.owner_started_at_ms = now;
        self.owner_id = write_owner_marker(&self.data_dir, std::process::id(), now, now)?;
        Ok(self.owner_id.clone())
    }

    pub fn renew_owner(&self) -> Result<(), Box<dyn Error>> {
        write_owner_marker(
            &self.data_dir,
            std::process::id(),
            self.owner_started_at_ms,
            now_millis(),
        )?;
        Ok(())
    }

    pub fn terminal_summary(&self) -> Result<Value, Box<dyn Error>> {
        let replay = evaluate_durable_recovery(
            runtime_durable_event_root(&self.data_dir),
            runtime_durable_checkpoint_root(&self.data_dir),
        );
        let Some(state) = replay.state else {
            return Ok(json!([]));
        };
        Ok(json!(state
            .work
            .items
            .values()
            .filter(|item| item.work_kind == SURFACE_APPROVAL_WORK_KIND)
            .map(|item| json!({
                "work_id": item.work_id,
                "state": format!("{:?}", item.state),
                "terminal_kind": item.terminal_kind.map(|kind| format!("{:?}", kind)),
                "terminal_sequence": item.terminal_sequence,
            }))
            .collect::<Vec<_>>()))
    }

    pub fn snapshot(&self) -> Result<Value, Box<dyn Error>> {
        Ok(json!({
            "workspace": self.workspace,
            "data_dir": self.data_dir,
            "owner_id": self.owner_id,
            "pending_lineage": self.pending_lineage()?,
            "execution_count": self.execution_count(),
            "terminals": self.terminal_summary()?,
        }))
    }

    pub fn stop(mut self) -> Result<(), Box<dyn Error>> {
        if let Some(worker) = self.worker.take() {
            worker.stop()?;
        }
        Ok(())
    }
}
