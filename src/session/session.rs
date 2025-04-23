use dashmap::DashMap;

use crate::utils::HostId;

use super::EncryptSession;

// 会话管理
struct SessionManager {
    inner: DashMap<HostId, EncryptSession>,
}

// 通过任务管理器
// 任务管理器可以下载也可以同时上传
// 通过 channel 将网络事件循环桥接到任务管理器
// 任务管理器将收到的buf写到hotfile里

// 任务管理器可以将新创建或已经存在的task传输到别人并同时写入
