use crate::{endpoint::EndPoint, link_recovery_scheduler::RecoveryTask};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering},
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use thiserror::Error;
use tracing::{error, info};

#[derive(Debug, Error)]
pub enum LinkError {
    #[error("No healthy links available")]
    NoHealthyLinks,
    #[error("Link failure: {0}")]
    Failure(String),
}

#[derive(Debug)]
pub struct LinkState {
    pub addr_local: EndPoint,
    pub addr_remote: EndPoint,
    pub metric: u64,
    pub failure_count: AtomicU8,
    pub is_healthy: AtomicBool,
    pub last_used: AtomicU64,
}

impl LinkState {
    pub fn new(addr_local: EndPoint, addr_remote: EndPoint, metric: u64) -> Self {
        Self {
            addr_local,
            addr_remote,
            metric,
            failure_count: AtomicU8::new(0),
            is_healthy: AtomicBool::new(true),
            last_used: AtomicU64::new(0),
        }
    }

    pub fn reset(&self) {
        self.failure_count.store(0, Ordering::SeqCst);
        self.is_healthy.store(true, Ordering::SeqCst);
        info!("Link {}->{} recovered", self.addr_local, self.addr_remote);
    }
    // 应当对不同系统有不一样的行为
    pub fn weight(&self) -> u64 {
        // Use inverse metric + 1 to avoid division by zero
        // Higher metric means lower weight
        1_000_000 / (self.metric + 1)
    }
    // 分配链路后立刻调用
    pub fn update_usage(&self) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        self.last_used.store(now, Ordering::Relaxed);
    }
}

pub trait Fade {
    fn delay(self: Arc<Self>) -> Option<RecoveryTask>;
}

impl Fade for LinkState {
    // 链路状态表负责调用此函数，返回some代表还有推迟的必要
    fn delay(self: Arc<Self>) -> Option<RecoveryTask> {
        // 记录错误次数，将链路标记为不健康
        let failure_count = self.failure_count.fetch_add(1, Ordering::SeqCst) + 1;
        self.is_healthy.store(false, Ordering::SeqCst);
        let delay = match failure_count {
            0 => unreachable!(), //调用此函数说明至少错了一次
            1 => Duration::from_secs(5).into(),
            2 => Duration::from_secs(30).into(),
            3 => Duration::from_mins(1).into(),
            _ => return None, // 当链路状态返回无的时候，链路状态表drop它
        };
        Some(RecoveryTask::new(delay, Box::new(move || self.reset())))
    }
}
