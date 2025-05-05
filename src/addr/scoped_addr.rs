use super::error::DomainError;
use ScopedAddr::*;
use bincode::{Decode, Encode};
use std::{fmt::Display, net::Ipv6Addr, str::FromStr};

pub type StdIpv6Addr = std::net::Ipv6Addr;
pub type ScopeId = u32;

#[derive(Debug, Copy, Clone, Encode, Decode, PartialEq, Hash, Eq)]
/// only for unicast address
pub enum ScopedAddr {
    Lan { addr: StdIpv6Addr, scope: ScopeId },
    Wan(StdIpv6Addr),
}

impl Display for ScopedAddr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Lan { addr, scope } => write!(f, "{}%{}", addr, scope),
            Wan(addr) => write!(f, "{}", addr),
        }
    }
}

impl FromStr for ScopedAddr {
    type Err = super::ParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.contains("%") {
            let mut iter = s.split("%");
            let ss = iter.next_chunk::<2>().unwrap();
            let addr = Ipv6Addr::from_str(ss[0])?;
            let scope = ScopeId::from_str(ss[1])?;
            Ok(Lan { addr, scope })
        } else {
            Ok(Wan(Ipv6Addr::from_str(s)?))
        }
    }
}

impl ScopedAddr {
    pub fn is_lan(&self) -> bool {
        !self.is_wan()
    }

    pub fn is_wan(&self) -> bool {
        if let Wan(_) = self { true } else { false }
    }

    pub fn get_std(&self) -> &StdIpv6Addr {
        match self {
            Lan { addr, .. } | Wan(addr) => addr,
        }
    }

    pub fn scope_id(&self) -> Option<ScopeId> {
        match self {
            Lan { scope, .. } => Some(*scope),
            Wan(_) => None,
        }
    }
}

impl TryFrom<(StdIpv6Addr, ScopeId)> for ScopedAddr {
    type Error = DomainError;

    fn try_from((addr, scope): (StdIpv6Addr, ScopeId)) -> Result<Self, Self::Error> {
        if addr.is_unicast_link_local() {
            return Ok(Lan { addr, scope });
        }
        Err(DomainError::NotLinkLocalAddr { addr, scope })
    }
}

impl TryFrom<StdIpv6Addr> for ScopedAddr {
    type Error = DomainError;

    fn try_from(addr: StdIpv6Addr) -> Result<Self, Self::Error> {
        if addr.is_unicast_global() {
            return Ok(Wan(addr));
        }
        Err(DomainError::NotGlobalAddr(addr))
    }
}

impl From<ScopedAddr> for StdIpv6Addr {
    fn from(scoped_addr: ScopedAddr) -> Self {
        match scoped_addr {
            Lan { addr, .. } | Wan(addr) => addr,
        }
    }
}

impl From<ScopedAddr> for std::net::IpAddr {
    fn from(scoped_addr: ScopedAddr) -> Self {
        StdIpv6Addr::from(scoped_addr).into()
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use anyhow::Result;
    use rand::Rng;

    pub fn mock_scoped_lan() -> ScopedAddr {
        let mut rng = rand::rng();
        let p0: u16 = rng.random_range(0..=0xFFFF);
        let p1: u16 = rng.random_range(0..=0xFFFF);
        let p2: u16 = rng.random_range(0..=0xFFFF);
        let p3: u16 = rng.random_range(0..=0xFFFF);
        let scope: ScopeId = rng.random_range(0..=0xFFFFFFFF);
        let addr = StdIpv6Addr::new(0xFE80, 0, 0, 0, p0, p1, p2, p3);
        (addr, scope).try_into().unwrap()
    }

    pub fn mock_scoped_wan() -> ScopedAddr {
        let mut rng = rand::rng();
        let p0: u16 = rng.random_range(0..=0xFFFF);
        let p1: u16 = rng.random_range(0..=0xFFFF);
        let p2: u16 = rng.random_range(0..=0xFFFF);
        let p3: u16 = rng.random_range(0..=0xFFFF);
        let p4: u16 = rng.random_range(0..=0xFFFF);
        let p5: u16 = rng.random_range(0..=0xFFFF);
        let p6: u16 = rng.random_range(0..=0xFFFF);
        let addr = StdIpv6Addr::new(0x240e, p0, p1, p2, p3, p4, p5, p6);
        addr.try_into().unwrap()
    }

    const LAN_IP: &str = "fe80::ddf:a82c:b441:d088";
    const WAN_IP: &str = "240e:430:123b:79d8:cf61:9682:3589:64e6";
    #[test]
    fn valid_unicast_link_local() -> Result<(), DomainError> {
        let addr = LAN_IP.parse::<StdIpv6Addr>().unwrap();
        let scope = 0;
        let lan = ScopedAddr::try_from((addr, scope))?;
        assert_eq!(lan, ScopedAddr::Lan { addr, scope });
        assert_eq!(lan.is_lan(), true);
        Ok(())
    }

    #[test]
    fn valid_unicast_global() -> Result<(), DomainError> {
        let addr = WAN_IP.parse::<StdIpv6Addr>().unwrap();
        let wan = ScopedAddr::try_from(addr)?;
        assert_eq!(wan, ScopedAddr::Wan(addr));
        assert_eq!(wan.is_wan(), true);
        Ok(())
    }

    #[test]
    #[should_panic]
    fn ula_into_global() {
        let addr = "FC00:0:0:0:1:2:3:4".parse::<StdIpv6Addr>().unwrap();
        ScopedAddr::try_from(addr).unwrap();
    }

    #[test]
    #[should_panic]
    fn ula_into_link_local() {
        let addr = "FC00:0:0:0:1:2:3:4".parse::<StdIpv6Addr>().unwrap();
        let scope = 1;
        ScopedAddr::try_from((addr, scope)).unwrap();
    }

    #[test]
    #[should_panic]
    fn global_multicast_into_unicast() {
        let addr = "FF0E::1".parse::<StdIpv6Addr>().unwrap();
        ScopedAddr::try_from(addr).unwrap();
    }

    #[test]
    #[should_panic]
    fn link_local_multicast_into_unicast() {
        let scope = 1;
        let addr = "FF02::1".parse::<StdIpv6Addr>().unwrap();
        ScopedAddr::try_from((addr, scope)).unwrap();
    }

    #[test]
    #[should_panic]
    fn global_into_link_local() {
        let scope = 3;
        let addr = WAN_IP.parse::<StdIpv6Addr>().unwrap();
        ScopedAddr::try_from((addr, scope)).unwrap();
    }

    #[test]
    #[should_panic]
    fn link_local_into_global() {
        let addr = LAN_IP.parse::<StdIpv6Addr>().unwrap();
        ScopedAddr::try_from(addr).unwrap();
    }

    #[test]
    fn parse_lan_addr() -> Result<()> {
        let addr = ScopedAddr::from_str(&format!("{LAN_IP}%17"))?;
        let expected = (LAN_IP.parse()?, 17).try_into()?;
        assert_eq!(addr, expected);
        Ok(())
    }

    #[test]
    fn parse_wan_addr() -> Result<()> {
        let addr = str::parse::<ScopedAddr>(WAN_IP)?;
        let expected: ScopedAddr = WAN_IP.parse::<StdIpv6Addr>()?.try_into()?;
        assert_eq!(addr, expected);
        Ok(())
    }

    #[test]
    #[should_panic]
    fn parse_invalid_str() {
        ScopedAddr::from_str(&format!("{LAN_IP}%%17")).unwrap();
    }
}
