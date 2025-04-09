use super::HandshakeState;
use crate::utils::{EndPoint, Uid};
use bincode::{Decode, Encode};

pub type HostId = Uid;

#[derive(Debug, Clone, Encode, Decode)]
pub enum Msg {
    /// 发明报文用于构建链路状态表，这里包含的是对方的HostId和地址
    Discovery { host_id: HostId, remote: EndPoint },
    Auth {
        host_id: HostId,
        state: HandshakeState,
    },
    /// 当 seq 为 0 时，表示的是文件的基本信息
    /// 随后才是文件内容
    Transfer {
        host_id: HostId,
        task_id: HostId,
        seq: u64,
    },
    // todo CheckSum 信息
}

impl<'a> Msg {
    pub fn host_id(&'a self) -> &'a HostId {
        use Msg::*;
        match self {
            Discovery { host_id, .. } | Auth { host_id, .. } | Transfer { host_id, .. } => host_id,
        }
    }
}
