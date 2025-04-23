use crate::addr::ScopedAddr;
use netif::{Interface, Up};
use std::net::IpAddr;

pub struct NicView {
    iter: Option<Up>,
}

impl Iterator for NicView {
    type Item = ScopedAddr;

    fn next(&mut self) -> Option<Self::Item> {
        let ifaces = self.iter.as_mut()?;
        loop {
            let Interface {
                address, scope_id, ..
            } = ifaces.next()?;
            let item = match address {
                IpAddr::V6(addr) if addr.is_unicast_link_local() => {
                    scope_id.map(|scope| ScopedAddr::Lan { addr, scope })
                }
                IpAddr::V6(addr) if addr.is_unicast_global() => Some(ScopedAddr::Wan(addr)),
                _ => None,
            };
            if let Some(item) = item {
                return Some(item);
            }
        }
    }
}

impl Default for NicView {
    fn default() -> Self {
        Self {
            iter: netif::up().ok(),
        }
    }
}
