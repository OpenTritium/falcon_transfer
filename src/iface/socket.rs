use super::NicView;
use crate::{
    env::{MsgCodec, global_config},
    utils::{EndPoint, Msg, RawIpv6Addr},
};
use anyhow::Result;
use futures::{
    StreamExt,
    stream::{SelectAll, SplitSink, SplitStream},
};
use std::{collections::HashMap, net::SocketAddr};
use tokio::net::UdpSocket;
use tokio_util::udp::UdpFramed;

/// 为所有活跃的网络接口创建 socket
/// 对于本地链路地址需要加入特定组播进行发现
/// 对于 scope 比 link_local 更广的地址则不需要加入组播
async fn create_socket(addr: &EndPoint) -> Result<UdpSocket> {
    let multi_addr = RawIpv6Addr::from_segments([0xFF12, 0, 0, 0, 0, 0, 0, 1]);
    let sock = UdpSocket::bind(SocketAddr::from(*addr)).await?;
    if let Some(scope_id) = addr.get_scope_id() {
        sock.join_multicast_v6(&multi_addr, scope_id)?;
        sock.set_multicast_loop_v6(false)?;
    }
    Ok(sock)
}

pub type MsgSink = SplitSink<UdpFramed<MsgCodec>, (Msg, SocketAddr)>;
pub type MsgStream = SplitStream<UdpFramed<MsgCodec>>;
pub type MsgSinkMap = HashMap<EndPoint, MsgSink>;
pub type MsgStreamMux = SelectAll<MsgStream>;

pub async fn split_group() -> Result<(MsgSinkMap, MsgStreamMux)> {
    let rsts =
        futures::future::try_join_all(NicView::default().map(async move |iface| -> Result<_> {
            let addr = EndPoint::new(iface, global_config().protocol_port);
            let sock = create_socket(&addr).await?;
            Ok((addr, UdpFramed::new(sock, MsgCodec).split()))
        }))
        .await?;
    // 分离sink和stream到不同集合
    let mut sinks = HashMap::with_capacity(rsts.len());
    let mut streams = SelectAll::new();
    for (addr, (sink, stream)) in rsts {
        sinks.insert(addr, sink);
        streams.push(stream);
    }
    Ok((sinks, streams))
}
// 遍历所有网络接口，然后创建socket
// 将socket分离为 sink 和 stream
// 将sink放在map中，然后通过dispatch分流
// 将stream 直接聚合到同一个消息通道
