use atomicwrites::{AtomicFile, OverwriteBehavior::AllowOverwrite};
use camino::{Utf8Path, Utf8PathBuf};
use config::{Config, ConfigError, File};
use notify_debouncer_mini::{
    new_debouncer,
    notify::{self, RecursiveMode},
};
use std::{
    collections::HashMap,
    fmt::Display,
    fs::OpenOptions,
    io::Write,
    sync::Arc,
    time::Duration,
};
use thiserror::Error;
use tokio::{
    sync::{RwLock as AsyncRwLock, mpsc},
    task::yield_now,
};
use tracing::error;

#[derive(Debug, Error)]
pub enum ConfigManagerError {
    #[error(transparent)]
    ConfigError(#[from] ConfigError),
    #[error(transparent)]
    NotifyError(#[from] notify::Error),
    #[error(transparent)]
    IoError(#[from] std::io::Error),
    #[error(transparent)]
    WriteError(#[from] atomicwrites::Error<std::io::Error>),
    #[error("config dir was not found")]
    ConfigDirNotFound,
}

type Settings = HashMap<String, String>;

pub struct ConfigManager {
    settings: Arc<AsyncRwLock<Settings>>,
    abs_path: Utf8PathBuf, // suffix must be .toml
}

#[derive(Debug, Clone, Copy)]
pub enum ConfigItem {
    ProtocolPort,
}

impl From<ConfigItem> for &'static str {
    #[inline]
    fn from(item: ConfigItem) -> Self {
        match item {
            ConfigItem::ProtocolPort => "protocol_port",
        }
    }
}

impl Display for ConfigItem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s: &'static str = (*self).into();
        write!(f, "{}", s)
    }
}

impl ConfigItem {
    #[inline]
    fn default(&self) -> &'static str {
        match self {
            ConfigItem::ProtocolPort => "5555",
        }
    }
}

impl ConfigManager {
    fn load_config(path: &Utf8Path) -> Result<Config, ConfigManagerError> {
        let cfg = Config::builder()
            .add_source(File::with_name(path.as_str()))
            .build()?;
        Ok(cfg)
    }

    fn default_inner() -> Settings {
        use ConfigItem::*;
        HashMap::from_iter([(ProtocolPort.to_string(), ProtocolPort.default().to_string())])
    }

    pub fn create(path: &Utf8Path) -> Result<Self, ConfigManagerError> {
        if !path.exists() {
            std::fs::File::create(path)?;
        }
        let abs_path = path.canonicalize_utf8()?;
        let cfg = match Self::load_config(path) {
            Ok(cfg) => cfg,
            Err(err) => {
                error!("{err}, construct config manager in default values");
                let settings = Arc::new(AsyncRwLock::new(Self::default_inner()));
                Self::watch(abs_path.clone(), settings.clone())?;
                return Ok(Self { settings, abs_path });
            }
        };
        let settings = cfg.try_deserialize::<Settings>().unwrap_or_else(|err| {
            error!("{err}");
            Self::default_inner()
        });
        let settings = Arc::new(AsyncRwLock::new(settings));
        Self::watch(abs_path.clone(), settings.clone())?;
        Ok(Self { settings, abs_path })
    }

    /// 没有就映射到默认值
    pub async fn get(&self, item: ConfigItem) -> String {
        self.settings
            .read()
            .await
            .get(item.into())
            .cloned()
            .unwrap_or_else(|| item.default().to_string())
    }

    // 如果之前的配置文件解析失败，应当生成新的空白配置文件并set
    // 这样其他的选项依然会遵从默认值
    pub async fn set(
        &self,
        item: ConfigItem,
        value: toml::Value,
    ) -> Result<(), ConfigManagerError> {
        AtomicFile::new(&self.abs_path, AllowOverwrite).write_with_options(
            |f| {
                let content = std::fs::read_to_string(&self.abs_path)?;
                let mut table: toml::value::Table = toml::from_str(&content).unwrap_or_default();
                table.insert(item.to_string(), value);
                let new_content =
                    toml::to_string_pretty(&table).expect("Failed to serialize table");
                f.write_all(new_content.as_bytes())?;
                f.flush()?;
                f.sync_all()?;
                Ok(())
            },
            OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .to_owned(),
        )?;
        Ok(())
    }

    /// 失败了不会修改读写锁中的内容
    async fn refresh(
        config_path: &Utf8Path,
        settings: Arc<AsyncRwLock<Settings>>,
    ) -> Result<(), ConfigManagerError> {
        let new = Self::load_config(config_path)?.try_deserialize::<Settings>()?;
        *settings.write().await = new;
        Ok(())
    }

    pub(crate) fn watch(
        config_path: Utf8PathBuf,
        settings: Arc<AsyncRwLock<Settings>>,
    ) -> Result<(), notify::Error> {
        let (tx, mut rx) = mpsc::channel(1);
        let mut debouncer = new_debouncer(Duration::from_secs(1), move |result| {
            if let Ok(event) = result {
                tx.blocking_send(event).unwrap();
            }
        })?;
        debouncer
            .watcher()
            .watch(config_path.as_std_path(), RecursiveMode::NonRecursive)?;
        tokio::spawn(async move {
            let _debouncer = debouncer; // 移动到这个协程里防止被drop
            while let Some(_) = rx.recv().await {
                let _ = Self::refresh(&config_path, settings.clone()).await; // 有时候刷新会失败，这是由于load时格式解析失败，直到格式正确锁中的内容才会被真正刷新
                yield_now().await;
            }
        });
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::Builder;
    use tokio::{
        fs::OpenOptions,
        io::AsyncWriteExt,
        time::{Duration, sleep},
    };

    // 创建带 .toml 后缀的临时配置文件
    fn create_temp_config(content: &str) -> (tempfile::TempDir, Utf8PathBuf) {
        let dir = Builder::new().tempdir().unwrap();
        let file_path:Utf8PathBuf = dir.path().join("config.toml").try_into().unwrap();
        let mut file = std::fs::File::create(&file_path).unwrap();
        writeln!(file, "{}", content).unwrap();
        file.sync_all().unwrap();

        (dir, file_path)
    }

    #[tokio::test(start_paused = true)]
    async fn async_get_valid_config() {
        let (dir, path) = create_temp_config("protocol_port = \"8080\"");
        let manager = ConfigManager::create(&path).unwrap();

        let port = manager.get(ConfigItem::ProtocolPort).await;
        assert_eq!(port, "8080");
        dir.close().unwrap(); // 显式清理
    }

    #[tokio::test]
    async fn config_refresh_on_change() {
        let (dir, path) = create_temp_config("protocol_port = \"8080\"");
        let manager = ConfigManager::create(&path).unwrap();

        let mut file = OpenOptions::new()
            .write(true)
            .truncate(true)
            .open(&path)
            .await
            .unwrap();
        file.write_all(b"protocol_port = \"8081\"").await.unwrap();
        file.flush().await.unwrap();
        file.sync_all().await.unwrap();

        sleep(Duration::from_secs(2)).await; // 监控线程非 tokio 协程无法快进
        // 平台相关：不同性能的平台收到事件的事件不一样，可能会有延迟

        let port = manager.get(ConfigItem::ProtocolPort).await;
        assert_eq!(port, "8081");
        dir.close().unwrap();
    }

    #[tokio::test]
    async fn set_config() {
        let (dir, path) = create_temp_config("protocol_port = \"8080\"");
        let manager = ConfigManager::create(&path).unwrap();
        manager
            .set(ConfigItem::ProtocolPort, "8081".into())
            .await
            .unwrap();
        sleep(Duration::from_secs(2)).await; // 监控线程非 tokio 协程无法快进

        let port = manager.get(ConfigItem::ProtocolPort).await;
        assert_eq!(port, "8081");
        dir.close().unwrap();
    }

    #[tokio::test]
    async fn handle_invalid_config() {
        let (dir, path) = create_temp_config("invalid_toml = [");
        let manager = ConfigManager::create(&path).unwrap();

        manager
            .set(ConfigItem::ProtocolPort, "8082".into())
            .await
            .unwrap();
        sleep(Duration::from_secs(2)).await;
        let port = manager.get(ConfigItem::ProtocolPort).await;
        assert_eq!(&port, "8082");
        dir.close().unwrap();
    }

    #[tokio::test]
    async fn preserve_other_settings() {
        let (dir, path) = create_temp_config("protocol_port = \"8080\"\nlog_level = \"debug\"\n");
        let manager = ConfigManager::create(&path).unwrap();

        manager
            .set(ConfigItem::ProtocolPort, "8081".into())
            .await
            .unwrap();

        let content = tokio::fs::read_to_string(&path).await.unwrap();
        println!("{}", content);
        assert!(content.contains("protocol_port = \"8081\""));
        assert!(content.contains("log_level = \"debug\""));
        dir.close().unwrap();
    }
}
