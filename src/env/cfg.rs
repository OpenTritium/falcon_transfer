use crate::utils::Uid;
use std::sync::OnceLock;

pub struct Env {
    pub host_id: Uid,
    pub host_name: &'static (dyn Fn() -> String + Sync + Send),
    pub protocol_port: u16,
    pub protocol_version: u8,
    pub user_name: &'static str,
}

static ENV: OnceLock<Env> = OnceLock::new();

pub fn global_config() -> &'static Env {
    ENV.get_or_init(|| Env {
        host_id: Uid::random(),
        host_name: &(|| hostname::get().unwrap().to_string_lossy().to_string()),
        protocol_port: 5555, //本机监听端口，别人不一定是这个
        protocol_version: 0x0,
        user_name: "",
    })
}
