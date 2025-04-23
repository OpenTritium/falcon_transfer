use crate::{
    link::link_state_table,
    utils::{EndPoint, HostId},
};

pub fn on_discovery(remote: EndPoint, target: HostId, local: EndPoint) {
    // 查找链路状态表单例
    // 查找到就修改链路状态表
    // 我觉得没必要将事件处理结果返回到事件处理器层面
    link_state_table().update(target, &local, &remote);
    // 立刻准备initiator，然后更新到channel表中，使用它构建消息然后发送
    // 对方收到hello消息后准备responder,更新到自己的链路状态表中
    // 假如双方同时收到发现消息并同时回复
    // 这里就要体现发送消息后将链路变更为hello的重要性
    // 在链路状态检测中加入如果已经被hello了则不回复hello
    // 包装一下eventReciver为数据channel
}
