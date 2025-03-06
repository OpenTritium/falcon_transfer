use super::ResumeTask;
use futures::StreamExt;
use tokio::{
    sync::mpsc::{channel, Sender},
    task::AbortHandle,
};
use tokio_util::time::DelayQueue;

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
                        delay_queue.insert(task.callback, task.delay);
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
