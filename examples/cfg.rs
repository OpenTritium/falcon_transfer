use std::time::Duration;

use falcon_transfer::config::config_manager;
use tokio::time::sleep;

#[tokio::main]
async fn main(){
    let cfg = config_manager().unwrap();
    cfg.set(falcon_transfer::config::ConfigItem::ProtocolPort, "8080".into()).await.unwrap();
    sleep(Duration::from_secs(8)).await; // Sleep to allow the config manager to refresh
    println!("Protocol port set to: {}", cfg.get(falcon_transfer::config::ConfigItem::ProtocolPort).await);
    cfg.set(falcon_transfer::config::ConfigItem::ProtocolPort, "8081".into()).await.unwrap();
    sleep(Duration::from_secs(2)).await; // Sleep to allow the config manager to refresh
    println!("Protocol port set to: {}", cfg.get(falcon_transfer::config::ConfigItem::ProtocolPort).await);
}