use super::HostId;
use super::Msg;
use super::MsgSinkMap;
use crate::link::Event;
use crate::link::LinkStateTable;
use bytes::Bytes;
use futures::SinkExt;
use std::sync::Arc;
use tokio::{sync::mpsc, task::AbortHandle};
use tracing::info;
use tracing::warn;

pub struct Outbound {
    abort: AbortHandle,
}

enum OutboundMsg {
    Link(Msg),
    Session(Bytes), // 里面是 cipher，然后查找会话表
}

impl Outbound {
    pub fn run(
        links: Arc<LinkStateTable>,
        mut sinks: MsgSinkMap,
    ) -> (Self, mpsc::Sender<(HostId, Event)>) {
        let (tx, mut rx) = mpsc::unbounded_channel::<Msg>();
        let abort = tokio::spawn(async move {
            while let Some((host, event)) = rx.recv().await {
                let Ok(link) = links.assign(&host) else {
                    warn!("No reachable link found in the link state table for hostId: {}, network event: {:#?} will be drop",host,event);
                    continue;
                };
                let msg = match event {
                    Event::Auth(state) => Msg::Auth { host, state },
                    Event::Task(cipher) => Msg::Task { host, cipher: cipher.to_vec() },
                };
                let local = link.local();
                let remote = *link.remote();
                let Some(sink) = sinks.get_mut(local) else {
                    warn!("No sink for {local}");
                    continue;
                };
                // Use .send().await and handle errors gracefully
                if let Err(e) = sink.send((msg, remote.into())).await {
                    warn!("Failed to send message to sink for {local} -> {remote} : {:?}", e);
                }
            }
        })
        .abort_handle();
        (Self { abort }, tx)
    }
}

impl Drop for Outbound {
    fn drop(&mut self) {
        self.abort.abort();
        info!("Outbound has been aborted");
    }
}
