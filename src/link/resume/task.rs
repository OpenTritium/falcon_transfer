use std::time::Duration;

type ResetCallback = Box<dyn FnOnce() + Send + 'static>;

pub struct LinkResumeTask {
    pub timeout: Duration,
    pub callback: ResetCallback,
}

impl LinkResumeTask {
    pub fn new(timeout: Duration, callback: ResetCallback) -> Self {
        Self { timeout, callback }
    }
}
