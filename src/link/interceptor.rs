use std::net::SocketAddr;

use tokio::{sync::mpsc, task::AbortHandle};
use tracing::warn;

use crate::{addr::EndPoint, inbound::Msg, link::link_state_table};

use super::Event;

struct Interceptor {
    abort: AbortHandle,
}

impl Interceptor {
    pub fn run(
        mut up_rx: mpsc::UnboundedReceiver<(Msg, SocketAddr)>,
    ) -> (Self, mpsc::Receiver<Event>) {
        let (down_tx, down_rx) = mpsc::channel::<Event>(1024);
        let abort = tokio::spawn(async move {
            while let Some((msg, local)) = up_rx.recv().await {
                let SocketAddr::V6(local) = local else {
                    warn!("only ipv6 is supported");
                    continue;
                };
                let Ok(local) = EndPoint::try_from(local) else {
                    warn!("failed to convert socket addr to endpoint");
                    continue;
                };
                if let Msg::Discovery { host, remote } = msg {
                    println!("Intercepted discovery message from {} to {}", host, remote);
                    link_state_table().update(host, &local, &remote);
                } else {
                    let event: Event = msg.into();
                    down_tx.send(event).await.unwrap();
                }
            }
        })
        .abort_handle();
        (Self { abort }, down_rx)
    }
}
