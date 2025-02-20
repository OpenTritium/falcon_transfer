use std::sync::{Arc, atomic::Ordering};

use dashmap::DashMap;
use rand::Rng;
use tokio::sync::mpsc::Sender;

use crate::{
    endpoint::EndPoint,
    link_recovery_scheduler::{RecoveryScheduler, RecoveryTask},
    link_state::{Fade, LinkError, LinkState},
    uid::Uid,
};

pub struct AssignedLink {
    pub local: EndPoint,
    pub remote: EndPoint,
    pub solution: Box<dyn FnOnce() + Send + 'static>,
}

pub struct LinkStateTable {
    links: Arc<DashMap<Uid, Vec<Arc<LinkState>>>>,
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
    pub fn add_link(&self, uid: Uid, local: EndPoint, remote: EndPoint, metric: u64) {
        let link_state = Arc::new(LinkState::new(local, remote, metric));
        self.links
            .entry(uid)
            .or_insert_with(Vec::new)
            .push(link_state);
    }
    //metric 加权
    /// 如果返回的链路不能用，那就调用solution，然后再重新申请一条
    pub fn assign(&self, uid: &Uid) -> Result<AssignedLink, LinkError> {
        // 首先通过uid选链路组
        let links = match self.links.get(uid) {
            Some(links) => links,
            None => return Err(LinkError::NoHealthyLinks), // todo 没有链路可选
        };

        // 收集其中健康链路
        let candidates: Vec<_> = links
            .iter()
            .filter(|link| link.is_healthy.load(Ordering::Relaxed))
            .collect();

        if candidates.is_empty() {
            return Err(LinkError::NoHealthyLinks); // 没有健康链路
        }

        // Calculate total weight (inverse metric sum)
        let total_weight: u64 = candidates.iter().map(|link| link.weight()).sum();

        if total_weight == 0 {
            return Err(LinkError::NoHealthyLinks); //todo ，总权重太小这是可能被产生的
        }

        // Weighted random selection
        // 这个随机器是线程局部的
        let mut selected = rand::rng().random_range(0..total_weight);
        let mut selected_index = 0;

        for (i, link) in candidates.iter().enumerate() {
            if selected < link.weight() {
                selected_index = i;
                break;
            }
            selected -= link.weight();
        }

        let link = candidates[selected_index].clone();
        let LinkState {
            addr_local,
            addr_remote,
            ..
        } = *link;

        let solution = {
            let uid = uid.clone();
            let link_state_table = self.links.clone();
            let delay_task_sender = self.delay_task_sender.clone();
            Box::new(move || {
                link.update_usage();
                // When failure occurs, schedule recovery and mark as inactive
                if let Some(task) = Fade::delay(link.clone()) {
                    delay_task_sender.try_send(task).unwrap(); // 那边应该处理起来很快
                } else {
                    // 没有再推迟的必要了，直接从表里面剔除
                    link_state_table
                        .entry(uid)
                        .and_modify(|v| v.retain(|l| !Arc::ptr_eq(l, &link)));
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
