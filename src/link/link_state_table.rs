use crate::link::assigned::AssignedLink;
use crate::link::bond::Bond;
use crate::link::{ResumeScheduler, ResumeTask};
use crate::{
    link::link_state::{LinkError, LinkState},
    utils::EndPoint,
    utils::Uid,
};
use dashmap::DashMap;
use rand::Rng;
use std::sync::{Arc, atomic::Ordering};
use tokio::sync::mpsc::Sender;

pub struct LinkStateTable {
    links: Arc<DashMap<Uid, Bond>>,
    scheduler: ResumeScheduler,
    delay_task_sender: Sender<ResumeTask>,
}

impl LinkStateTable {
    pub fn new() -> Self {
        let (scheduler, delay_task_sender) = ResumeScheduler::run();
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
                bond.update(local, remote);
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

        let (candidates, total_weight) = bond
            .links
            .iter()
            .filter(|link| link.is_healthy.load(Ordering::Relaxed))
            .fold(
                (Vec::new(), 0u64),
                |(mut candidates, total_weight), link| {
                    candidates.push(link);
                    (candidates, total_weight.saturating_add(link.weight()))
                },
            );
        // 提前处理无候选情况
        if candidates.is_empty() || total_weight == 0 {
            return Err(LinkError::LinksNotFound);
        }
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
        let selected_link = candidates[selected_index].clone();
        let LinkState {
            addr_local,
            addr_remote,
            ..
        } = *selected_link;
        // 以分配时间为准
        selected_link.update_usage();
        // todo 让闭包持有弱引用，延迟队列持有弱引用
        let solve = {
            let uid = uid.clone();
            let links = self.links.clone();
            let delay_task_sender = self.delay_task_sender.clone();
            //  最重要的引用保存在表中，这里也会持有一份，此函数调用之后返回的结果不包含强引用
            // 很显然它可能会被很多线程同时调用，因为可能会派发相同的链路
            Box::new(move || {
                if let Some(task) = selected_link.clone().delay() {
                    delay_task_sender.try_send(task)?;
                    Ok(())
                }
                // 返回none代表没必要延迟了
                else {
                    let Some(mut bond) = links.get_mut(&uid) else {
                        return Ok(());
                    };
                    bond.links.swap_remove(&selected_link);
                    if bond.links.is_empty() {
                        links.remove(&uid);
                    };
                    Ok(())
                }
            })
        };

        Ok(AssignedLink {
            local: addr_local,
            remote: addr_remote,
            solve,
        })
    }
}
