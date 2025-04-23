use std::sync::Arc;
use tokio::sync::mpsc::{self, Sender, channel};
use tokio::task::AbortHandle;

use super::on_discovery;
use crate::link::LinkStateTable;
use crate::utils::{HandshakeState, NetworkEvent};

pub type EventSender = Sender<NetworkEvent>;
struct EventLoop {
    abort: AbortHandle,
}

impl EventLoop {
    fn run() -> (Self, EventSender) {
        use HandshakeState::*;
        let (tx, mut rx) = mpsc::channel(1024);
        let abort = tokio::spawn(async move {
            while let Some(event) = rx.recv().await {
                match event {
                    NetworkEvent::Discovery {
                        remote,
                        host: host_id,
                        local,
                    } => on_discovery(remote, host_id, local),
                    NetworkEvent::Auth {
                        host: host_id,
                        state,
                    } => match state {
                        Hello(v) => todo!(),
                        Exchange(v) => todo!(),
                        Full(v) => todo!(),
                    },
                    NetworkEvent::Transfer {
                        host: host_id,
                        task_id,
                        seq,
                    } => todo!(),
                }
            }
        })
        .abort_handle();
        (Self { abort }, tx)
    }
}
