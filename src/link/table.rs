use super::LinkResumeTaskError;
use crate::addr::EndPoint;
use crate::inbound::HostId;
use crate::link::assigned::AssignedLink;
use crate::link::bond::Bond;
use crate::link::link_state::LinkError;
use crate::link::{LinkResumeScheduler, LinkResumeTask};
use dashmap::DashMap;
use rand::Rng;
use std::sync::OnceLock;
use std::sync::{Arc, atomic::Ordering};
use tokio::sync::mpsc::Sender;

static LINK_STATE_TABLE: OnceLock<LinkStateTable> = OnceLock::new();
pub fn link_state_table() -> &'static LinkStateTable {
    LINK_STATE_TABLE.get_or_init(LinkStateTable::new)
}
pub struct LinkStateTable {
    links: Arc<DashMap<HostId, Bond>>,
    _scheduler: LinkResumeScheduler,
    delay_task_sender: Sender<LinkResumeTask>,
}

impl LinkStateTable {
    pub fn new() -> Self {
        let (scheduler, delay_task_sender) = LinkResumeScheduler::run();
        LinkStateTable {
            links: Arc::new(DashMap::new()),
            _scheduler: scheduler,
            delay_task_sender,
        }
    }
    // 仅仅在不存在时才插入
    pub fn update(&self, host_id: HostId, local: &EndPoint, remote: &EndPoint) {
        self.links
            .entry(host_id)
            .and_modify(|bond| {
                bond.update(*local, *remote);
            })
            .or_insert_with(|| Bond::new(local, remote));
    }
    //metric 加权
    // todo 重写
    /// 如果返回的链路不能用，那就调用solution，然后再重新申请一条
    pub fn assign(&self, host_id: &HostId) -> Result<AssignedLink, LinkError> {
        let bond = self
            .links
            .get(host_id)
            .ok_or(LinkError::BondNotFound)?
            .clone();
        let (candidates, total_weight) = bond
            .links
            .iter()
            .filter(|link| link.is_healthy.load(Ordering::Relaxed))
            .fold(
                (Vec::with_capacity(bond.links.len()), 0usize),
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
        let weight_distributes = candidates
            .iter()
            .scan(0usize, |acc, link| {
                *acc += link.weight();
                Some(*acc)
            })
            .collect::<Vec<usize>>();
        let selected_index = weight_distributes
            .binary_search_by(|probe| probe.cmp(&selected))
            .unwrap_or_else(|i| i);
        let selected_link = candidates[selected_index].clone();
        let (addr_local, addr_remote) = selected_link.local_remote_addr();
        // 以分配时间为准
        selected_link.update_usage();
        let solve = {
            let selected_link = Arc::downgrade(&selected_link);
            let host_id = host_id.clone();
            let links = self.links.clone();
            let delay_task_sender = self.delay_task_sender.clone();
            //  最重要的引用保存在表中，这里也会持有一份，此函数调用之后返回的结果不包含强引用
            // 很显然它可能会被很多线程同时调用，因为可能会派发相同的链路
            Box::new(move || {
                let selected_link = selected_link
                    .upgrade()
                    .ok_or(LinkResumeTaskError::LinkRefInvalid)?;
                if let Some(task) = selected_link.clone().deacitve() {
                    delay_task_sender.try_send(task)?;
                    Ok(())
                }
                // 返回none代表没必要延迟了
                // todo 持有锁可能会造成死锁
                else {
                    let need_remove = {
                        if let Some(mut entry) = links.get_mut(&host_id) {
                            entry.links.swap_remove(&selected_link);
                            entry.links.is_empty()
                        } else {
                            false
                        }
                    };
                    if need_remove {
                        links.remove(&host_id); // 此时可以安全获取锁
                    }
                    Ok(())
                }
            })
        };

        Ok(AssignedLink::new(addr_local, addr_remote, solve))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::addr::{mock_endpoint_lan, mock_endpoint_wan};
    use anyhow::Result;
    use tokio::{task::yield_now, time::Duration};

    // 测试update方法
    #[tokio::test(start_paused = true)]
    async fn update_links() -> Result<()> {
        let table = LinkStateTable::new();
        let host = HostId::random();

        // 测试新增Host
        let ep1 = mock_endpoint_lan();
        let ep2 = mock_endpoint_lan();
        table.update(host.clone(), &ep1, &ep2);

        assert!(table.links.contains_key(&host));
        let bond = table.links.get(&host).unwrap();
        assert_eq!(bond.links.len(), 1);
        assert!(
            bond.links
                .iter()
                .any(|l| l.local_remote_addr() == (ep1, ep2))
        );
        drop(bond);
        // 测试添加不同链路
        let ep3 = mock_endpoint_lan();
        table.update(host.clone(), &ep1, &ep3);

        let bond = table.links.get(&host).unwrap();
        assert_eq!(bond.links.len(), 2);

        drop(bond);
        // 测试重复添加相同链路
        table.update(host.clone(), &ep1, &ep2);
        let bond = table.links.get(&host).unwrap();
        assert_eq!(bond.links.len(), 2); // 数量不变

        Ok(())
    }

    // 测试基本分配逻辑
    #[tokio::test(start_paused = true)]
    async fn assign_basic() -> Result<()> {
        let table = LinkStateTable::new();
        let host = HostId::random();

        // 准备测试数据
        let ep_local1 = mock_endpoint_lan();
        let ep_remote1 = mock_endpoint_lan();
        let ep_remote2 = mock_endpoint_lan();

        table.update(host.clone(), &ep_local1, &ep_remote1);
        table.update(host.clone(), &ep_local1, &ep_remote2);

        // 测试正常分配
        let assigned = table.assign(&host)?;
        assert_eq!(*assigned.local(), ep_local1);
        assert!([ep_remote1, ep_remote2].contains(assigned.remote()));

        // 验证最后使用时间更新
        let bond = table.links.get(&host).unwrap();
        let link = bond
            .links
            .iter()
            .find(|l| l.addr_remote == *assigned.remote())
            .unwrap();
        let last_used = link.last_used.load(Ordering::Relaxed);
        assert!(last_used > 0);

        Ok(())
    }

    // 测试错误处理
    #[tokio::test(start_paused = true)]
    async fn assign_errors() {
        let table = LinkStateTable::new();
        let unknown_host = HostId::random();

        // 测试不存在的Host
        assert!(matches!(
            table.assign(&unknown_host),
            Err(LinkError::BondNotFound)
        ));

        // 测试所有链路不健康
        let host = HostId::random();
        let ep_local = mock_endpoint_wan();
        let ep_remote = mock_endpoint_wan();
        table.update(host.clone(), &ep_local, &ep_remote);

        let bond = table.links.get_mut(&host).unwrap();
        for link in &bond.links {
            link.is_healthy.store(false, Ordering::Release);
        }
        drop(bond);
        assert!(matches!(table.assign(&host), Err(LinkError::LinksNotFound)));
    }

    #[tokio::test(start_paused = true)]
    async fn link_recovery() -> Result<()> {
        let table = LinkStateTable::new();
        let host = HostId::random();
        let ep_local = mock_endpoint_wan();
        let ep_remote = mock_endpoint_wan();

        table.update(host.clone(), &ep_local, &ep_remote);

        // 获取并标记链路失败
        let assigned = table.assign(&host)?;
        assigned.solve()?;

        // 快进时间触发恢复
        tokio::task::yield_now().await; // 确保任务执行
        tokio::time::advance(Duration::from_secs(10)).await; // 假设原始恢复时间为5秒
        tokio::task::yield_now().await; // 确保任务执行

        let bond = table.links.get(&host).unwrap();
        let link = bond.links.first().unwrap();
        assert!(link.is_healthy.load(Ordering::Acquire));
        assert_eq!(link.failure_count.load(Ordering::Acquire), 1);
        Ok(())
    }

    #[tokio::test(start_paused = true)]
    async fn link_eviction() -> Result<()> {
        let table = LinkStateTable::new();
        let host = HostId::random();
        let ep_local = mock_endpoint_lan();
        let ep_remote = mock_endpoint_lan();

        table.update(host.clone(), &ep_local, &ep_remote);

        // 分配三次
        for _ in 0..3 {
            let assigned = table.assign(&host)?;
            assigned.solve()?; // 此操作会让链路失败状态+1
            yield_now().await;
            tokio::time::advance(Duration::from_mins(2)).await; // 快进1分钟
            yield_now().await;
        }
        // 三次之后就应该触发链路失效流程
        let a = table.assign(&host);
        assert!(a.is_ok());
        a.unwrap().solve()?;
        assert_eq!(table.links.get(&host).is_none(), true);
        let l = table.assign(&host);
        assert!(matches!(l, Err(LinkError::BondNotFound)));
        Ok(())
    }
}
