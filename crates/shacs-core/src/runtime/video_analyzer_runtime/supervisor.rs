use super::invocation::AnalyzerInvocation;
use super::staging::AnalyzerStagingLease;
use crate::runtime::{
    VideoContextAnalysis, VideoContextAnalyzer, VideoContextError, VideoContextRequest,
};
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{mpsc, Arc, Mutex, MutexGuard};
use std::thread;
use std::time::Duration;

const TERMINAL_RUNNING: u8 = 0;
const TERMINAL_COMPLETED: u8 = 1;
const TERMINAL_CANCELLED: u8 = 2;
const TERMINAL_TIMED_OUT: u8 = 3;
const WAIT_STEP: Duration = Duration::from_millis(5);

#[derive(Debug)]
pub(crate) struct SupervisedVideoAnalyzer {
    analyzer: Arc<dyn VideoContextAnalyzer>,
    active: Mutex<bool>,
}

impl SupervisedVideoAnalyzer {
    pub(crate) fn new(analyzer: Arc<dyn VideoContextAnalyzer>) -> Self {
        Self {
            analyzer,
            active: Mutex::new(false),
        }
    }

    fn try_acquire(self: &Arc<Self>) -> Option<AnalyzerPermit> {
        let mut active = recover_lock(&self.active);
        if *active {
            return None;
        }
        *active = true;
        Some(AnalyzerPermit {
            owner: Arc::clone(self),
        })
    }
}

struct AnalyzerPermit {
    owner: Arc<SupervisedVideoAnalyzer>,
}

impl Drop for AnalyzerPermit {
    fn drop(&mut self) {
        *recover_lock(&self.owner.active) = false;
    }
}

pub(crate) enum SupervisedVideoAnalyzerOutcome {
    Completed(Box<SupervisedVideoAnalyzerCompletion>),
    Busy,
    Cancelled,
    TimedOut,
    Failed,
}

pub(crate) struct SupervisedVideoAnalyzerCompletion {
    result: Result<VideoContextAnalysis, VideoContextError>,
    _staging: AnalyzerStagingLease,
}

impl SupervisedVideoAnalyzerCompletion {
    pub(crate) fn result(&self) -> &Result<VideoContextAnalysis, VideoContextError> {
        &self.result
    }
}

pub(crate) fn run_supervised_video_analyzer(
    analyzer: Arc<SupervisedVideoAnalyzer>,
    invocation: AnalyzerInvocation,
    request: VideoContextRequest,
) -> SupervisedVideoAnalyzerOutcome {
    if invocation.is_cancelled() {
        return SupervisedVideoAnalyzerOutcome::Cancelled;
    }
    if invocation.deadline_elapsed() {
        return SupervisedVideoAnalyzerOutcome::TimedOut;
    }
    let Some(permit) = analyzer.try_acquire() else {
        return SupervisedVideoAnalyzerOutcome::Busy;
    };
    let staging = match AnalyzerStagingLease::create(
        invocation.staging_root().to_path_buf(),
        invocation.staging_directory().to_path_buf(),
    ) {
        Ok(staging) => staging,
        Err(()) => return SupervisedVideoAnalyzerOutcome::Failed,
    };
    let terminal = Arc::new(AtomicU8::new(TERMINAL_RUNNING));
    let worker_terminal = Arc::clone(&terminal);
    let worker_invocation = invocation.clone();
    let worker_analyzer = Arc::clone(&analyzer.analyzer);
    let (sender, receiver) = mpsc::sync_channel(1);
    let worker = thread::spawn(move || {
        let _permit = permit;
        let result = worker_analyzer.analyze(&worker_invocation, request);
        if worker_invocation.is_cancelled() {
            let _ = worker_terminal.compare_exchange(
                TERMINAL_RUNNING,
                TERMINAL_CANCELLED,
                Ordering::SeqCst,
                Ordering::SeqCst,
            );
        } else if worker_invocation.deadline_elapsed() {
            let _ = worker_terminal.compare_exchange(
                TERMINAL_RUNNING,
                TERMINAL_TIMED_OUT,
                Ordering::SeqCst,
                Ordering::SeqCst,
            );
        } else if worker_terminal
            .compare_exchange(
                TERMINAL_RUNNING,
                TERMINAL_COMPLETED,
                Ordering::SeqCst,
                Ordering::SeqCst,
            )
            .is_ok()
        {
            let _ = sender.send(SupervisedVideoAnalyzerCompletion {
                result,
                _staging: staging,
            });
        }
    });

    loop {
        if invocation.is_cancelled() {
            let _ = terminal.compare_exchange(
                TERMINAL_RUNNING,
                TERMINAL_CANCELLED,
                Ordering::SeqCst,
                Ordering::SeqCst,
            );
        } else if invocation.deadline_elapsed() {
            let _ = terminal.compare_exchange(
                TERMINAL_RUNNING,
                TERMINAL_TIMED_OUT,
                Ordering::SeqCst,
                Ordering::SeqCst,
            );
        }
        match receiver.recv_timeout(WAIT_STEP) {
            Ok(completion) => {
                if worker.join().is_err() {
                    return SupervisedVideoAnalyzerOutcome::Failed;
                }
                return terminal_outcome(&terminal, Some(completion));
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                if worker.join().is_err() {
                    return SupervisedVideoAnalyzerOutcome::Failed;
                }
                return terminal_outcome(&terminal, None);
            }
        }
    }
}

fn terminal_outcome(
    terminal: &AtomicU8,
    completion: Option<SupervisedVideoAnalyzerCompletion>,
) -> SupervisedVideoAnalyzerOutcome {
    match (terminal.load(Ordering::SeqCst), completion) {
        (TERMINAL_COMPLETED, Some(completion)) => {
            SupervisedVideoAnalyzerOutcome::Completed(Box::new(completion))
        }
        (TERMINAL_CANCELLED, _) => SupervisedVideoAnalyzerOutcome::Cancelled,
        (TERMINAL_TIMED_OUT, _) => SupervisedVideoAnalyzerOutcome::TimedOut,
        (TERMINAL_RUNNING | TERMINAL_COMPLETED, _) => SupervisedVideoAnalyzerOutcome::Failed,
        _ => SupervisedVideoAnalyzerOutcome::Failed,
    }
}

fn recover_lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    match mutex.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}
