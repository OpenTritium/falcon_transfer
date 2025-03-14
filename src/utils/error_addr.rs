use super::scoped_addr::{RawIpv6Addr, ScopeId};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AddrError {
    #[error("Address {addr}%{scope} is not a link-local unicast address")]
    NotLinkLocal { addr: RawIpv6Addr, scope: ScopeId },
    #[error("Address {0} is not a global unicast address")]
    NotGlobal(RawIpv6Addr),
    #[error("Address {addr}%{scope} is neither a link-local nor a global unicast address")]
    Unknown { addr: RawIpv6Addr, scope: ScopeId },
}
