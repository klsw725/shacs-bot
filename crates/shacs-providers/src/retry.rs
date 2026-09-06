use std::thread;
use std::time::Duration;

mod execution;
mod fallback;
mod policy;

pub use execution::{
    chat_stream_with_retry, chat_stream_with_retry_using_waiter, chat_with_retry,
    chat_with_retry_using_waiter,
};
pub use policy::{
    is_transient_provider_error, is_transient_response, retry_after_from_response,
    retry_decision_for_error, retry_decision_for_error_with_identical_count,
    retry_decision_for_response,
};

const STANDARD_DELAYS: [f64; 3] = [1.0, 2.0, 4.0];
const PERSISTENT_MAX_DELAY_S: f64 = 60.0;
const PERSISTENT_IDENTICAL_ERROR_LIMIT: u32 = 10;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderRetryMode {
    Standard,
    Persistent,
}

impl ProviderRetryMode {
    pub fn from_config(value: &str) -> Self {
        if value.eq_ignore_ascii_case("persistent") {
            Self::Persistent
        } else {
            Self::Standard
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetryStopReason {
    NotError,
    NonTransient,
    AttemptsExhausted,
    IdenticalTransientErrorLimit,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProviderRetryDecision {
    pub should_retry: bool,
    pub delay_s: Option<f64>,
    pub stop_reason: Option<RetryStopReason>,
}

pub trait ProviderRetryWaiter {
    fn wait(&mut self, delay_s: f64, message: &str);
}

#[derive(Debug, Clone, Default)]
pub struct ThreadRetryWaiter {
    cancellation: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
    deadline: Option<std::time::Instant>,
}

impl ThreadRetryWaiter {
    pub fn new(cancellation: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>) -> Self {
        Self {
            cancellation,
            deadline: None,
        }
    }

    pub fn with_deadline(
        cancellation: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
        deadline: Option<std::time::Instant>,
    ) -> Self {
        Self {
            cancellation,
            deadline,
        }
    }
}

impl ProviderRetryWaiter for ThreadRetryWaiter {
    fn wait(&mut self, delay_s: f64, _message: &str) {
        let requested_deadline =
            std::time::Instant::now() + Duration::from_secs_f64(delay_s.max(0.0));
        let deadline = self.deadline.map_or(requested_deadline, |deadline| {
            deadline.min(requested_deadline)
        });
        while std::time::Instant::now() < deadline {
            if self
                .cancellation
                .as_ref()
                .is_some_and(|flag| flag.load(std::sync::atomic::Ordering::SeqCst))
            {
                return;
            }
            thread::sleep(
                deadline
                    .saturating_duration_since(std::time::Instant::now())
                    .min(Duration::from_millis(10)),
            );
        }
    }
}
