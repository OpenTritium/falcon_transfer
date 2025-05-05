use crate::inbound::{Handshake, HostId};
use anyhow::{Result, anyhow};
use bytes::{Bytes, BytesMut};
use dashmap::DashMap;
use std::sync::OnceLock;
enum Session {
    Initiator(snow::HandshakeState),
    Responder(snow::HandshakeState),
    Transport(snow::TransportState),
}

pub fn session_table() -> &'static DashMap<HostId, Session> {
    static SESSION_TABLE: OnceLock<DashMap<HostId, Session>> = OnceLock::new();
    SESSION_TABLE.get_or_init(DashMap::new)
}

/// 发现对方以后，先手进行 hello，此操作会操作会话表和链路状态表
///
/// 记得操作链路状态表
/// 保证原子性
pub fn set_hello(host: HostId, buf: BytesMut) -> Result<Handshake> {
    let st = session_table();
    if st.contains_key(&host) {
        return Err(anyhow!("current session has already exists"));
    }
    // todo 需要注意潜在的key状态不一致，当然只存在于并发中
    let mut session = Session::new_initiator();
    let payload = session.hello(buf)?;
    st.insert(host, session);
    Ok(Handshake::Exchange(payload.to_vec()))
}

// 接受者还需要一步进入full,发起者会直接进入full
pub fn set_exchange_or_full(host: HostId, msg: Vec<u8>, buf: BytesMut) -> Result<Handshake> {
    let st = session_table();
    let result = if let Some((host, mut session)) = st.remove(&host) {
        let payload = session.exchange(msg, buf)?;
        let session = session.full()?;
        st.insert(host, session);
        Handshake::Full(payload.to_vec())
    } else {
        let mut session = Session::new_responder();
        let payload = session.exchange(msg, buf)?;
        st.insert(host, session);
        Handshake::Exchange(payload.to_vec())
    };
    Ok(result)
}

pub fn set_last_full(host: HostId, msg: Vec<u8>, buf: BytesMut) -> Result<()> {
    let st = session_table();
    if let Some((host, session)) = st.remove(&host) {
        let session = session.full_with_msg(msg, buf)?;
        st.insert(host, session);
        return Ok(());
    };
    Err(anyhow!("session not found"))
}

const PATTERN: &str = "Noise_XX_25519_AESGCM_BLAKE2b";

impl Session {
    fn new_initiator() -> Self {
        Session::Initiator(
            snow::Builder::new(PATTERN.parse().unwrap())
                .local_private_key(b"123")
                .build_initiator()
                .unwrap(),
        )
    }

    fn new_responder() -> Self {
        Session::Initiator(
            snow::Builder::new(PATTERN.parse().unwrap())
                .local_private_key(b"321")
                .build_responder()
                .unwrap(),
        )
    }

    pub fn initiator_mut(&mut self) -> Result<&mut snow::HandshakeState> {
        match self {
            Session::Initiator(s) => Ok(s),
            Session::Responder(_) | Session::Transport(_) => Err(anyhow!("not initiator")),
        }
    }

    pub fn responder_mut(&mut self) -> Result<&mut snow::HandshakeState> {
        match self {
            Session::Responder(s) => Ok(s),
            Session::Initiator(_) | Session::Transport(_) => Err(anyhow!("not responder")),
        }
    }

    /// 语义是向远方发起握手
    /// 通常由gui事件发起
    pub fn hello(&mut self, mut buf: BytesMut) -> Result<Bytes> {
        if !self.is_initialtor() {
            return Err(anyhow!("not initiator"));
        }
        let state = self.initiator_mut()?;
        // -> e,ee
        let sz = state.write_message(&[], &mut buf)?;
        let payload = buf.split_to(sz).freeze();
        Ok(payload)
    }

    /// exchange key mainly
    pub fn exchange(&mut self, msg: Vec<u8>, mut buf: BytesMut) -> Result<Bytes> {
        match self {
            Session::Initiator(state) => {
                // <- e,ee,s,es
                state.read_message(&msg, &mut buf)?;
                // -> s,es
                let sz = state.write_message(&[], &mut buf)?;
                let payload = buf.split_to(sz).freeze();
                Ok(payload)
            }
            Session::Responder(state) => {
                // <- e,ee
                state.read_message(&msg, &mut buf)?;
                // -> e,ee,s,es
                let sz = state.write_message(&[], &mut buf)?;
                let payload = buf.split_to(sz).freeze();
                Ok(payload)
            }
            Session::Transport(_) => Err(anyhow!(
                "Incorrect use of transport session during exchange"
            )),
        }
    }

    // into transport mode
    pub fn full_with_msg(self, msg: Vec<u8>, mut buf: BytesMut) -> Result<Self> {
        use Session::*;
        match self {
            Responder(mut state) => {
                // <- s,es
                state.read_message(&msg, &mut buf)?;
                let session = Session::Transport(state.into_transport_mode()?);
                Ok(session)
            }
            Initiator(_) => Err(anyhow!("not responder, no need msg to full")),
            Transport(_) => Err(anyhow!("alread handshaked")),
        }
    }

    pub fn full(self) -> Result<Self> {
        use Session::*;
        match self {
            Initiator(state) => {
                let session = Session::Transport(state.into_transport_mode()?);
                Ok(session)
            }
            Responder(_) => Err(anyhow!("not initiator, need msg to full")),
            Transport(_) => Err(anyhow!("alread handshaked")),
        }
    }

    pub fn is_initialtor(&self) -> bool {
        match self {
            Session::Initiator(_) => true,
            Session::Responder(_) | Session::Transport(_) => false,
        }
    }

    pub fn is_responder(&self) -> bool {
        match self {
            Session::Initiator(_) | Session::Transport(_) => false,
            Session::Responder(_) => true,
        }
    }

    pub fn is_transport(&self) -> bool {
        match self {
            Session::Initiator(_) | Session::Responder(_) => false,
            Session::Transport(_) => true,
        }
    }
}
