use std::error::Error;
use std::path::Path;
use std::thread;
use std::time::{Duration, Instant};

pub fn wait_for_path(path: &Path) -> Result<(), Box<dyn Error>> {
    let deadline = Instant::now() + Duration::from_secs(3);
    while !path.exists() {
        if Instant::now() >= deadline {
            return Err(format!("timed out waiting for {}", path.display()).into());
        }
        thread::sleep(Duration::from_millis(10));
    }
    Ok(())
}

pub fn wait_for_process_exit(pid: i32) -> Result<(), Box<dyn Error>> {
    let deadline = Instant::now() + Duration::from_secs(3);
    while nix::sys::signal::kill(nix::unistd::Pid::from_raw(pid), None).is_ok() {
        if Instant::now() >= deadline {
            return Err(format!("descendant {pid} survived cleanup").into());
        }
        thread::sleep(Duration::from_millis(10));
    }
    Ok(())
}
