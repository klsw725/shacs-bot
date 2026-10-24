use super::mp4_video_bytes;
use shacs_core::runtime::{ContextBuilder, VideoContextAnalyzer};
use std::error::Error;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

pub(super) struct VideoFixture {
    workspace: tempfile::TempDir,
    media_root: tempfile::TempDir,
    pub(super) media: Vec<String>,
}

impl VideoFixture {
    pub(super) fn new() -> Result<Self, Box<dyn Error>> {
        let workspace = tempfile::tempdir()?;
        let media_root = tempfile::tempdir()?;
        let attachments = media_root.path().join("attachments/cli");
        std::fs::create_dir_all(&attachments)?;
        let video = attachments.join("clip.mp4");
        std::fs::write(&video, mp4_video_bytes(6))?;
        Ok(Self {
            workspace,
            media_root,
            media: vec![video.to_string_lossy().to_string()],
        })
    }

    pub(super) fn workspace(&self) -> &Path {
        self.workspace.path()
    }

    pub(super) fn staging_root(&self) -> PathBuf {
        self.workspace().join("video-analyzer-staging")
    }

    pub(super) fn context(&self, analyzer: Arc<dyn VideoContextAnalyzer>) -> ContextBuilder {
        ContextBuilder::new(self.workspace())
            .with_media_roots([self.media_root.path().to_path_buf()])
            .with_video_analyzer(analyzer)
            .with_video_analyzer_staging_root(self.staging_root())
    }
}

pub(super) fn wait_for_absence(path: &Path) -> Result<(), Box<dyn Error>> {
    let deadline = Instant::now() + Duration::from_secs(2);
    while path.exists() && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(10));
    }
    if path.exists() {
        return Err(format!(
            "staging path remained after terminal state: {}",
            path.display()
        )
        .into());
    }
    Ok(())
}

pub(super) fn wait_for_presence(path: &Path) -> Result<(), Box<dyn Error>> {
    let deadline = Instant::now() + Duration::from_secs(2);
    while !path.exists() && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(10));
    }
    if !path.exists() {
        return Err(format!("expected fixture path was not created: {}", path.display()).into());
    }
    Ok(())
}

#[cfg(unix)]
pub(super) fn wait_for_process_exit(pid: i32) -> Result<(), Box<dyn Error>> {
    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline {
        let running = std::process::Command::new("/bin/kill")
            .args(["-0", &pid.to_string()])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()?
            .success();
        if !running {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(10));
    }
    Err(format!("controlled child process remained after cleanup: pid={pid}").into())
}
