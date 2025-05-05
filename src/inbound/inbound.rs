use super::{Msg, MsgStream};
use futures::{StreamExt, stream::SelectAll};
use std::net::SocketAddr;
use tokio::{sync::mpsc, task::AbortHandle};
use tracing::{error, info};

pub struct Inbound {
    abort: AbortHandle,
}

impl Inbound {
    pub async fn receiving(
        mut stream: SelectAll<MsgStream>,
    ) -> (Self, mpsc::UnboundedReceiver<(Msg, SocketAddr)>) {
        let (tx, rx) = mpsc::unbounded_channel(); //需要足够大的buffer
        let abort = tokio::spawn(async move {
            while let Ok(parcel) = stream.select_next_some().await {
                tx.send(parcel).unwrap(); // 不要阻塞
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
