use super::{ConfigManager, ConfigManagerError};
use std::sync::OnceLock;

static GLOBAL_CONFIG: OnceLock<ConfigManager> = OnceLock::new();

pub fn global_config() -> Result<&'static ConfigManager, ConfigManagerError> {
    GLOBAL_CONFIG.get_or_try_init(|| {
        ConfigManager::try_new(
            r#"
            host_id = "FFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFF"
            host_name = "TritiumQin"
            protocol_port = 5555
            protocol_version = 0
            "#,
        )
    })
}
