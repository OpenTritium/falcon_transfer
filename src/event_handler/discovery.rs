use crate::utils::{EndPoint, HostId};

pub fn on_discovery(remote: EndPoint, target: HostId, local: EndPoint) {
    // 查找链路状态表单例
    // 查找到就修改链路状态表
    // 我觉得没必要将事件处理结果返回到事件处理器层面
}
