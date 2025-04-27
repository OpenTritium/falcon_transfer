use super::{
    scoped_addr::{
        StdIpv6Addr, ScopeId,
        ScopedAddr::{self, *},
    },
    DomainError, ParseError,
};
use bincode::{Decode, Encode};
use regex::Regex;
use std::{
    fmt::Display,
    net::{SocketAddr, SocketAddrV6},
    str::FromStr,
};

pub type Port = u16;

#[derive(Debug, Clone, Copy, Encode, Decode, PartialEq, Hash, Eq)]
pub struct EndPoint {
    addr: ScopedAddr,
    port: Port,
}

impl Display for EndPoint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}]:{}", self.addr, self.port)
    }
}

impl From<EndPoint> for SocketAddrV6 {
    /// flow_info deafult to 0
    fn from(EndPoint { addr, port }: EndPoint) -> Self {
        SocketAddrV6::new(*addr.get_std(), port, 0, addr.scope_id().unwrap_or_default())
    }
}

impl FromStr for EndPoint {
    type Err = ParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let regex = Regex::new(r"\[([0-9a-fA-F:]+(?:%\d+)?)\]:(\d+)").unwrap();
        let failed_match_error = || ParseError::FaildToMatchEndpoint(s.to_string());
        let caps = regex.captures(s).ok_or_else(failed_match_error)?;
        let addr = caps
            .get(1)
            .ok_or_else(failed_match_error)?
            .as_str()
            .parse::<ScopedAddr>()?;
        let port = caps
            .get(2)
            .ok_or_else(failed_match_error)?
            .as_str()
            .parse::<Port>()?;
        Ok(EndPoint::new(addr, port))
    }
}

impl From<EndPoint> for SocketAddr {
    fn from(ep: EndPoint) -> Self {
        SocketAddrV6::from(ep).into()
    }
}

impl EndPoint {
    pub fn new(addr: ScopedAddr, port: Port) -> Self {
        Self { addr, port }
    }

    pub fn std_addr(&self) -> &StdIpv6Addr {
        self.addr.get_std()
    }

    pub fn scoped_addr(&self) -> &ScopedAddr {
        &self.addr
    }

    pub fn get_scope_id(&self) -> Option<&ScopeId> {
        if let Lan { scope, .. } = self.scoped_addr() {
            Some(scope)
        } else {
            None
        }
    }

    pub fn is_lan(&self) -> bool {
        self.addr.is_lan()
    }

    pub fn is_wan(&self) -> bool {
        !self.is_lan()
    }
}

/// format: [ipv6%iface_index]:port
/// cannot use iface_name instead of iface_index
impl TryFrom<SocketAddrV6> for EndPoint {
    type Error = DomainError;

    fn try_from(sock_addr: SocketAddrV6) -> Result<Self, Self::Error> {
        let addr: ScopedAddr = match *sock_addr.ip() {
            addr if addr.is_unicast_link_local() => (addr, sock_addr.scope_id()).try_into()?,
            addr if addr.is_unicast_global() => addr.try_into()?,
            _ => {
                return Err(DomainError::UnknownAddr {
                    addr: *sock_addr.ip(),
                    scope: sock_addr.scope_id(),
                });
            }
        };
        Ok(EndPoint::new(addr, sock_addr.port()))
    }
}

#[cfg(test)]
pub mod tests {
    use super::super::{mock_scoped_lan, mock_scoped_wan};
    use super::EndPoint;

    pub fn mock_endpoint_lan() -> EndPoint {
        EndPoint {
            addr: mock_scoped_lan(),
            port: 56,
        }
    }

    pub fn mock_endpoint_wan() -> EndPoint {
        EndPoint {
            addr: mock_scoped_wan(),
            port: 78,
        }
    }
    #[test]
    fn parse_valid() {
        vec![
            "[fe80::ddf:a82c:b441:d088%17]:8888",
            "[2001:db8::1]:80",
            "[fe80::ddf:a82c:b441:d088%7]:8888",
        ]
        .iter()
        .for_each(|&x| {
            x.parse::<EndPoint>().unwrap();
        });
    }

    #[test]
    #[should_panic]
    fn parse_with_duplicated_colon() {
        "[2001:db8::1]::80".parse::<EndPoint>().unwrap();
    }

    #[test]
    #[should_panic]
    fn parse_with_duplicated_braces() {
        "[2001:db8::1]]:80".parse::<EndPoint>().unwrap();
    }
}
