use super::{
    FileHash, OptSource, Payload, TaggedTaskEvent, TaskCommand, TaskCtrl, TaskEvent, TaskState,
};
use crate::{
    hot_file::{FileRange, HotFile, arrange_bytes_to_vec},
    utils::{HostId, Uid},
};
use tokio::sync::{mpsc, watch};

async fn verify_hash_or_correct(
    file: &HotFile,
    range: FileRange,
    remote: FileHash,
    event_in: &mpsc::Sender<TaggedTaskEvent>,
    status_in: &watch::Sender<TaskState>,
    host: HostId,
) {
    match file.read(range.into()).await {
        Ok(bufs) => {
            if HotFile::hash(&bufs) != remote {
                let payload = Payload::new(range.start(), arrange_bytes_to_vec(bufs.into_iter()));
                if let Err(err) = event_in
                    .send(((0, host.clone()), TaskEvent::Confirm(payload)))
                    .await
                {
                    status_in.send_modify(|state| {
                        state.set_upload_err(host, err);
                    })
                }
            }
        }
        Err(err) => status_in.send_modify(|state| {
            state.set_upload_err(host, err);
        }),
    }
}

pub async fn main_event_loop(
    remote: HostId, // 主任务主机的id，只用于传递到事件而不是命令
    file: HotFile,
    mut ctrl_out: mpsc::Receiver<TaskCtrl>, // 被传递到这个任务的控制
    event_in: mpsc::Sender<TaggedTaskEvent>, //下游网络事件输入，用于分享到其他
    status_in: watch::Sender<TaskState>,    // 状态更新输入
) {
    loop {
        if !status_in.borrow().has_download_error()
            && let Some(ctrl) = ctrl_out.recv().await
        {
            let handle_payload = async |payload: Payload| {
                let occupy = payload.occupy();
                file.write(payload.buf(), occupy.start())
                    .await
                    .map_err(|err| {
                        status_in.send_modify(|state| {
                            state.set_download_err(err);
                        })
                    });
            };
            use TaskCommand::*;
            use TaskCtrl::*;
            use TaskEvent::*;
            match ctrl {
                Event(New(_)) => unreachable!(),
                Event(Append(payload)) => handle_payload(payload).await, // 实现恢复
                Event(Confirm(patch)) => {
                    file.sync().await.unwrap();
                    handle_payload(patch).await;
                }
                Event(Cancel) => {
                    status_in.send_modify(|state| {
                        state.stop_download(OptSource::Remote).map_err(|err| {
                            state.set_download_err(err);
                        });
                    });
                }
                Event(Check {
                    range,
                    partial_hash,
                }) => {
                    verify_hash_or_correct(
                        &file,
                        range,
                        partial_hash,
                        &event_in,
                        &status_in,
                        remote.clone(),
                    )
                    .await
                }

                Command(Rescind(_)) => todo!(), //那还有想办法保存另一个任务的状态
                Command(Share(_)) => todo!(),   // 启动另外的任务
                Command(Open(_)) => todo!(), // 需要维护一个分享表，映射到任务的取消token和watch上
            }
        }
    }
}
