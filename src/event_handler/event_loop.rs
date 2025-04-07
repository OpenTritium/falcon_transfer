use tokio::sync::mpsc::{Sender, channel};
use tokio::{sync::mpsc::Receiver, task::AbortHandle};

use crate::utils::{Event, HandshakeState};

struct EventLoop {
    abort: AbortHandle,
}

impl EventLoop {
    fn run(mut rx: Receiver<Event>) -> Self {
        use HandshakeState::*;
        let abort = tokio::spawn(async move {
            while let Some(event) = rx.recv().await {
                match event {
                    Event::Discovery {
                        remote,
                        host_id,
                        local,
                    } => todo!(),
                    Event::Auth { host_id, state } => match state {
                        Hello(v) => todo!(),
                        Exchange(v) => todo!(),
                        Full(v) => todo!(),
                    },
                    Event::Transfer {
                        host_id,
                        task_id,
                        seq,
                    } => todo!(),
                }
            }
        })
        .abort_handle();
        Self { abort }
    }
}
