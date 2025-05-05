use std::default;
use std::path::{Component, Path, PathBuf};

use crate::link::{Event, Uid};
use crate::{addr::EndPoint, task::FileHash};
use bincode::{Decode, Encode};
use camino::Utf8PathBuf;

pub type HostId = Uid;

#[derive(Debug, Clone, Encode, Decode, PartialEq)]
pub enum Msg {
    /// 发现报文用于构建链路状态表，这里包含的是对方的HostId和地址
    /// 在链路层处理
    /// 其他发现方式直接通过事件接入
    ///
    /// 发现消息应该在链路层就被处理了
    Discovery {
        host: HostId,
        remote: EndPoint,
    },
    Auth {
        host: HostId,
        state: Handshake,
    },
    Task {
        owner: HostId,
        hash: FileHash,
        file_name: String,
        total: u64,
    },
    /// 里面都是加密的taskevent
    Transfer {
        host: HostId,
        payload: Vec<u8>,
    },
}

impl Msg {
    pub fn auth(state: Handshake, local: HostId) -> Self {
        Msg::Auth { host: local, state }
    }
}

#[derive(Debug, Clone, Encode, Decode, PartialEq, Default)]
pub enum Handshake {
    // -> e
    #[default]
    Hello,
    // <- e,ee,s,es
    Exchange(Vec<u8>),
    // -> s,se
    Full(Vec<u8>),
}
