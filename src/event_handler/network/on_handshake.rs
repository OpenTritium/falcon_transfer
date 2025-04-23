use crate::{
    iface::Outbound,
    utils::{HandshakeState, HostId, Msg},
};
use anyhow::{Context, Result, anyhow};
use bytes::BytesMut;
use snow::{Builder, HandshakeState as NoiseHandshakeState, params::NoiseParams};
use std::sync::{Arc, OnceLock};
use tokio::sync::Mutex;

// 操作会话表，变更会话状态
