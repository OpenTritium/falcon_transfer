use futures::StreamExt;
use std::time::Duration;
use tokio::{
    sync::mpsc::{Sender, channel},
    task::AbortHandle,
};
use tokio_util::time::DelayQueue;

type ResetCallback = Box<dyn FnOnce() + Send + 'static>;

pub struct RecoveryTask {
    delay: Duration,
    callback: ResetCallback,
}

impl RecoveryTask {
    pub fn new(delay: Duration, callback: ResetCallback) -> Self {
        Self { delay, callback }
    }
}

pub struct RecoveryScheduler {
    abort: AbortHandle,
}

impl RecoveryScheduler {
    pub fn run() -> (Self, Sender<RecoveryTask>) {
        let (tx, mut rx) = channel::<RecoveryTask>(128); // todo 认真考虑背压    
        let abort = tokio::spawn(async move {
            let mut delay_queue = DelayQueue::new();
            loop {
                tokio::select! {
                    // 接收新任务
                    Some(task) = rx.recv() => {
                        delay_queue.insert(task.callback, task.delay);
                    }
                    // 处理到期任务
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

impl Drop for RecoveryScheduler {
    fn drop(&mut self) {
        self.abort.abort();
    }
}
