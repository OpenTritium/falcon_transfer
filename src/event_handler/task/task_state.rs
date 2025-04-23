use std::{borrow::Cow, collections::HashMap};

use super::{TaskError, TaskTag};
use crate::{
    hot_file::{FileMultiRange, FileRange, FileRangeError},
    utils::HostId,
};
use thiserror::Error;

/// 任务状态机可能发生的错误
#[derive(Debug, Error)]
pub enum ProgressError {
    /// 文件范围操作错误
    #[error(transparent)]
    Range(#[from] FileRangeError),
    #[error("")]
    UploadTaskNotExist,
    /// 状态转换错误
    #[error("Invalid state transition: {0}")]
    Transition(Cow<'static, str>),
}

/// 操作来源（远程/本地）
#[derive(Debug, Clone, Copy)]
pub enum OptSource {
    Remote,
    Local,
}

/// 工作负载状态
#[derive(Debug)]
pub enum WorkloadState {
    /// 运行中
    Running,

    /// 已暂停（包含暂停来源）
    Paused(OptSource),
}

impl WorkloadState {
    /// 判断是否处于运行状态
    fn is_running(&self) -> bool {
        matches!(self, Self::Running)
    }

    /// 判断是否处于暂停状态
    fn is_paused(&self) -> bool {
        !self.is_running()
    }
}

/// 带状态管理的进度信息
#[derive(Debug)]
pub struct ProgressState {
    /// 已完成的文件范围
    progress: FileMultiRange,

    /// 当前工作状态
    state: WorkloadState,
}

impl PartialEq for ProgressState {
    fn eq(&self, other: &Self) -> bool {
        self.progress == other.progress
    }
}

impl ProgressState {
    /// 添加新的进度范围（仅在运行状态允许）
    pub fn add(&mut self, rgn: FileRange) -> Result<(), ProgressError> {
        if self.state.is_running() {
            self.progress.add(rgn);
            Ok(())
        } else {
            Err(ProgressError::Transition(
                "Cannot add range while paused".into(),
            ))
        }
    }

    /// 暂停操作
    pub fn pause(&mut self, src: OptSource) -> Result<(), ProgressError> {
        if self.state.is_running() {
            self.state = WorkloadState::Paused(src);
            Ok(())
        } else {
            Err(ProgressError::Transition("Already paused".into()))
        }
    }

    /// 恢复操作
    pub fn resume(&mut self) -> Result<(), ProgressError> {
        if self.state.is_paused() {
            self.state = WorkloadState::Running;
            Ok(())
        } else {
            Err(ProgressError::Transition("Not paused".into()))
        }
    }

    /// 获取当前进度
    pub fn progress(&self) -> &FileMultiRange {
        &self.progress
    }
}

impl Default for ProgressState {
    fn default() -> Self {
        Self {
            progress: Default::default(),
            state: WorkloadState::Running,
        }
    }
}

/// 完整任务状态管理
#[derive(Debug)]
pub struct TaskState {
    /// 上传进度状态，或任务错误
    uploaded: Option<HashMap<HostId, Result<ProgressState, TaskError>>>,

    /// 下载进度状态
    downloaded: Result<ProgressState, TaskError>,

    /// 完整文件范围
    full: FileMultiRange,
}

impl TaskState {
    pub fn try_new(total: usize) -> Result<Self, ProgressError> {
        Ok(Self {
            uploaded: None,
            downloaded: Ok(Default::default()),
            full: FileRange::try_new(0, total)?.into(),
        })
    }

    fn with_download_mut<F>(&mut self, f: F) -> Result<(), TaskError>
    where
        F: FnOnce(&mut ProgressState) -> Result<(), ProgressError>,
    {
        let state = self.downloaded.as_mut().map_err(|err| {
            ProgressError::Transition(format!("Download in error state: {err} ").into())
        })?;
        f(state)?; //  细节将进度错误转换到任务错误
        Ok(())
    }

    /// 记录下载范围
    pub fn download(&mut self, rgn: FileRange) -> Result<(), TaskError> {
        self.with_download_mut(|s| s.add(rgn))
    }

    /// 记录上传范围
    pub fn with_upload_mut<F>(&mut self, host: HostId, f: F) -> Result<(), TaskError>
    where
        F: FnOnce(&mut ProgressState) -> Result<(), ProgressError>,
    {
        use std::collections::hash_map::Entry;
        let uploaded_map = self.uploaded.get_or_insert_default();
        match uploaded_map.entry(host) {
            Entry::Occupied(mut entry) => {
                let state = entry.get_mut().as_mut().map_err(|err| {
                    TaskError::from(ProgressError::Transition(
                        format!("Upload in error state: {err}").into(),
                    ))
                })?;
                f(state)?;
            }
            Entry::Vacant(entry) => {
                // 没有就插入默认值
                entry.insert(Ok(Default::default()));
            }
        }
        Ok(())
    }

    /// 暂停上传
    pub fn stop_upload(&mut self, host: HostId, src: OptSource) -> Result<(), TaskError> {
        self.with_upload_mut(host, |s| s.pause(src))
    }

    /// 暂停下载
    pub fn stop_download(&mut self, src: OptSource) -> Result<(), TaskError> {
        self.with_download_mut(|s| s.pause(src))
    }

    /// 恢复上传
    pub fn resume_upload(&mut self, host: HostId) -> Result<(), TaskError> {
        self.with_upload_mut(host, |s| s.resume())
    }

    /// 恢复下载
    pub fn resume_download(&mut self) -> Result<(), TaskError> {
        self.with_download_mut(|s| s.resume())
    }

    // todo 错误链实现
    pub fn set_download_err(&mut self, err: impl Into<TaskError>) {
        self.downloaded = Err(err.into());
    }

    pub fn set_upload_err(&mut self, host: HostId, err: impl Into<TaskError>) {
        let uploaded_map = self.uploaded.get_or_insert_default();
        let entry = uploaded_map.entry(host);
        use std::collections::hash_map::Entry;
        match entry {
            Entry::Occupied(mut entry) => {
                let state = entry.get_mut();
                *state = Err(err.into());
            }
            Entry::Vacant(entry) => {
                entry.insert(Err(err.into()));
            }
        }
    }

    /// 检查下载错误状态
    pub fn has_download_error(&self) -> bool {
        self.downloaded.is_err()
    }

    pub fn get_download_progress(&self) -> &Result<ProgressState, TaskError> {
        &self.downloaded
    }

    pub fn get_upload_progress(&self, host: &HostId) -> Option<&Result<ProgressState, TaskError>> {
        let Some(upload_map) = self.uploaded.as_ref() else {
            return None;
        };
        upload_map.get(host)
    }
}

// 主要应对初始化文件range时的结果，成功就直接返回成功状态，失败就转换成状态
impl From<Result<TaskState, ProgressError>> for TaskState {
    fn from(value: Result<TaskState, ProgressError>) -> Self {
        match value {
            Ok(state) => state,
            Err(err) => TaskState {
                uploaded: None,
                downloaded: Err(err.into()),
                full: Default::default(),
            },
        }
    }
}
