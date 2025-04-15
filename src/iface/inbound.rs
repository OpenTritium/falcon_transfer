use super::MsgStreamMux;
use crate::utils::Msg;
use anyhow::Result;
use futures::StreamExt;
use std::net::SocketAddr;

struct Inbound {
    inner: MsgStreamMux,
}

impl Inbound {
    pub fn new(stream: MsgStreamMux) -> Self {
        Self { inner: stream }
    }

    pub async fn next(&mut self) -> Result<(Msg, SocketAddr)> {
        self.inner.select_next_some().await
    }
}
