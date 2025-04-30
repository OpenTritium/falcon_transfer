use directories::ProjectDirs;

use super::{ConfigManager, ConfigManagerError};
use std::sync::OnceLock;

pub fn config_manager() -> Result<&'static ConfigManager, ConfigManagerError> {
    static CONFIG_MANAGER: OnceLock<ConfigManager> = OnceLock::new();
    CONFIG_MANAGER.get_or_try_init(|| {
        let prj_dir = ProjectDirs::from("com", "tritium",  "falcon_transfer").ok_or(ConfigManagerError::ConfigDirNotFound)?;
        let cfg_dir = prj_dir.config_local_dir();
        if !cfg_dir.exists() {
            std::fs::create_dir_all(cfg_dir)?;
        }
        let path = cfg_dir.join("config.toml");
        println!("Config file path: {:?}", path);
        ConfigManager::create(&path)
    })
}
