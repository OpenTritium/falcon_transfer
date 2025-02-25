use AddrConvertError::*;
use ScopedAddr::*;
use serde::{Deserialize, Serialize};
use std::{fmt::Display, net::Ipv6Addr};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AddrConvertError {
    #[error("Address is not a link-local unicast address")]
    NotLinkLocal,
    #[error("Address is not a global unicast address")]
    NotGlobal,
}

pub type ScopeId = u32;

#[derive(Debug, Copy, Clone, Serialize, Deserialize, PartialEq, Hash, Eq)]
pub enum ScopedAddr {
    Lan { addr: Ipv6Addr, scope: ScopeId },
    Wan(Ipv6Addr),
}

impl Display for ScopedAddr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Lan { addr, scope } => write!(f, "{}%{}", addr, scope),
            Wan(addr) => write!(f, "{}", addr),
        }
    }
}

impl ScopedAddr {
    pub fn is_lan(&self) -> bool {
        !self.is_wan()
    }
    pub fn is_wan(&self) -> bool {
        let Wan(_) = self else { return false };
        true
    }
}

type AddrWithScope = (Ipv6Addr, ScopeId);

impl TryFrom<(Ipv6Addr, ScopeId)> for ScopedAddr {
    type Error = AddrConvertError;

    fn try_from((addr, scope): AddrWithScope) -> Result<Self, Self::Error> {
        if addr.is_unicast_link_local() {
            return Ok(Lan { addr, scope });
        }
        return Err(NotLinkLocal);
    }
}

impl TryFrom<Ipv6Addr> for ScopedAddr {
    type Error = AddrConvertError;

    fn try_from(addr: Ipv6Addr) -> Result<Self, Self::Error> {
        if addr.is_unicast_global() {
            return Ok(Wan(addr));
        }
        return Err(NotGlobal);
    }
}

impl From<ScopedAddr> for Ipv6Addr {
    fn from(scoped_addr: ScopedAddr) -> Self {
        match scoped_addr {
            Lan { addr, .. } | Wan(addr) => addr,
        }
    }
}

impl From<ScopedAddr> for std::net::IpAddr {
    fn from(scoped_addr: ScopedAddr) -> Self {
        Ipv6Addr::from(scoped_addr).into()
    }
}
