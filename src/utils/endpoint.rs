use ScopedAddr::*;
use serde::{Deserialize, Serialize};
use std::{
    fmt::Display,
    net::{SocketAddr, SocketAddrV6},
};

use super::error_addr::AddrError;
pub use super::scoped_addr::{RawIpv6Addr, ScopeId, ScopedAddr};
pub type Port = u16;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Hash, Eq)]
pub struct EndPoint {
    ip: ScopedAddr,
    port: Port,
}

impl Display for EndPoint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}]:{}", self.ip, self.port)
    }
}

// flow_info deafult to 0
impl From<EndPoint> for SocketAddrV6 {
    fn from(ep: EndPoint) -> Self {
        match ep {
            EndPoint {
                ip: Lan { addr, scope },
                port,
            } => SocketAddrV6::new(addr, port, 0, scope),
            EndPoint {
                ip: Wan(addr),
                port,
            } => SocketAddrV6::new(addr, port, 0, 0),
        }
    }
}

impl From<EndPoint> for SocketAddr {
    fn from(ep: EndPoint) -> Self {
        SocketAddrV6::from(ep).into()
    }
}

impl EndPoint {
    pub fn new(ip: ScopedAddr, port: Port) -> Self {
        Self { ip, port }
    }

    pub fn get_addr(&self) -> RawIpv6Addr {
        self.ip.get_raw()
    }

    pub fn get_scoped_addr(&self) -> ScopedAddr {
        self.ip
    }

    pub fn get_scope_id(&self) -> Option<ScopeId> {
        let Lan { scope, .. } = self.get_scoped_addr() else {
            return None;
        };
        Some(scope)
    }

    pub fn is_lan(&self) -> bool {
        self.ip.is_lan()
    }

    pub fn is_wan(&self) -> bool {
        !self.is_lan()
    }
}

impl TryFrom<SocketAddrV6> for EndPoint {
    type Error = AddrError;

    fn try_from(sock_addr: SocketAddrV6) -> Result<Self, Self::Error> {
        let addr: ScopedAddr = match *sock_addr.ip() {
            addr if addr.is_unicast_link_local() => (addr, sock_addr.scope_id()).try_into()?,
            addr if addr.is_unicast_global() => addr.try_into()?,
            _ => {
                return Err(super::error_addr::AddrError::Unknown {
                    addr: *sock_addr.ip(),
                    scope: sock_addr.scope_id(),
                });
            }
        };
        Ok(EndPoint::new(addr, sock_addr.port()))
    }
}
