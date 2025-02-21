use crate::{endpoint::EndPoint, uid::Uid};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Msg {
    Discovery {
        host_id: Uid,
        addr: EndPoint,
    },
    Auth {
        host_id: Uid,
        state: Handshake,
    },
    Transfer {
        host_id: Uid,
        task_id: Uid,
        seq: u64, //seq为0时，包含的是文件基本信息
    },
}

impl<'a> Msg {
    pub fn host_id(&'a self) -> &'a Uid {
        use Msg::*;
        match self {
            Discovery { host_id, .. } | Auth { host_id, .. } | Transfer { host_id, .. } => host_id,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Handshake {
    Hello(Vec<u8>),
    Exchange(Vec<u8>),
    Full(Vec<u8>),
}

// 除了发现报文需要源地址与目标地址外，其他报文只需要uid就可以查表到可达链路
#[derive(Debug)]
pub enum Event {
    Discovery {
        remote: EndPoint,
        host_id: Uid,
        local: EndPoint,
    },
    Auth {
        host_id: Uid,
        state: Handshake,
    },
    Transfer {
        host_id: Uid,
        task_id: Uid,
        seq: u64,
    },
}

impl From<(Msg, EndPoint)> for Event {
    //  第二个地址实际上是本地传入地址，仅仅在通过发现报文构建链路状态表时才需要
    fn from(parcel: (Msg, EndPoint)) -> Self {
        use Msg::*;
        match parcel {
            (Discovery { host_id, addr: src }, dest) => Event::Discovery {
                remote: src,
                host_id,
                local: dest,
            },
            (Auth { host_id, state }, _) => Event::Auth { host_id, state },
            (
                Transfer {
                    host_id,
                    task_id,
                    seq,
                },
                _,
            ) => Event::Transfer {
                host_id,
                task_id,
                seq,
            },
        }
    }
}
