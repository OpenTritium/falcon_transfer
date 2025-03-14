use super::error_addr::AddrError::{self, *};
use ScopedAddr::*;
use serde::{Deserialize, Serialize};
use std::fmt::Display;

pub type RawIpv6Addr = std::net::Ipv6Addr;
pub type ScopeId = u32;
pub type AddrWithScope = (RawIpv6Addr, ScopeId);

// only for unicast address
#[derive(Debug, Copy, Clone, Serialize, Deserialize, PartialEq, Hash, Eq)]
pub enum ScopedAddr {
    Lan { addr: RawIpv6Addr, scope: ScopeId },
    Wan(RawIpv6Addr),
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

    pub fn get_raw(&self) -> RawIpv6Addr {
        match self {
            Lan { addr, .. } | Wan(addr) => *addr,
        }
    }
}

impl TryFrom<(RawIpv6Addr, ScopeId)> for ScopedAddr {
    type Error = AddrError;

    fn try_from((addr, scope): AddrWithScope) -> Result<Self, Self::Error> {
        if addr.is_unicast_link_local() {
            return Ok(Lan { addr, scope });
        }
        Err(NotLinkLocal { addr, scope })
    }
}

impl TryFrom<RawIpv6Addr> for ScopedAddr {
    type Error = AddrError;

    fn try_from(addr: RawIpv6Addr) -> Result<Self, Self::Error> {
        if addr.is_unicast_global() {
            return Ok(Wan(addr));
        }
        Err(NotGlobal(addr))
    }
}

impl From<ScopedAddr> for RawIpv6Addr {
    fn from(scoped_addr: ScopedAddr) -> Self {
        match scoped_addr {
            Lan { addr, .. } | Wan(addr) => addr,
        }
    }
}

impl From<ScopedAddr> for std::net::IpAddr {
    fn from(scoped_addr: ScopedAddr) -> Self {
        RawIpv6Addr::from(scoped_addr).into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_unicast_link_local() -> Result<(), AddrError> {
        let addr = "fe80::80a4:abff:85bd:69b2".parse::<RawIpv6Addr>().unwrap();
        let scope = 0;
        let lan = ScopedAddr::try_from((addr, scope))?;
        assert_eq!(lan, ScopedAddr::Lan { addr, scope });
        assert_eq!(lan.is_lan(), true);
        Ok(())
    }

    #[test]
    fn valid_unicast_global() -> Result<(), AddrError> {
        let addr = "240e:430:123b:79d8:cf61:9682:3589:64e6"
            .parse::<RawIpv6Addr>()
            .unwrap();
        let wan = ScopedAddr::try_from(addr)?;
        assert_eq!(wan, ScopedAddr::Wan(addr));
        assert_eq!(wan.is_wan(), true);
        Ok(())
    }

    #[test]
    #[should_panic]
    fn ula_into_global() {
        let addr = "FC00:0:0:0:1:2:3:4".parse::<RawIpv6Addr>().unwrap();
        ScopedAddr::try_from(addr).unwrap();
    }

    #[test]
    #[should_panic]
    fn ula_into_link_local() {
        let addr = "FC00:0:0:0:1:2:3:4".parse::<RawIpv6Addr>().unwrap();
        let scope = 1;
        ScopedAddr::try_from((addr, scope)).unwrap();
    }

    #[test]
    #[should_panic]
    fn global_multicast_into_unicast() {
        let addr = "FF0E::1".parse::<RawIpv6Addr>().unwrap();
        ScopedAddr::try_from(addr).unwrap();
    }

    #[test]
    #[should_panic]
    fn link_local_multicast_into_unicast() {
        let scope = 1;
        let addr = "FF02::1".parse::<RawIpv6Addr>().unwrap();
        ScopedAddr::try_from((addr, scope)).unwrap();
    }

    #[test]
    #[should_panic]
    fn global_into_link_local() {
        let scope = 3;
        let addr = "240e:430:123b:79d8:cf61:9682:3589:64e6"
            .parse::<RawIpv6Addr>()
            .unwrap();
        ScopedAddr::try_from((addr, scope)).unwrap();
    }

    #[test]
    #[should_panic]
    fn link_local_into_global() {
        let addr = "fe80::ddf:a82c:b441:d088".parse::<RawIpv6Addr>().unwrap();
        ScopedAddr::try_from(addr).unwrap();
    }
}
