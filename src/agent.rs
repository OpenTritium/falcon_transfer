use std::{borrow::Cow, sync::Arc};

use dashmap::DashMap;
use futures::{SinkExt, StreamExt, TryStreamExt};
use thiserror::Error;
use tokio::{
    spawn,
    sync::{
        Semaphore,
        mpsc::{UnboundedReceiver, UnboundedSender, error::SendError, unbounded_channel},
    },
    task::AbortHandle,
};
use tracing::{debug, error, warn};

use crate::{
    endpoint::EndPoint,
    link_state_table::LinkStateTable,
    msg::{Event, Msg},
    socket::{MsgSink, MsgSinkStreamGroup, MsgStream},
};

#[derive(Debug, Error)]
enum AgentError {
    #[error("序列化失败: {0}")]
    Serialization(#[from] bincode::Error),

    #[error("I/O错误: {0}")]
    Io(#[from] std::io::Error),

    #[error("通道关闭，未发送事件: {0:?}")]
    ChannelClosed(#[from] SendError<Event>),
}

struct Agent {
    recv_task_aborts: DashMap<EndPoint, AbortHandle>, // 一个出口对应一个
    extend_event_sender: EventSender,      // 当增加消息套接字时从这里拿到事件发送器
    send_task_abort: AbortHandle,                     //显然发送任务只有一个
    egresses: Arc<DashMap<EndPoint, MsgSink>>,
}

pub type MsgReceiver = UnboundedReceiver<Msg>;
pub type MsgSender = UnboundedSender<Msg>;
pub type EventReceiver = UnboundedReceiver<Event>;
pub type EventSender =  UnboundedSender<Event>;

impl Agent {
    // 注入链路表，因为事件处理器也会分享
    fn init(
        sockets: MsgSinkStreamGroup,
        link_state_table: Arc<LinkStateTable>,
    ) -> (Self, MsgSender, EventReceiver) {
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
                extend_event_sender: upstream,
                egresses,
            },
            upsink,
            downstream,
        )
    }

    fn run_recv(ep: EndPoint, stream: MsgStream, tx: EventSender) -> AbortHandle {
        spawn(async move {
            let ep = &ep; // 避免多次克隆

            stream
                .map(|result| match result {
                    Ok((msg, _)) => Ok((msg, *ep).into()),
                    Err(err) => {
                        warn!("[{}] Stream error: {:?}", ep, err);
                        Err(AgentError::from(err))
                    }
                })
                .inspect_ok(|event| debug!("[{}] Received event: {:?}", ep, event))
                .try_for_each(async |event| {
                    tx.send(event)?; // 自动转换错误类型
                    Ok(())
                })
                .await
                .unwrap_or_else(|err| {
                    error!("[{}] 处理失败: {}", ep, err);
                });
        })
        .abort_handle()
    }

    fn run_send(
        link_state_table: Arc<LinkStateTable>,
        egresses: Arc<DashMap<EndPoint, MsgSink>>,
        rx: MsgReceiver,
    ) -> AbortHandle {
        const CONCURRENT_TASK_COUNT: usize = 8;
        spawn(async move {
            let semaphore = Arc::new(Semaphore::new(CONCURRENT_TASK_COUNT));

            futures::stream::unfold(rx, async |mut rx| { rx.recv().await.map(|msg| (msg, rx)) })
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

impl Drop for Agent {
    fn drop(&mut self) {
        // Perform necessary cleanup here
        self.recv_task_aborts.iter().for_each(|entry| {
            entry.abort();
        });
        self.send_task_abort.abort();
    }
}
