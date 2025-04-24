use super::{MsgCodec, NetworkMsg, NicView};
use crate::{
    addr::{EndPoint, RawIpv6Addr},
    config::global_config,
};
use anyhow::Result;
use futures::{
    StreamExt,
    future::try_join_all,
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
        sock.join_multicast_v6(&multi_addr, *scope_id)?;
        sock.set_multicast_loop_v6(false)?;
    }
    Ok(sock)
}

pub type NetworkMsgSink = SplitSink<UdpFramed<MsgCodec>, (NetworkMsg, SocketAddr)>;
pub type NetworkMsgStream = SplitStream<UdpFramed<MsgCodec>>;
pub type NetworkMsgSinkMap = HashMap<EndPoint, NetworkMsgSink>;
pub type NetworkMsgStreamMux = SelectAll<NetworkMsgStream>;

pub async fn split_group() -> Result<(NetworkMsgSinkMap, NetworkMsgStreamMux)> {
    let results = try_join_all(NicView::default().map(async move |iface| -> Result<_> {
        let addr = EndPoint::new(iface, global_config().protocol_port);
        let sock = create_socket(&addr).await?;
        Ok((addr, UdpFramed::new(sock, MsgCodec).split()))
    }))
    .await?;
    let mut sinks = HashMap::with_capacity(results.len());
    let mut streams = SelectAll::new();
    for (addr, (sink, stream)) in results {
        sinks.insert(addr, sink);
        streams.push(stream);
    }
    Ok((sinks, streams))
}
