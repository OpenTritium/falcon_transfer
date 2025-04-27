use super::scoped_addr::{StdIpv6Addr, ScopeId};
use std::{net::AddrParseError, num::ParseIntError};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum DomainError {
    #[error("Address {addr}%{scope} is not a link-local unicast address")]
    NotLinkLocalAddr { addr: StdIpv6Addr, scope: ScopeId },
    #[error("Address {0} is not a global unicast address")]
    NotGlobalAddr(StdIpv6Addr),
    #[error("Address {addr}%{scope} is neither a link-local nor a global unicast address")]
    UnknownAddr { addr: StdIpv6Addr, scope: ScopeId },
}

#[derive(Debug, Error)]
pub enum ParseError {
    #[error(transparent)]
    InvalidScope(#[from] ParseIntError),
    #[error(transparent)]
    InvalidIpAddr(#[from] AddrParseError),
    #[error("Failed to match endpoint with the provided regular expression: {0}")]
    FaildToMatchEndpoint(String),
}
