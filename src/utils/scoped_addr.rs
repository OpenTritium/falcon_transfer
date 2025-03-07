use serde::{Deserialize, Serialize};
use std::{fmt::Display, net::Ipv6Addr};
use thiserror::Error;
use AddrConvertError::*;
use ScopedAddr::*;

#[derive(Debug, Error)]
pub enum AddrConvertError {
    #[error("Address is not a link-local unicast address")]
    NotLinkLocal,
    #[error("Address is not a global unicast address")]
    NotGlobal,
}

pub type ScopeId = u32;

// only for unicast address
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
        Err(NotLinkLocal)
    }
}

impl TryFrom<Ipv6Addr> for ScopedAddr {
    type Error = AddrConvertError;

    fn try_from(addr: Ipv6Addr) -> Result<Self, Self::Error> {
        if addr.is_unicast_global() {
            return Ok(Wan(addr));
        }
        Err(NotGlobal)
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv6Addr;

    #[test]
    fn link_local_addr()->Result<(), AddrConvertError> {
        let addr = "fe80::80a4:abff:85bd:69b2".parse::<Ipv6Addr>()?;
        let scope = 0;
        let lan = ScopedAddr::try_from((addr, scope))?;
        assert_eq!(lan, ScopedAddr::Lan { addr, scope });
        assert_eq!(lan.is_lan(), true);
    }

    #[test]
    fn global_addr()->Result<(),AddrConvertError> {
        let addr = "240e:430:123b:79d8:cf61:9682:3589:64e6".parse::<Ipv6Addr>()?;
        let wan = ScopedAddr::try_from(addr)?;
        assert_eq!(wan, ScopedAddr::Wan(addr));
        assert_eq!(wan.is_wan(), true);
    }

    #[test]
    #[should_panic]
    fn multicast_addr()->Result<(),AddrConvertError> {
        let addr = "ff02::1".parse::<Ipv6Addr>()?;
        let scope = 1;
        let lan = ScopedAddr::try_from((addr, scope))?;
        assert_eq!(lan, ScopedAddr::Lan { addr, scope });
        assert_eq!(lan.is_lan(), false);
        assert_eq!(lan.is_wan(), false);
    }
}
