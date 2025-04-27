use crate::inbound::Handshake;
use bytes::Bytes;

// 除了发现报文需要源地址与目标地址外，其他报文只需要uid就可以查表到可达链路
#[derive(Debug)]
pub enum NetworkEvent {
    /// 后续的事件都是基于该链路已经发现的假设
    Auth(Handshake),
    /// 你需要看看msg那边的注释
    Task(Bytes),
}
