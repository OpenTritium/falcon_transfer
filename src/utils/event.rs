use crate::utils::Msg;
use super::{EndPoint, HandshakeState, Uid};

// 除了发现报文需要源地址与目标地址外，其他报文只需要uid就可以查表到可达链路
#[derive(Debug)]
pub enum Event {
    /// 事件处理器需要对方地址，对方主机，和本机入口，才能建立链路状态表
    Discovery {
        remote: EndPoint,
        host_id: Uid,
        local: EndPoint,
    },
    /// 后续的事件都是基于该链路已经发现的假设
    Auth {
        host_id: Uid,
        state: HandshakeState,
    },
    /// 你需要看看msg那边的注释
    Transfer {
        host_id: Uid,
        task_id: Uid,
        seq: u64,
    },
}

/// 这里的地址其实是本地入口，不对discovery 特殊处理是因为代码能少写点
impl From<(Msg, &EndPoint)> for Event {
    fn from(parcel: (Msg, &EndPoint)) -> Self {
        use Msg::*;
        match parcel {
            (Discovery { host_id, remote: src }, local) => Event::Discovery {
                remote: src,
                host_id,
                local: *local,
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