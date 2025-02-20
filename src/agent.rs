use std::{borrow::Cow, sync::Arc};

use dashmap::DashMap;
use futures::{SinkExt, StreamExt, future::BoxFuture};
use tokio::{
    spawn, stream,
    sync::{
        Semaphore,
        mpsc::{Receiver, Sender, UnboundedReceiver, UnboundedSender, unbounded_channel},
    },
    task::AbortHandle,
};
use tracing::warn;

use crate::{
    endpoint::EndPoint,
    link_state_table::{self, LinkStateTable},
    msg::{Event, Msg},
    socket::{MsgSink, MsgSinkStreamGroup, MsgStream},
};

struct Agent {
    recv_task_aborts: DashMap<EndPoint, AbortHandle>, // 一个出口对应一个
    send_task_abort: AbortHandle,                     //显然发送任务只有一个
    egresses: Arc<DashMap<EndPoint, MsgSink>>,
}

impl Agent {
    // 注入链路表，因为事件处理器也会分享
    fn init(
        sockets: MsgSinkStreamGroup,
        link_state_table: Arc<LinkStateTable>,
    ) -> (Self, UnboundedSender<Msg>, UnboundedReceiver<Event>) {
        let (upstream, downstream) = unbounded_channel();
        let (upsink, downsink) = unbounded_channel();

        let (egresses, recv_task_aborts) = sockets
            .into_iter()
            .map(|(ep, (sink, stream))| {
                let recv_abort = Self::run_recv(ep, stream, upstream.clone());
                (ep, sink, recv_abort)
            })
            .fold(
                (DashMap::new(), DashMap::new()),
                |(egresses, recv_task_aborts), (ep, sink, abort)| {
                    egresses.insert(ep, sink);
                    recv_task_aborts.insert(ep, abort);
                    (egresses, recv_task_aborts)
                },
            );
        let egresses = Arc::new(egresses);
        let send_task_abort = Self::run_send(link_state_table, egresses.clone(), downsink);
        (
            Self {
                recv_task_aborts,
                send_task_abort,
                egresses,
            },
            upsink,
            downstream,
        )
    }

    fn run_recv(ep: EndPoint, mut stream: MsgStream, tx: UnboundedSender<Event>) -> AbortHandle {
        spawn(async move {
            while let Some(Ok((msg, _))) = stream.next().await {
                tx.send((msg, ep).into()).unwrap();
            }
            panic!("");
        })
        .abort_handle()
    }

    fn run_send(
        link_state_table: Arc<LinkStateTable>,
        egresses: Arc<DashMap<EndPoint, MsgSink>>,
        rx: UnboundedReceiver<Msg>,
    ) -> AbortHandle {
        const CONCURRENT_TASK_COUNT: usize = 8;
        spawn(async move {
            let semaphore = Arc::new(Semaphore::new(CONCURRENT_TASK_COUNT));

            futures::stream::unfold(rx, |mut rx| async { rx.recv().await.map(|msg| (msg, rx)) })
                .for_each_concurrent(CONCURRENT_TASK_COUNT, |msg| {
                    let semaphore = semaphore.clone();
                    let links = link_state_table.clone();
                    let egresses = egresses.clone();

                    async move {
                        // 存疑是不是scope后释放
                        let _permit = semaphore.acquire().await.unwrap();
                        let msg: Cow<'_, Msg> = Cow::Owned(msg);

                        const MAX_TRY_COUNT: u8 = 3;
                        for _ in 0..=MAX_TRY_COUNT {
                            let link = match links.assign(msg.host_id()) {
                                Ok(l) => l,
                                Err(e) => {
                                    warn!("Assign link failed: {:?}", e);
                                    break;
                                }
                            };
                            let send_result = match egresses.get_mut(&link.local) {
                                Some(mut sink) => {
                                    let msg = msg.clone().into_owned();
                                    sink.send((msg, link.remote.into())).await
                                }
                                None => {
                                    warn!("No sink found for {:?}", link.local);
                                    break;
                                }
                            };

                            match send_result {
                                Ok(_) => break,
                                Err(e) => {
                                    warn!("Send failed: {:?}", e);
                                    (link.solution)();
                                }
                            }
                        }
                    }
                })
                .await;
        })
        .abort_handle()
    }
}
