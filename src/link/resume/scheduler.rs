use super::task::LinkResumeTask;
use futures::StreamExt;
use thiserror::Error;
use tokio::sync::mpsc::error::TrySendError;
use tokio::{
    sync::mpsc::{Sender, channel},
    task::AbortHandle,
};
use tokio_util::time::DelayQueue;
use tracing::info;

#[derive(Debug, Error)]
pub enum LinkResumeTaskError {
    #[error(transparent)]
    TaskSendError(#[from] TrySendError<LinkResumeTask>),
    #[error("the arc refference of this link is invalid for now")]
    LinkRefInvalid,
}

unsafe impl Sync for LinkResumeTaskError {}
unsafe impl Send for LinkResumeTaskError {}

pub struct LinkResumeScheduler {
    abort: AbortHandle,
}

impl LinkResumeScheduler {
    pub fn run() -> (Self, Sender<LinkResumeTask>) {
        let (tx, mut rx) = channel::<LinkResumeTask>(128); // todo 认真考虑背压
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

impl Drop for LinkResumeScheduler {
    fn drop(&mut self) {
        self.abort.abort();
        info!("Link Resume Scheduler has been dropped")
    }
}
