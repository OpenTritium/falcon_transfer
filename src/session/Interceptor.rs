use crate::inbound::Handshake;
use crate::inbound::Msg;
use crate::link::Event;
use crate::link::Uid;
use bytes::BytesMut;
use tokio::{sync::mpsc, task::AbortHandle};

use super::session;
use super::set_exchange_or_full;
use super::set_last_full;
use super::{session_table, set_hello};

struct Interceptor {
    abort: AbortHandle,
}

impl Interceptor {
    // 这里好像就要注入 outbound 了
    pub fn run(
        mut up_rx: mpsc::Receiver<Event>,
        out: mpsc::UnboundedSender<Msg>,
    ) -> (Self, mpsc::Receiver<Event>) {
        let (down_tx, down_rx) = mpsc::channel::<Event>(1024);
        let buf = BytesMut::with_capacity(u32::MAX as usize);
        let abort = tokio::spawn(async move {
            while let Some(event) = up_rx.recv().await {
                match event {
                    Event::Auth { host, state: event } => match *event {
                        //-> Exchange(e,ee)
                        Handshake::Hello => {
                            let state = set_hello(host.clone(), buf.clone()).unwrap();
                            // todo 记得替换成自己的uid
                            let msg = Msg::auth(state, host);
                            out.send(msg).unwrap();
                        }
                        // <- Exchange(e,ee,s,es) then -> Full(s,es) and set full
                        // <- Exchange(e,ee) and then -> Exchange(e,ee,s,es)
                        Handshake::Exchange(payload) => {
                            let state =
                                set_exchange_or_full(host.clone(), payload, buf.clone()).unwrap();
                            let msg = Msg::auth(state, host);
                            out.send(msg).unwrap();
                        }
                        // <- Full(s,es) and set full
                        Handshake::Full(payload) => {
                            set_last_full(host, payload, buf.clone()).unwrap();
                        }
                    },
                    event => down_tx.send(event).await.unwrap(),
                }
            }
        })
        .abort_handle();
        (Self { abort }, down_rx)
    }
}
