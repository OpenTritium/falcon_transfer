use super::event_loop::EventSender;
use crate::{
    iface::Inbound,
    utils::{EndPoint, Msg, NetworkEvent},
};
use anyhow::Result;
use anyhow::anyhow;
use std::net::SocketAddr;
use tokio::{
    sync::mpsc::{self, Receiver, Sender},
    task::yield_now,
};

pub struct MsgEventAdapter {
    inner: Inbound,
}

impl MsgEventAdapter {
    fn new(inbound: Inbound) -> Self {
        Self { inner: inbound }
    }

    async fn recv(&mut self) -> Result<NetworkEvent> {
        self.inner.recv().await.and_then(|(msg, addr)| {
            let SocketAddr::V6(addr) = addr else {
                return Err(anyhow!("non-IPv6"));
            };
            let addr: EndPoint = addr.try_into()?;
            let event: NetworkEvent = (msg, addr).into();
            Ok(event)
        })
    }

    pub fn run(inbound: Inbound, tx: EventSender) {
        tokio::spawn(async move {
            let mut this = Self::new(inbound);
            while let Ok(event) = this.recv().await {
                tx.try_send(event).unwrap();
                yield_now().await;
            }
        });
    }
}
