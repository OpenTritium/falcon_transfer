use crate::{hot_file::FileRange, utils::HostId};
use bytes::Bytes;
use std::{path::Path, usize};
pub type FileHash = u64;

// 传输事件，上下游均能收到，来源网络
// 在外面包key谢谢
// 下游的事件也要变更到包含key的

pub enum TaskEvent {
    New(FileInfo),
    Append(Payload),
    Confirm(Payload),
    Cancel,
    // Resume(progress)//来自远方的请求恢复事件，并携带了进度
    Check {
        range: FileRange,
        partial_hash: FileHash,
    },
}

// 传输命令，控制下游该传输什么传输事件
pub enum TaskCommand {
    Open(FileInfo), // 已经open 了就不能new了
    Share(TaskTag),
    Rescind(TaskTag), //
}

pub enum TaskCtrl {
    Event(TaskEvent),
    Command(TaskCommand),
}

pub type TaskTag = (FileHash, HostId);
pub type TaggedTaskEvent = (TaskTag, TaskEvent);

pub struct FileInfo {
    file_hash: FileHash,
    file_name: PathBuf, //文件名
    size: usize,
}

impl FileInfo {
    pub fn file_hash(&self) -> FileHash {
        self.file_hash
    }

    pub fn size(&self) -> usize {
        self.size
    }

    pub fn file_name(&self) -> &Path {
        self.file_name.as_ref()
    }
}

pub struct Payload {
    offset: usize,
    buf: Bytes,
}

impl Payload {
    /// 直接夺舍 vec
    pub fn new(offset: usize, buf: Vec<u8>) -> Self {
        Self {
            offset,
            buf: Bytes::from(buf),
        }
    }

    pub fn buf(&self) -> &[u8] {
        self.buf.as_ref()
    }

    pub fn offset(&self) -> usize {
        self.offset
    }

    pub fn occupy(&self) -> FileRange {
        FileRange::new(self.offset, self.offset + self.buf.len())
    }
}
