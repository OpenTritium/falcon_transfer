use crate::{codec::MsgCodec, endpoint::EndPoint, msg::Msg};
use crate::{env, nic::NicView};
use anyhow::{Ok, Result};
use dashmap::DashMap;
use futures::StreamExt;
use std::net::{Ipv6Addr, SocketAddr};
use tokio::net::UdpSocket;
use tokio_util::udp::UdpFramed;

pub type MsgSink = futures::stream::SplitSink<UdpFramed<MsgCodec>, (Msg, SocketAddr)>;
pub type MsgStream = futures::stream::SplitStream<UdpFramed<MsgCodec>>;
pub type MsgSinkStreamGroup = DashMap<EndPoint, (MsgSink, MsgStream)>;

/// 为所有活跃的网络接口创建 socket
/// 对于本地链路地址需要加入特定组播进行发现
/// 对于 scope 比 linklocal 更广的地址则不需要加入组播
pub async fn create_socket(addr: &EndPoint) -> Result<UdpSocket> {
    let multi_addr = "FF12::1".parse::<Ipv6Addr>()?;
    let sock = UdpSocket::bind(SocketAddr::from(*addr)).await?;
    if let Some(scope_id) = addr.get_scope_id() {
        sock.join_multicast_v6(&multi_addr, scope_id)?;
        sock.set_multicast_loop_v6(false)?;
    }
    Ok(sock)
}

async fn split_group() -> Result<MsgSinkStreamGroup> {
    Ok(futures::future::try_join_all(
        NicView::default()
            .into_iter()
            .filter(|iface| !iface.is_lan())
            .map(async |iface| {
                let addr = EndPoint::new(iface, env().protocol_port);
                let sock = create_socket(&addr).await?;
                Ok((addr, UdpFramed::new(sock, MsgCodec).split()))
            }),
    )
    .await?
    .into_iter()
    .collect())
}
