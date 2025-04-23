use super::{Payload, TaggedTaskEvent, TaskEvent, TaskState, TaskTag};
use crate::hot_file::{HotFile, arrange_bytes_to_vec};
use tokio::{
    sync::{mpsc, watch},
    task::AbortHandle,
};

// 这个函数应当应对share 事件，且返回aborthandle
fn spwan_share_task(
    file: HotFile,
    mut status_out: watch::Receiver<TaskState>,
    status_in: watch::Sender<TaskState>,
    event_in: mpsc::Sender<TaggedTaskEvent>,
    tag: TaskTag,
) -> AbortHandle {
    tokio::spawn(async move {
        // 先观察当前进度，迅速生成数据流扔管道里
        'a: loop {
            // 然后等待下载进度变化
            if let Err(_) = status_out.changed().await {
                break;
            }

            let (_, host) = tag.clone();
            // 获取下载和上传进度
            //这样会有问题吗？当然没有，主任务保存了上传进度的
            // 不过下一版本需要将上传进度改成map了
            let remain = {
                let borrowed_status = status_out.borrow();
                let Ok(download) = borrowed_status.get_download_progress() else {
                    break;
                };
                let Some(result) = borrowed_status.get_upload_progress(&host) else {
                    break;
                };
                let Ok(upload) = result else {
                    break;
                };
                download.progress().subtract(&upload.progress())
            };
            // 分割成指定大小的块
            let mut split_iter = remain.split(8); // 假设返回 Result 迭代器
            // 遍历每个分割后的区块
            while let Some(rgn_result) = split_iter.next() {
                match rgn_result {
                    Ok(rgn) => {
                        let buf = file.read(rgn.into()).await.unwrap();
                        let buf = arrange_bytes_to_vec(buf.into_iter());
                        // 构造并发送网络事件
                        let event = (tag.clone(), TaskEvent::Append(Payload::new(0, buf)));
                        if let Err(err) = event_in.send(event).await {
                            status_in.send_modify(|state| state.set_upload_err(host.clone(), err));
                            break 'a;
                        }
                    }
                    Err(err) => {
                        // 分割错误时更新状态并退出
                        status_in.send_modify(|state| state.set_upload_err(host, err));
                        break 'a;
                    }
                }
            }
        }
    })
    .abort_handle()
}
