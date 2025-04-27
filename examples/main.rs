use falcon_transfer::config::{ConfigItem, ConfigManager};
use indoc::indoc;
use tokio::{io::AsyncWriteExt, time::sleep, time::Duration};

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();  // 初始化日志记录器
    // 首次创建并写入配置文件
    let mut file = tokio::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .open("config.toml")
        .await
        .unwrap();

    let ctx = indoc! {
        br#"
            host_id = "FFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFF"
            host_name = "TritiumQin"
            protocol_port = 5555
            protocol_version = 0
            "#
    };
    file.write_all(ctx).await.unwrap();
    file.flush().await.unwrap();
    file.sync_all().await.unwrap();  // 确保写入磁盘

    // 初始化配置管理器
    let manager = ConfigManager::try_new("config.toml").unwrap();
    
    // 第一次读取
    let id = manager.async_get(ConfigItem::HostId).await;
    println!("首次读取 Host ID: {:?}", id);

    // 覆盖文件内容（关键修改点）
    let mut file = tokio::fs::OpenOptions::new()
        .write(true)
        .truncate(true)  // 清空文件内容
        .open("config.toml")
        .await
        .unwrap();

    let new_ctx = indoc! {
        br#"
            host_id = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"
            host_name = "None"
            protocol_port = 6666
            protocol_version = 6
            "#
    };
    file.write_all(new_ctx).await.unwrap();
    file.flush().await.unwrap();
    file.sync_all().await.unwrap();  // 确保写入磁盘

    // 添加等待时间（关键修改点）
    sleep(Duration::from_secs(6)).await;  // 等待文件监视器刷新
    
    // 第二次读取
    let id = manager.async_get(ConfigItem::HostId).await;
    println!("更新后 Host ID: {:?}", id);
}