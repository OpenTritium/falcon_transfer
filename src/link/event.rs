use crate::{
    inbound::{Handshake, HostId, Msg},
    task::FileHash,
};
use bytes::Bytes;
use camino::{Utf8Component, Utf8PathBuf};

// 除了发现报文需要源地址与目标地址外，其他报文只需要uid就可以查表到可达链路
#[derive(Debug)]
pub enum Event {
    /// 后续的事件都是基于该链路已经发现的假设
    Auth {
        host: HostId,
        state: Box<Handshake>,
    },
    Task {
        owner: HostId,
        hash: FileHash,
        file_name: Utf8PathBuf,
        total: usize,
    },
    Transfer {
        host: HostId,
        payload: Bytes,
    },
}

impl From<Msg> for Event {
    #[inline(always)]
    fn from(msg: Msg) -> Self {
        let event = match msg {
            Msg::Auth { host, state } => Event::Auth {
                host,
                state: Box::new(state),
            },
            Msg::Task {
                owner,
                hash,
                file_name,
                total,
            } => Event::Task {
                owner,
                hash,
                file_name: Utf8PathBuf::from(file_name)
                    .components()
                    .last()
                    .filter(|c| matches!(c, Utf8Component::Normal(_)))
                    .iter()
                    .collect(),
                total: total as usize,
            },
            Msg::Transfer { host, payload } => Event::Transfer {
                host,
                payload: payload.into(),
            },
            _ => unreachable!("Discovery should be handled in link layer"),
        };
        event
    }
}
