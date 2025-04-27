use config::{Config, ConfigError, File};
use notify_debouncer_mini::{
    new_debouncer,
    notify::{self, RecursiveMode},
};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use thiserror::Error;
use tokio::{
    sync::{RwLock as AsyncRwLock, mpsc::unbounded_channel},
    task::yield_now,
};
use tracing::{error, info};

#[derive(Debug, Error)]
pub enum ConfigManagerError {
    #[error(transparent)]
    ConfigError(#[from] ConfigError),
    #[error(transparent)]
    NotifyError(#[from] notify::Error),
}

type Prefs = HashMap<String, String>;
pub struct ConfigManager {
    prefs: Arc<AsyncRwLock<Prefs>>,
    absolute_path: PathBuf,
}

pub enum ConfigItem {
    ProtocolPort,
    ProtocolVersion,
    HostName,
    HostId,
}
impl ConfigManager {
    fn load_config(path: &str) -> Result<Config, ConfigManagerError> {
        let cfg = Config::builder()
            .add_source(File::with_name(path))
            .build()?;
        Ok(cfg)
    }

    pub fn try_new(path: &str) -> Result<Self, ConfigManagerError> {
        let prefs = Self::load_config(path)?.try_deserialize::<Prefs>()?;
        let absolute_path = PathBuf::from(path);
        let prefs = Arc::new(AsyncRwLock::new(prefs));
        Self::watch(&absolute_path, prefs.clone())?;
        Ok(Self {
            prefs,
            absolute_path,
        })
    }

    #[inline]
    fn perf_map(item: ConfigItem) -> &'static str {
        match item {
            ConfigItem::ProtocolPort => "protocol_port",
            ConfigItem::ProtocolVersion => "protocol_version",
            ConfigItem::HostName => "host_name",
            ConfigItem::HostId => "host_id",
        }
    }

    pub async fn async_get(&self, item: ConfigItem) -> Option<String> {
        let k = Self::perf_map(item);
        self.prefs.read().await.get(k).cloned()
    }

    pub fn block_get(&self, item: ConfigItem) -> Option<String> {
        let k = Self::perf_map(item);
        self.prefs.blocking_read().get(k).cloned()
    }

    async fn refresh(
        config_path: &Path,
        prefs: Arc<AsyncRwLock<Prefs>>,
    ) -> Result<(), ConfigManagerError> {
        let new_prefs =
            Self::load_config(config_path.to_str().unwrap())?.try_deserialize::<Prefs>()?;
        *prefs.write().await = new_prefs;
        Ok(())
    }

    pub(crate) fn watch(
        config_path: &Path,
        prefs: Arc<AsyncRwLock<Prefs>>,
    ) -> Result<(), notify::Error> {
        println!("开始监视配置文件: {:?}", config_path);
        let (tx, mut rx) = unbounded_channel();
        let mut debouncer = new_debouncer(Duration::from_secs(2), move |result| {
            info!("触发回调{:?}", result);
            println!("触发回调{:?}", result);
            match result {
                Ok(event) => tx.send(event).unwrap_or_else(|_| ()),
                Err(err) => error!("Error: {err}"),
            }
        })?;
        let path = config_path.canonicalize().unwrap();
        debouncer
            .watcher()
            .watch(&path, RecursiveMode::Recursive)?;
        tokio::spawn(async move {
            while let Some(e) = rx.recv().await {
                info!("Config changed; refreshing...{:?}", e);
                if let Err(err) = Self::refresh(&path, prefs.clone()).await {
                    error!("{err}");
                }
                yield_now().await;
            }
        });
        Ok(())
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use std::{
        io::Write,
        sync::{Arc, OnceLock},
    };
    use tempfile::Builder;
    use tokio::{
        fs::OpenOptions,
        io::AsyncWriteExt,
        task::yield_now,
        time::{Duration, sleep},
    };

    #[tokio::test]
    async fn config_loading() {
        // 创建带.toml扩展名的临时文件
        let mut file = Builder::new()
            .prefix("test_config")
            .suffix(".toml")
            .tempfile()
            .unwrap();

        // 写入有效配置内容
        writeln!(file, "protocol_port = 5555\nprotocol_version = 0").unwrap();
        let path = file.path().to_str().unwrap();

        // 初始化配置管理器
        let manager = ConfigManager::try_new(path).unwrap();

        // 验证配置加载
        assert_eq!(
            manager.async_get(ConfigItem::ProtocolPort).await,
            Some("5555".to_string())
        );
        assert_eq!(
            manager.async_get(ConfigItem::ProtocolVersion).await,
            Some("0".to_string())
        );
    }

    #[tokio::test]
    async fn dynamic_refresh() {
        // Create a temporary directory and config file
        let dir = Builder::new().prefix("config_test").tempdir().unwrap();
        let path = dir.path().join("config.toml");

        // Write initial config
        std::fs::write(&path, "protocol_port = 6666").unwrap();
        let manager = ConfigManager::try_new(path.to_str().unwrap()).unwrap();
        // Ensure the file system events are flushed and processed
        let mut file = OpenOptions::new().write(true).open(&path).await.unwrap();
        file.write(b"protocol_port = 5555").await.unwrap();
        file.sync_all().await.unwrap();
        tokio::time::sleep(Duration::from_secs(1)).await;
        // Poll for the config update with a 5-second timeout
        let result = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if manager.async_get(ConfigItem::ProtocolPort).await == Some("5555".to_string()) {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        })
        .await;

        // Assert that the config refreshed within the timeout
        assert!(
            result.is_ok(),
            "Config did not refresh to '5555' within 5 seconds; still reads {:?}",
            manager.async_get(ConfigItem::ProtocolPort).await
        );
    }

    #[tokio::test]
    async fn concurrent_access() {
        let manager = Arc::new(create_test_manager("host_name = \"TritiumQin\"").unwrap());

        // 并发读取测试
        let handles: Vec<_> = (0..10)
            .map(|_| {
                let manager = manager.clone();
                tokio::spawn(async move { manager.async_get(ConfigItem::HostName).await })
            })
            .collect();

        for handle in handles {
            assert_eq!(handle.await.unwrap(), Some("TritiumQin".to_string()));
        }
    }

    #[tokio::test]
    async fn error_handling() {
        // 无效文件路径
        let result = ConfigManager::try_new("nonexistent.toml");
        assert!(matches!(
            result.err(),
            Some(ConfigManagerError::ConfigError(_))
        ));

        // 非法格式测试
        let mut file = Builder::new().suffix(".toml").tempfile().unwrap();
        writeln!(file, "invalid_toml [").unwrap();
        let path = file.path().to_str().unwrap();

        let result = ConfigManager::try_new(path);
        assert!(matches!(
            result.err(),
            Some(ConfigManagerError::ConfigError(_))
        ));
    }

    // 辅助函数优化
    fn create_test_manager(content: &str) -> Result<ConfigManager, ConfigManagerError> {
        let mut file = Builder::new().suffix(".toml").tempfile().unwrap();
        writeln!(file, "{}", content).unwrap();
        let path = file.path().to_str().unwrap();
        ConfigManager::try_new(path)
    }

    pub fn mock() -> Result<&'static ConfigManager, ConfigManagerError> {
        static CONFIG_MANAGER: OnceLock<ConfigManager> = OnceLock::new();
        CONFIG_MANAGER.get_or_try_init(|| {
            create_test_manager(
                r#"
            host_id = "FFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFF"
            host_name = "TritiumQin"
            protocol_port = 5555
            protocol_version = 0
            "#,
            )
        })
    }
}
