use crate::link::ResumeTask;
use crate::utils::EndPoint;
use bitflags::bitflags;
use std::hash::Hash;
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
    LinksNotFound,
    #[error("no way to reach this bond")]
    BondNotFound,
}
bitflags! {
    pub struct LinkStateFlag:u8 {
        const DISCOVED = 0; // 全0 表示仅仅才发现
        const HELLO = 1;
        const EXCHANGE = Self::HELLO.bits() << 1;
        const FULL = Self::EXCHANGE.bits() << 1;
        // 上面三个状态只能存在一个，且仅有full能与tranfer共存
        const TRANSFER = Self::FULL.bits() << 1;
    }
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

impl Hash for LinkState {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.addr_local.hash(state);
        self.addr_remote.hash(state);
        self.metric.hash(state);
        self.failure_count.load(Ordering::Acquire).hash(state);
        self.is_healthy.load(Ordering::Acquire).hash(state);
        self.last_used.load(Ordering::Relaxed).hash(state);
    }
}

impl PartialEq for LinkState {
    fn eq(&self, other: &Self) -> bool {
        self.addr_local == other.addr_local
            && self.addr_remote == other.addr_remote
            && self.metric == other.metric
            && self.failure_count.load(Ordering::Acquire)
                == other.failure_count.load(Ordering::Acquire)
            && self.is_healthy.load(Ordering::Acquire) == other.is_healthy.load(Ordering::Acquire)
            && self.last_used.load(Ordering::Relaxed) == other.last_used.load(Ordering::Relaxed)
    }
}

impl Eq for LinkState {}

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
        self.failure_count.store(0, Ordering::Release);
        self.is_healthy.store(true, Ordering::Release);
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

    pub fn delay(self: Arc<Self>) -> Option<ResumeTask> {
        // 记录错误次数，将链路标记为不健康
        // relaxed 足矣，马上有release同步
        let failure_count = self.failure_count.fetch_add(1, Ordering::Relaxed) + 1;
        self.is_healthy.store(false, Ordering::Release);
        let delay = match failure_count {
            0 => unreachable!(), //调用此函数说明至少错了一次
            1 => Duration::from_secs(5),
            2 => Duration::from_secs(30),
            3 => Duration::from_mins(1),
            _ => return None, // 当链路状态返回无的时候，链路状态表drop它
        };
        let link = Arc::downgrade(&self);
        Some(ResumeTask::new(
            delay,
            Box::new(move || {
                if let Some(link) = link.upgrade() {
                    link.reset();
                }
            }),
        ))
    }
}
