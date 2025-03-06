use crate::link::ResumeTask;
use crate::utils::EndPoint;
use thiserror::Error;
use tokio::sync::mpsc::error::TrySendError;

#[derive(Debug, Error)]
pub enum ResumeTaskError {
    #[error("the link_state entry has been removed bt another thread.")]
    RemovedByOtherThread,
    #[error(transparent)]
    TaskSendError(#[from] TrySendError<ResumeTask>),
}


pub struct AssignedLink {
    pub local: EndPoint,
    pub remote: EndPoint,
    pub solve: Box<dyn FnOnce() -> Result<(), ResumeTaskError> + Send + 'static>,
}