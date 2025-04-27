use crate::addr::EndPoint;
use crate::link::Uid;
use bincode::{Decode, Encode};

pub type HostId = Uid;

#[derive(Debug, Clone, Encode, Decode)]
pub enum Msg {
    /// 发现报文用于构建链路状态表，这里包含的是对方的HostId和地址
    /// 在链路层处理
    /// 其他发现方式直接通过事件接入
    Discovery {
        host: HostId,
        remote: EndPoint,
    },
    Auth {
        host: HostId,
        state: Handshake,
    },
    /// 里面都是加密的taskevent
    Task {
        host: HostId,
        cipher: Vec<u8>,
    },
}

impl Msg {
    pub fn host(&self) -> &HostId {
        use Msg::*;
        match self {
            Discovery { host, .. } | Auth { host, .. } | Task { host, .. } => host,
        }
    }
}

#[derive(Debug, Clone, Encode, Decode)]
pub enum Handshake {
    // -> e
    Hello(Vec<u8>),
    // <- e,ee,s,es
    Exchange(Vec<u8>),
    // -> s,se
    Full(Vec<u8>),
}
