#![feature(ip)]
#![feature(duration_constructors)]
use std::future::pending;

mod scoped_addr;
mod env;
mod uid;
mod nic;
mod handshake;
mod socket;
mod endpoint;
use env::env;
mod msg;
mod codec;
mod agent;
mod link_state;
mod link_state_table;
mod link_recovery_scheduler;

#[tokio::main]
async fn main() {
// 从一开始就要根据nic列表准备socket，
//随即广播自己的本地链路地址和uid
//收到后根据uid和地址聚合记录
    pending::<()>().await;
}
