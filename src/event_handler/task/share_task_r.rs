impl Uploader {
    async fn run(mut self) {
        const MAX_BATCH_SIZE: usize = 32;
        let mut last_downloaded = FileMultiRange::new();

        loop {
            tokio::select! {
                // 监听下载进度变化
                _ = self.download_watcher.changed() => {
                    let current_download = self.download_watcher.borrow().clone();

                    // 计算新增的可上传范围
                    let new_ranges = current_download.difference(&last_downloaded);

                    for range in new_ranges.iter_ranges() {
                        self.process_range(range).await;
                        last_downloaded = current_download.clone();

                        // 批量发送优化
                        if self.batch.len() >= MAX_BATCH_SIZE {
                            self.flush_batch().await;
                        }
                    }
                }

                // 处理状态变化
                _ = self.state_watcher.changed() => {
                    if let TaskState::Paused(_) = *self.state_watcher.borrow() {
                        self.flush_batch().await; // 暂停前发送剩余数据
                        break; // 终止上传协程
                    }
                }

                // 主动刷新批次
                else => {
                    if !self.batch.is_empty() {
                        self.flush_batch().await;
                    }
                }
            }

            // 检查上传是否赶上
            if self.uploaded.contains(&last_downloaded) {
                // 等待新的下载进度通知
                self.download_watcher.changed().await;
            }
        }
    }

    async fn process_range(&mut self, range: FileRange) {
        match self.file.read(range).await {
            Ok(data) => {
                let payload = Payload {
                    offset: range.start(),
                    buf: concat_bytes_to_vec(data),
                };

                let event = NetworkEvent::Append(payload).with_tag(self.task_tag.clone());

                self.batch.push(event);
                self.uploaded.add(range);
            }
            Err(e) => {
                let error_event = NetworkEvent::Error(e.into()).with_tag(self.task_tag.clone());
                let _ = self.event_tx.send(error_event).await;
            }
        }
    }

    async fn flush_batch(&mut self) {
        let batch = std::mem::take(&mut self.batch);
        for event in batch {
            // 带背压感知的发送
            if let Err(e) = self.event_tx.send(event).await {
                // 处理通道关闭的情况
                break;
            }
        }
    }
}
