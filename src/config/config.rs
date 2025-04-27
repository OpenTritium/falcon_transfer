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
    #[error("配置状态异常{0}")]
    ConfigStateError(String),
}

type Prefs = HashMap<String, String>;
pub struct ConfigManager {
    prefs: Arc<AsyncRwLock<Result<Prefs, ConfigManagerError>>>, // todo 用 result 包一下
    abs_path: PathBuf,
}

pub enum ConfigItem {
    ProtocolPort,
}
impl ConfigManager {
    fn load_config(path: &Path) -> Result<Config, ConfigManagerError> {
        let cfg = Config::builder()
            .add_source(File::with_name(path.to_str().unwrap()))
            .build()?;
        Ok(cfg)
    }

    pub fn try_open(path: &Path) -> Result<Self, ConfigManagerError> {
        let prefs = Self::load_config(path)?.try_deserialize::<Prefs>()?;
        let abs_path = PathBuf::from(path);
        let prefs = Arc::new(AsyncRwLock::new(Ok(prefs)));
        Self::watch(&abs_path, prefs.clone())?;
        Ok(Self { prefs, abs_path })
    }

    #[inline]
    fn pref_map(item: ConfigItem) -> &'static str {
        match item {
            ConfigItem::ProtocolPort => "protocol_port",
        }
    }

    pub async fn async_get(&self, item: ConfigItem) -> Result<String, ConfigManagerError> {
        let k = Self::pref_map(item);
        match self.prefs.read().await.as_ref() {
            Ok(prefs) => {
                prefs
                    .get(k)
                    .cloned()
                    .ok_or(ConfigManagerError::ConfigError(ConfigError::NotFound(
                        k.to_string(),
                    )))
            }
            Err(e) => Err(ConfigManagerError::ConfigStateError(e.to_string())),
        }
    }

    pub fn block_get(&self, item: ConfigItem) -> Result<String, ConfigManagerError> {
        let k = Self::pref_map(item);
        match self.prefs.blocking_read().as_ref() {
            Ok(prefs) => {
                prefs
                    .get(k)
                    .cloned()
                    .ok_or(ConfigManagerError::ConfigError(ConfigError::NotFound(
                        k.to_string(),
                    )))
            }
            Err(e) => Err(ConfigManagerError::ConfigStateError(e.to_string())),
        }
    }

    async fn refresh(
        config_path: &Path,
        prefs: Arc<AsyncRwLock<Result<Prefs, ConfigManagerError>>>,
    ) -> Result<(), ConfigManagerError> {
        let new_prefs = match Self::load_config(config_path) {
            Ok(cfg) => match cfg.try_deserialize::<Prefs>() {
                Ok(prefs) => Ok(prefs),
                Err(e) => Err(ConfigManagerError::ConfigError(e)),
            },
            Err(e) => Err(e),
        };
        *prefs.write().await = new_prefs;
        Ok(())
    }

    pub(crate) fn watch(
        config_path: &Path,
        prefs: Arc<AsyncRwLock<Result<Prefs, ConfigManagerError>>>,
    ) -> Result<(), notify::Error> {
        let (tx, mut rx) = mpsc::channel(1);
        let mut debouncer = new_debouncer(Duration::from_secs(1), move |result| {
            if let Ok(event) = result {
                tx.blocking_send(event).unwrap();
            }
        })?;
        let path = config_path.canonicalize().unwrap();
        debouncer.watcher().watch(&path, RecursiveMode::Recursive)?;
        tokio::spawn(async move {
            let _debouncer = debouncer; // 移动到这个协程里防止被drop
            while let Some(_) = rx.recv().await {
                let _ = Self::refresh(&path, prefs.clone()).await;
                yield_now().await;
            }
        });
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{io::{Seek, SeekFrom, Write}, thread::sleep};
    use tempfile::Builder;
    use tokio::time::{self, Duration};

    // 创建带 .toml 后缀的临时配置文件
    fn create_temp_config(content: &str) -> (tempfile::TempDir, PathBuf) {
        let dir = Builder::new().tempdir().unwrap();
        let file_path = dir.path().join("config.toml");
        
        let mut file = std::fs::File::create(&file_path).unwrap();
        writeln!(file, "{}", content).unwrap();
        file.sync_all().unwrap();
        
        (dir, file_path)
    }

    #[tokio::test(start_paused = true)]
    async fn test_async_get_valid_config() {
        let (dir, path) = create_temp_config("protocol_port = \"8080\"");
        let manager = ConfigManager::try_open(&path).unwrap();

        let port = manager.async_get(ConfigItem::ProtocolPort).await.unwrap();
        assert_eq!(port, "8080");
        dir.close().unwrap(); // 显式清理
    }

    #[tokio::test]
    async fn test_config_refresh_on_change() {
        let (dir, path) = create_temp_config("protocol_port = \"8080\"");
        let manager = ConfigManager::try_open(&path).unwrap();

        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .truncate(true)
            .open(&path)
            .unwrap();
        file.write_all(b"protocol_port = \"8081\"").unwrap();
        file.flush().unwrap();
        file.sync_all().unwrap();

        sleep(Duration::from_secs(10)); // 等待文件监视器刷新

        let port = manager.async_get(ConfigItem::ProtocolPort).await.unwrap();
        assert_eq!(port, "8081");
        dir.close().unwrap();
    }
}