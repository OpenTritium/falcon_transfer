use super::{ConfigManager, ConfigManagerError};
use std::sync::OnceLock;

static GLOBAL_CONFIG: OnceLock<ConfigManager> = OnceLock::new();

// pub fn global_config() -> Result<&'static ConfigManager, ConfigManagerError> {
// }
