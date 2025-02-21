use dashmap::DashMap;
use indexmap::{indexset, IndexSet};
use rand::Rng;
use std::sync::{atomic::Ordering, Arc};
use thiserror::Error;
use tokio::sync::mpsc::Sender;

use crate::{
    endpoint::EndPoint,
    link_recovery_scheduler::{RecoveryScheduler, RecoveryTask},
    link_state::{Fade, LinkError, LinkState, LinkStateFlag},
    uid::Uid,
};

#[derive(Debug, Error)]
enum RecoveryTaskError {
    #[error("该 linkstate 已经被其他线程移除了")]
    RemovedByOtherThread,
    #[error(transparent)]
    TaskSendError(#[from] tokio::sync::mpsc::error::SendError<()>),
}
pub struct AssignedLink {
    pub local: EndPoint,
    pub remote: EndPoint,
    pub solution: Box<dyn FnOnce() -> RecoveryTaskError + Send + 'static>,
}

pub struct Bond {
    pub links: IndexSet<Arc<LinkState>>,
    flag: LinkStateFlag,
}

impl Bond {
    fn new(local: EndPoint, remote: EndPoint) -> Self {
        Self {
            links: indexset! {Arc::new(LinkState::new(local, remote, 0))},
            flag: LinkStateFlag::DISCOVED,
        }
    }
    // 仅当不存在时才构造linkstate
    pub fn add_link(&mut self, local: EndPoint, remote: EndPoint) {
        // 先检查是否存在相同 local/remote 的 LinkState
        if self
            .links
            .iter()
            .any(|link| link.addr_local == local && link.addr_remote == remote)
        {
            self.links
                .insert(Arc::new(LinkState::new(local, remote, 0)));
        }
    }
}

pub struct LinkStateTable {
    links: Arc<DashMap<Uid, Bond>>,
    scheduler: RecoveryScheduler,
    delay_task_sender: Sender<RecoveryTask>,
}

impl LinkStateTable {
    pub fn new() -> Self {
        let (scheduler, delay_task_sender) = RecoveryScheduler::run();
        LinkStateTable {
            links: Arc::new(DashMap::new()),
            scheduler,
            delay_task_sender,
        }
    }
    // 仅仅在不存在时才插入
    pub fn add_new_link(&self, uid: Uid, local: EndPoint, remote: EndPoint) {
        self.links
            .entry(uid)
            .and_modify(|bond| {
                bond.add_link(local, remote);
            })
            .or_insert_with(|| Bond::new(local, remote));
    }
    //metric 加权
    // todo 重写
    /// 如果返回的链路不能用，那就调用solution，然后再重新申请一条
    pub fn assign(&self, uid: &Uid) -> Result<AssignedLink, LinkError> {
        let bond = match self.links.get_mut(uid) {
            Some(bond) => bond,
            None => return Err(LinkError::BondNotFound),
        };

        // 优化点2：预分配候选集内存
        let mut candidates = Vec::with_capacity(bond.links.len());
        let mut total_weight = 0u64;

        // 单次遍历完成过滤和权重计算
        for link in &bond.links {
            if link.is_healthy.load(Ordering::Relaxed) {
                let weight = link.weight();
                candidates.push(link);
                total_weight = total_weight.saturating_add(weight);
            }
        }

        // 优化点3：提前处理无候选情况
        if candidates.is_empty() || total_weight == 0 {
            return Err(LinkError::LinksNotFound);
        }

        // 优化点4：使用别名法加速随机选择
        let selected = {
            let mut rng = rand::rng();
            rng.random_range(0..total_weight)
        };

        // 使用二分查找优化权重选择 (O(log n))
        let prefix_weights: Vec<u64> = candidates
            .iter()
            .scan(0u64, |acc, link| {
                *acc += link.weight();
                Some(*acc)
            })
            .collect();

        let selected_index = prefix_weights
            .binary_search_by(|probe| probe.cmp(&selected))
            .unwrap_or_else(|i| i);

        let selected_link = candidates[selected_index];

        // 结构解构模式匹配优化
        let &LinkState {
            addr_local,
            addr_remote,
            ..
        } = selected_link;
        // 以分配时间为准
        selected_link.update_usage();

        // 让闭包持有强引用
        let solution = {
            let uid = uid.clone();
            let links = self.links.clone();
            let sender = self.delay_task_sender.clone();
             //  最重要的引用保存在表中，这里也会持有一份，此函数调用之后返回的结果不包含强引用
            // 很显然它可能会被很多线程同时调用，因为可能会派发相同的链路
            Box::new(move || {

                // 情况1: 需要延迟恢复
                if let Some(task) = Fade::delay(selected_link.clone()) {
                    sender.try_send(task)?;
                    Ok(())
                }
                // 情况2: 需要立即移除
                else {
                    let need_remove = links
                        .entry(uid.clone())
                        .and_modify(|mut bond| {
                            bond.links.swap_remove(&selected_link);
                        })
                        .map(|bond| bond.links.is_empty())
                        .unwrap_or(false);

                    if need_remove {
                        links.remove(uid);
                    }
                    Ok(())
                }
            })
        };

        Ok(AssignedLink {
            local: addr_local,
            remote: addr_remote,
            solution,
        })
    }
}
