mod scheduler;
mod task;

pub use scheduler::*;
pub use task::*;

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::Ordering;
    use std::{
        sync::{Arc, atomic::AtomicBool},
        time::Duration,
    };
    use tokio::task::yield_now;
    #[tokio::test(start_paused = true)]
    async fn link_resume() {
        let (scheduler, task_sender) = LinkResumeScheduler::run();
        let shared_state = Arc::new(AtomicBool::new(false));
        let shared_state_clone = shared_state.clone();
        let task = LinkResumeTask::new(
            Duration::from_secs(3),
            Box::new(move || {
                shared_state_clone.store(true, Ordering::Release);
            }),
        );
        task_sender.send(task).await.unwrap();
        yield_now().await;
        tokio::time::advance(Duration::from_secs(10)).await;
        yield_now().await;
        assert!(shared_state.load(Ordering::Acquire));
        drop(scheduler);
    }
}
