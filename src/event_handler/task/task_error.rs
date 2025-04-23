use super::{ProgressError, TaggedTaskEvent};
use crate::hot_file::{FileRangeError, HotFileError};
use thiserror::Error;
use tokio::sync::mpsc::error::{SendError, TrySendError};

#[derive(Error, Debug)]
pub enum TaskError {
    #[error("")]
    UnblockingSend(#[from] TrySendError<TaggedTaskEvent>),
    #[error("")]
    BlockingSend(#[from] SendError<TaggedTaskEvent>),
    #[error(transparent)]
    File(#[from] HotFileError),
    #[error("")]
    Range(#[from] FileRangeError),
    #[error("")]
    TaskState(#[from] ProgressError),
}
