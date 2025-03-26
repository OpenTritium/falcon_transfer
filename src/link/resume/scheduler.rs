use super::task::ResumeTask;
use futures::StreamExt;
use thiserror::Error;
use tokio::sync::mpsc::error::TrySendError;
use tokio::{
    sync::mpsc::{Sender, channel},
    task::AbortHandle,
};
use tokio_util::time::DelayQueue;

#[derive(Debug, Error)]
pub enum ResumeTaskError {
    #[error(transparent)]
    TaskSendError(#[from] TrySendError<ResumeTask>),
}

pub struct ResumeScheduler {
    abort: AbortHandle,
}

impl ResumeScheduler {
    pub fn run() -> (Self, Sender<ResumeTask>) {
        let (tx, mut rx) = channel::<ResumeTask>(128); // todo 认真考虑背压
        let abort = tokio::spawn(async move {
            let mut delay_queue = DelayQueue::new();
            loop {
                tokio::select! {
                    Some(task) = rx.recv() => {
                        delay_queue.insert(task.callback, task.timeout);
                    }
                    Some(expired) = delay_queue.next() => {
                        let callback = expired.into_inner();
                        callback();
                    }
                }
            }
        })
        .abort_handle();
        (Self { abort }, tx)
    }
}

impl Drop for ResumeScheduler {
    fn drop(&mut self) {
        self.abort.abort();
    }
}
