use super::{Msg, MsgStreamMux};
use futures::StreamExt;
use std::net::SocketAddr;
use tokio::{sync::mpsc, task::AbortHandle};
use tracing::{error, info};

pub struct Inbound {
    abort: AbortHandle,
}

impl Inbound {
    pub async fn receiving(mut stream: MsgStreamMux) -> (Self, mpsc::Receiver<(Msg, SocketAddr)>) {
        let (tx, rx) = mpsc::channel(10240); //需要足够大的buffer
        let abort = tokio::spawn(async move {
            while let Ok(parcel) = stream.select_next_some().await {
                tx.try_send(parcel).unwrap(); // 不要阻塞
            }
            error!("error occuered while forwarding msg from msgstreammux to mpsc");
        })
        .abort_handle();
        (Self { abort }, rx)
    }
}

impl Drop for Inbound {
    fn drop(&mut self) {
        self.abort.abort();
        info!("Inbound has been dropped");
    }
}
