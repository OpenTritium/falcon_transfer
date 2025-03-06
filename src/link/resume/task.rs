use std::time::Duration;

type ResetCallback = Box<dyn FnOnce() + Send + 'static>;

pub struct ResumeTask {
    pub delay: Duration,
    pub callback: ResetCallback,
}

impl ResumeTask {
    pub fn new(delay: Duration, callback: ResetCallback) -> Self {
        Self { delay, callback }
    }
}
