mod endpoint;
mod error;
mod scoped_addr;

#[cfg(test)]
pub use endpoint::tests::{mock_endpoint_lan,mock_endpoint_wan};
#[cfg(test)]
pub use scoped_addr::tests::{mock_scoped_lan,mock_scoped_wan};


pub use endpoint::{
    Port,EndPoint
};
pub use error::*;
pub use scoped_addr::*;
