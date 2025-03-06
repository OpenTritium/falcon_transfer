use futures::StreamExt;
use std::sync::Arc;
use tokio::spawn;

use crate::{
    agent::{EventReceiver, MsgSender},
    link::LinkStateTable,
};

struct Handler {
    links: Arc<LinkStateTable>,
    msg_sender: MsgSender,
}

impl Handler {
    fn run(
        links: Arc<LinkStateTable>,
        msg_sender: MsgSender,
        event_receiver: EventReceiver,
    ) -> Self {
        // 启动一个事件处理线程，你觉得使用rayon，还是tokio？
        // 不停从通道取出事件然后处理，要求有并发能力
        spawn({
            let links = links.clone();
            async move {
                futures::stream::unfold(event_receiver, async |mut rx| {
                    rx.recv().await.map(|event| (event, rx))
                })
                    .for_each_concurrent(8, async |event| {
                        match event {
                            crate::msg::Event::Discovery {
                                remote,
                                host_id,
                                local,
                            } => {
                                links.add_new_link(host_id, local, remote); //自己从netif里查开销,算了还是让它的构造函数自己查
                            }
                            crate::msg::Event::Auth { host_id, state } => match state {
                                crate::msg::Handshake::Hello(items) => {}
                                crate::msg::Handshake::Exchange(items) => todo!(),
                                crate::msg::Handshake::Full(items) => todo!(),
                            },
                            crate::msg::Event::Transfer {
                                host_id,
                                task_id,
                                seq,
                            } => todo!(),
                        }
                    })
                    .await;
            }
        });

        Self { links, msg_sender }
    }
}
