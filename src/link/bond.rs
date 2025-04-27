use super::{BondStateFlag, LinkState};
use crate::addr::EndPoint;
use indexmap::{IndexSet, indexset};
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct Bond {
    pub links: IndexSet<Arc<LinkState>>,
    pub flag: BondStateFlag, // 该状态描述bond状态而非link状态
}

impl Bond {
    /// 此时bond状态必为发现
    pub fn new(local: &EndPoint, remote: &EndPoint) -> Self {
        Self {
            links: indexset! {Arc::new(LinkState::new(*local, *remote, 0))},
            flag: BondStateFlag::DISCOVED,
        }
    }

    /// 仅当不存在时才构造link_state
    /// 如果 bond 中已经存在此链路则返回 false
    pub fn update(&mut self, local: EndPoint, remote: EndPoint) -> bool {
        if self
            .links
            .iter()
            .any(|link| link.addr_local == local && link.addr_remote == remote)
        {
            return false;
        }
        // todo query metric
        self.links
            .insert(Arc::new(LinkState::new(local, remote, 0)))
    }

    // 没有remove 方法是因为bond 空了整个容器都会被移除
    // todo 实现迁移状态
}

#[cfg(test)]
mod tests {
    use super::Bond;
    use crate::addr::EndPoint;
    use anyhow::Result;

    #[test]
    fn avoid_reconstructing() -> Result<()> {
        let local = "[fe80::14dc:2dd0:51e7:fa65%17]:88".parse::<EndPoint>()?;
        let remote = "[fe80::addf:f8cf:506a:be8f%4]:88".parse::<EndPoint>()?;
        let mut bond = Bond::new(&local, &remote);
        assert!(!bond.update(local, remote));
        Ok(())
    }
}
