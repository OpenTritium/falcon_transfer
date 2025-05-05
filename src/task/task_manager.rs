use super::{
    FileHash, FileInfo, TaggedTaskEvent, TaskCtrl, TaskError, TaskEvent, TaskState, TaskTag,
    main_event_loop,
};
use crate::{
    event_handler::task::{Payload, TaskCommand},
    hot_file::{FileMultiRange, FileRange, HotFile},
    utils::{HostId, Uid},
};
use bytes::Bytes;
use futures::stream::SelectAll;
use std::collections::HashMap;
use tokio::{
    sync::{mpsc, watch},
    task::AbortHandle,
};
use tokio_stream::wrappers::ReceiverStream;

// 通过信号量控制并行任务数量

type FileId = FileHash;
struct TaskManager {
    manager_event: mpsc::Sender<TaggedTaskEvent>,
    event_upstream: mpsc::Receiver<TaggedTaskEvent>, // 用于接受上游网络事件，这个时候的事件还带tag，需要自己分配到对应的 event_input
    // 下面记得套个 rwlock
    event_downstream: SelectAll<ReceiverStream<TaggedTaskEvent>>, // 这个组用于输出发送到其他客户端的下游网络事件
    // 记得封自己的uid
    event_inputs: HashMap<FileId, mpsc::Sender<TaskCtrl>>, //不同的协程映射的网络事件接收器
    status_outputs: HashMap<FileId, watch::Receiver<TaskState>>, // 支持根据文件id访问文件状态
    running_tasks: HashMap<FileId, AbortHandle>,           // 保存协程句柄，根据文件id取消协程
}

impl TaskManager {
    // 在taskmanager 实例化时也插入一个
    // 这个函数只会在 new 下触发
    // 创建任务时，让他拿着一个信号量
    pub async fn download_or_share(&mut self, file_info: FileInfo, remote: HostId) {
        let (up_event_in, up_event_out) = mpsc::channel::<TaskCtrl>(1024);
        let (down_event_in, down_event_out) = mpsc::channel::<TaggedTaskEvent>(1024);
        let task_state_init = TaskState::try_new(file_info.size());
        let (status_in, status_out) = watch::channel::<TaskState>(task_state_init.into());

        // 记得拼接下文件路径
        let Ok(file) = HotFile::open_new(file_info.file_name())
            .await
            .map_err(|err| {
                status_in.send_modify(|state| state.set_download_err(err));
            })
        else {
            // 趁现在还能摸到下游网络事件，往下面塞取消请求
            //self.manager_event.send(());
            return;
        };

        self.event_downstream
            .push(ReceiverStream::new(down_event_out));
        let file_id = file_info.file_hash();
        self.event_inputs.insert(file_id, up_event_in);
        self.status_outputs.insert(file_id, status_out);
        let abort = tokio::spawn(async move {
            main_event_loop(remote, file, up_event_out, down_event_in, status_in)
        })
        .abort_handle();
        self.running_tasks.insert(file_id, abort);
    }
}
