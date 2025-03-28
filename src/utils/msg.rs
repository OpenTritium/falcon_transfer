use crate::utils::{EndPoint, Uid};
use serde::{Deserialize, Serialize};

use super::HandshakeState;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Msg {
    /// 发明报文用于构建链路状态表，这里包含的是对方的uid和地址
    Discovery {
        host_id: Uid,
        remote: EndPoint,
    },
    Auth {
        host_id: Uid,
        state: HandshakeState,
    },
    /// 当 seq 为 0 时，表示的是文件的基本信息
    /// 随后才是文件内容
    Transfer {
        host_id: Uid,
        task_id: Uid,
        seq: u64,
    },
    // todo CheckSum 信息
}

impl<'a> Msg {
    pub fn host_id(&'a self) -> &'a Uid {
        use Msg::*;
        match self {
            Discovery { host_id, .. } | Auth { host_id, .. } | Transfer { host_id, .. } => host_id,
        }
    }
}
