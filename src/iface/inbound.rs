use super::{NetworkMsg, NetworkMsgStreamMux};
use anyhow::Result;
use futures::StreamExt;
use std::net::SocketAddr;

pub struct Inbound {
    inner: NetworkMsgStreamMux,
}

impl Inbound {
    pub fn new(stream: NetworkMsgStreamMux) -> Self {
        Self { inner: stream }
    }

    pub async fn recv(&mut self) -> Result<(NetworkMsg, SocketAddr)> {
        self.inner.select_next_some().await
    }
}
