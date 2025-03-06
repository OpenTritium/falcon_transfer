use crate::link::{LinkState, LinkStateFlag};
use crate::utils::EndPoint;
use indexmap::{indexset, IndexSet};
use std::sync::Arc;

pub struct Bond {
    pub links: IndexSet<Arc<LinkState>>,
    flag: LinkStateFlag,
}

impl Bond {
    pub fn new(local: EndPoint, remote: EndPoint) -> Self {
        Self {
            links: indexset! {Arc::new(LinkState::new(local, remote, 0))},
            flag: LinkStateFlag::DISCOVED,
        }
    }
    // 仅当不存在时才构造link_state
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
