use super::ResumeTask;
use crate::utils::EndPoint;
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

pub type Metric = u64;

#[derive(Debug, Error)]
pub enum LinkError {
    #[error("No healthy links available")]
    LinksNotFound,
    #[error("no way to reach this bond")]
    BondNotFound,
}

#[derive(Debug)]
pub struct LinkState {
    pub addr_local: EndPoint,
    pub addr_remote: EndPoint,
    pub metric: Metric,
    pub failure_count: AtomicU8,
    pub is_healthy: AtomicBool,
    pub last_used: AtomicU64,
}

impl Clone for LinkState {
    fn clone(&self) -> Self {
        Self {
            addr_local: self.addr_local.clone(),
            addr_remote: self.addr_remote.clone(),
            metric: self.metric,
            failure_count: AtomicU8::new(self.failure_count.load(Ordering::Acquire)),
            is_healthy: AtomicBool::new(self.is_healthy.load(Ordering::Acquire)),
            last_used: AtomicU64::new(self.last_used.load(Ordering::Relaxed)),
        }
    }
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
    pub fn new(addr_local: EndPoint, addr_remote: EndPoint, metric: Metric) -> Self {
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
        info!(
            "Link: {} -> {} recovered",
            self.addr_local, self.addr_remote
        );
    }

    #[cfg(target_os = "windows")]
    // 应当对不同系统有不一样的行为
    pub fn weight(&self) -> u64 {
        // Use inverse metric + 1 to avoid division by zero
        // Higher metric means lower weight
        1_000_000 / (self.metric + 1)
    }
    #[cfg(target_os = "linux")]
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

    pub fn deacitve(self: Arc<Self>) -> Option<ResumeTask> {
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

#[cfg(test)]
mod test {
    use super::LinkState;
    use crate::utils::EndPoint;
    use std::{
        hash::{DefaultHasher, Hash, Hasher},
        sync::{Arc, OnceLock, atomic::Ordering},
        time::{Duration, SystemTime, UNIX_EPOCH},
    };
    static DEFAULT_LINK: OnceLock<LinkState> = OnceLock::new();
    fn default_link() -> &'static LinkState {
        DEFAULT_LINK.get_or_init(|| {
            let local = "[fe80::14dc:2dd0:51e7:fa65%17]:88"
                .parse::<EndPoint>()
                .unwrap();
            let remote = "[fe80::addf:f8cf:506a:be8f%4]:88"
                .parse::<EndPoint>()
                .unwrap();
            LinkState::new(local, remote, 100)
        })
    }
    #[test]
    fn deacitve() {
        let link = Arc::new(default_link().clone());
        let task1 = link.clone().deacitve().unwrap();
        assert_eq!(link.failure_count.load(Ordering::Acquire), 1);
        assert!(!link.is_healthy.load(Ordering::Acquire));
        assert_eq!(task1.timeout, Duration::from_secs(5));

        let task2 = link.clone().deacitve().unwrap();
        assert_eq!(link.failure_count.load(Ordering::Acquire), 2);
        assert_eq!(task2.timeout, Duration::from_secs(30));

        let task3 = link.clone().deacitve().unwrap();
        assert_eq!(link.failure_count.load(Ordering::Acquire), 3);
        assert_eq!(task3.timeout, Duration::from_mins(1));

        let task4 = link.clone().deacitve();
        assert!(task4.is_none());

        let task5 = link.clone().deacitve();
        assert!(task5.is_none());
    }

    #[test]
    fn hash_and_eq() {
        let link_ref = Arc::new(default_link().clone());
        let link_ref_cloned = link_ref.clone();
        let mut hasher_ref_cloned = DefaultHasher::new();
        let mut hasher_ref = DefaultHasher::new();
        link_ref.hash(&mut hasher_ref);
        link_ref_cloned.hash(&mut hasher_ref_cloned);
        assert_eq!(hasher_ref.finish(), hasher_ref_cloned.finish());
        assert_eq!(link_ref, link_ref_cloned);

        let link_cloned = link_ref.as_ref().clone();
        assert_eq!(link_cloned, default_link().clone());
    }

    #[test]
    fn update_usage() {
        const MAX_CONSUME_TIME: u64 = 1;
        let link = Arc::new(default_link().clone());
        link.update_usage();
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        assert!(link.last_used.load(Ordering::Relaxed) <= now);
        assert!(link.last_used.load(Ordering::Relaxed) > now - MAX_CONSUME_TIME);
    }

    #[test]
    fn reset_link() {
        let link = Arc::new(default_link().clone());
        let task = link.clone().deacitve().unwrap();
        assert_eq!(link.is_healthy.load(Ordering::Acquire), false);
        assert_eq!(link.failure_count.load(Ordering::Acquire), 1);
        (task.callback)();
        assert_eq!(link.is_healthy.load(Ordering::Acquire), true);
        assert_eq!(link.failure_count.load(Ordering::Acquire), 0);
    }
}
