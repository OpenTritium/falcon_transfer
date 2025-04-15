use super::MsgSinkMap;
use crate::{
    link::LinkStateTable,
    utils::{HostId, Msg},
};
use anyhow::Result;
use anyhow::anyhow;
use futures::SinkExt;
use std::sync::Arc;

struct Outbound {
    links: Arc<LinkStateTable>,
    inner: MsgSinkMap, // Fields and methods for the Outbound struct
}

impl Outbound {
    pub fn new(links: Arc<LinkStateTable>, inner: MsgSinkMap) -> Self {
        Self { links, inner }
    }

    pub async fn send(&mut self, target: &HostId, msg: Msg) -> Result<()> {
        let link = self.links.assign(target).unwrap();
        let remote = link.remote;
        let Some(sink) = self.inner.get_mut(&remote) else {
            return Err(anyhow!("No sink found for address: {}", remote));
        };
        sink.send((msg, remote.into())).await?; //todo feed and flush
        Ok(())
    }
}
