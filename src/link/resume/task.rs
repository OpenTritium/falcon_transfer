use std::time::Duration;

type ResetCallback = Box<dyn FnOnce() + Send + 'static>;

pub struct ResumeTask {
    pub timeout: Duration,
    pub callback: ResetCallback,
}

impl ResumeTask {
    pub fn new(timeout: Duration, callback: ResetCallback) -> Self {
        Self { timeout, callback }
    }
}
