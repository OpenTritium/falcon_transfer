use super::FileRangeError;
use super::{FileMultiRange, FileRange};
use bytes::{Bytes, BytesMut};
use futures::FutureExt;
use futures::future::join_all;
use std::collections::BTreeMap;
use std::collections::HashMap;
use std::hash::Hasher;
use std::hint::likely;
use std::hint::unlikely;
use std::io::Result as IoResult;
use std::io::SeekFrom;
use std::ops::Bound;
use std::path::Path;
use std::usize;
use thiserror::Error;
use tokio::fs::{File, OpenOptions};
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};
use tokio::sync::Mutex;
use xxhash_rust::xxh3::Xxh3;

pub type Offset = usize;

// 来个接口用于抽象文件和缓存
#[derive(Debug, Error)]
pub enum HotFileError {
    #[error(transparent)]
    FileRangeError(#[from] FileRangeError),
    #[error(transparent)]
    IoError(#[from] std::io::Error),
}

pub struct HotFile {
    disk: Mutex<File>,
    dirty: Mutex<BTreeMap<FileRange, Bytes>>,
}

impl HotFile {
    // todo 优化初始化setlen
    pub async fn open_new<P: AsRef<Path>>(path: P) -> IoResult<Self> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(path)
            .await?;
        Ok(Self {
            disk: Mutex::new(file),
            dirty: Default::default(),
        })
    }

    pub async fn open_existed<P: AsRef<Path>>(path: P) -> IoResult<Self> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(path)
            .await?;
        Ok(Self {
            disk: Mutex::new(file),
            dirty: Default::default(),
        })
    }

    /// 保证不插入重叠的 range
    /// todo 锁优化
    pub async fn write(&self, buf: Bytes, offset: Offset) {
        let buf_len = buf.len();
        let buf_rgn = FileRange::new(offset, offset + buf_len);
        let left_bnd = Bound::Unbounded;
        let right_bnd = Bound::Included(FileRange::new(buf_rgn.end, usize::MAX));
        let overlapped = self
            .dirty
            .lock()
            .await
            .range((left_bnd, right_bnd))
            .filter(|(rgn, _)| rgn.intersect(&buf_rgn).is_some())
            .map(|(&rgn, buf)| (rgn, buf.clone()))
            .collect::<Vec<_>>();
        let merged_start = overlapped
            .iter()
            .map(|(r, _)| r.start)
            .fold(buf_rgn.start, |acc, s| acc.min(s));
        let merged_end = overlapped
            .iter()
            .map(|(r, _)| r.end)
            .fold(buf_rgn.end, |acc, e| acc.max(e));
        let merged_rgn = FileRange::new(merged_start, merged_end);
        let mut merged_buf = BytesMut::with_capacity(merged_rgn.interval());
        merged_buf.resize(merged_rgn.interval(), 0u8);
        for (rgn, buf) in &overlapped {
            let merged_start = rgn.start - merged_start;
            let merged_end = merged_start + rgn.interval();
            merged_buf[merged_start..merged_end].copy_from_slice(buf);
        }
        let merged_start = offset - merged_start;
        merged_buf[merged_start..merged_start + buf_len].copy_from_slice(&buf);
        let mut dirty_guard = self.dirty.lock().await;
        for (rgn, _) in overlapped {
            dirty_guard.remove(&rgn);
        }
        dirty_guard.insert(merged_rgn, merged_buf.freeze());
    }

    pub async fn sync(&self) -> IoResult<()> {
        let snapshot = {
            let dirty_guard = self.dirty.lock().await;
            if unlikely(dirty_guard.is_empty()) {
                return Ok(());
            }
            dirty_guard
                .iter()
                .map(|(&rgn, data)| (rgn, data.clone()))
                .collect::<Vec<_>>()
        };
        {
            let mut disk_guard = self.disk.lock().await;
            for (rgn, buf) in &snapshot {
                disk_guard.seek(SeekFrom::Start(rgn.start as u64)).await?;
                disk_guard.write_all(buf).await?;
            }
            disk_guard.sync_data().await?;
        }
        let mut dirty_guard = self.dirty.lock().await;
        for (rgn, _) in snapshot.iter() {
            dirty_guard.remove(rgn);
        }
        Ok(())
    }

    #[inline]
    async fn read_disk_by_range(&self, rgn: FileRange) -> IoResult<Bytes> {
        let mut disk_guard = self.disk.lock().await;
        disk_guard.seek(SeekFrom::Start(rgn.start as u64)).await?;
        let buf_len = rgn.interval();
        let mut buf = BytesMut::with_capacity(buf_len);
        buf.resize(buf_len, 0);
        disk_guard.read_exact(&mut buf).await?;
        Ok(buf.freeze())
    }

    pub async fn read(&self, mask: FileMultiRange) -> Result<Vec<Bytes>, HotFileError> {
        let mut rst = Vec::new();
        for sub_rgn in mask.as_ref() {
            let left_bnd = Bound::Unbounded;
            let right_bnd = Bound::Included(FileRange::new(sub_rgn.end, usize::MAX));
            let dirty_segs = self
                .dirty
                .lock()
                .await
                .range((left_bnd, right_bnd))
                .filter_map(|(drt_rgn, seg)| {
                    sub_rgn
                        .intersect(drt_rgn)
                        .map(|ovlp| (ovlp, seg.slice(ovlp.offset(drt_rgn.start).unwrap())))
                })
                .collect::<HashMap<_, _>>();

            let dirty_mask = FileMultiRange::try_from(
                dirty_segs.keys().copied().collect::<Vec<_>>().as_slice(),
            )?;
            let sub_mask: FileMultiRange = (*sub_rgn).into();
            let disk_mask = sub_mask.subtract(&dirty_mask);
            // 注意这里的迭代器顺序并不和需求一致
            let fut_iter = dirty_segs
                .into_iter()
                .map(|(rgn, data)| (rgn, BufferSource::Dirty(data)))
                .chain(disk_mask.inner.iter().map(|rgn| (*rgn, BufferSource::Disk)))
                .map(|(rgn, src)| async move {
                    (
                        rgn,
                        match src {
                            BufferSource::Dirty(data) => Ok(data),
                            BufferSource::Disk => self.read_disk_by_range(rgn).await,
                        },
                    )
                });
            let mut chunk = join_all(fut_iter)
                .await
                .into_iter()
                .map(|(rgn, rst)| rst.map(|buf| (rgn, buf)))
                .collect::<Result<BTreeMap<_, _>, _>>()?;
            rst.append(&mut chunk.values().cloned().collect());
        }
        Ok(rst)
    }

    async fn compute_hash(&self, buf_chunk: &[impl AsRef<[u8]>]) -> IoResult<u64> {
        let mut hasher = Xxh3::new();
        for buf in buf_chunk {
            hasher.update(buf.as_ref());
        }
        Ok(hasher.finish())
    }
}

/// 数据源标识
enum BufferSource {
    Dirty(Bytes),
    Disk,
}

#[cfg(test)]
mod tests {
    use core::slice::SlicePattern;

    use super::*;
    use bytes::Bytes;
    use tempfile::tempdir;
    use tokio::io::AsyncReadExt;

    #[tokio::test]
    async fn test_create_new_file() {
        let temp_dir = tempdir().unwrap();
        let file_path = temp_dir.path().join("new_file");

        // 成功创建新文件
        let hot_file = HotFile::open_new(&file_path).await.unwrap();

        // 重复创建应失败
        assert!(HotFile::open_new(&file_path).await.is_err());

        // 用open_existed打开应成功
        let _ = HotFile::open_existed(&file_path).await.unwrap();
    }

    #[tokio::test]
    async fn test_write_merge() {
        let temp_dir = tempdir().unwrap();
        let hot_file = HotFile::open_new(temp_dir.path().join("merge_test"))
            .await
            .unwrap();

        // 写入不重叠区域
        hot_file.write(Bytes::from("hello"), 0).await; // 0..5
        hot_file.write(Bytes::from("world"), 10).await; // 10..15
        {
            let dirty = hot_file.dirty.lock().await;
            assert_eq!(dirty.len(), 2);
        }

        // 写入重叠区域
        hot_file.write(Bytes::from("XXXX"), 8).await; // 8..12
        {
            let dirty = hot_file.dirty.lock().await;
            assert_eq!(dirty.len(), 2);
        }
    }

    #[tokio::test]
    async fn test_sync_to_disk() {
        let temp_dir = tempdir().unwrap();
        let file_path = temp_dir.path().join("sync_test");

        let hot_file = HotFile::open_new(&file_path).await.unwrap();

        // 写入并同步
        hot_file.write(Bytes::from("test data"), 0).await;
        hot_file.sync().await.unwrap();

        // 验证磁盘内容
        let mut file = File::open(&file_path).await.unwrap();
        let mut contents = Vec::new();
        file.read_to_end(&mut contents).await.unwrap();
        assert_eq!(contents, b"test data");

        // 验证dirty清理
        assert!(hot_file.dirty.lock().await.is_empty());
    }

    #[tokio::test]
    async fn test_read_combination() {
        let temp_dir = tempdir().unwrap();
        let file_path = temp_dir.path().join("read_test");

        // 初始化磁盘数据
        {
            let mut file = File::create(&file_path).await.unwrap();
            file.write_all(b"ABCDEFGHIJKL").await.unwrap();
            // ABCDEFGHIJKL
        }

        let hot_file = HotFile::open_existed(&file_path).await.unwrap();

        // 写入部分缓存
        hot_file.write(Bytes::from("1234"), 2).await; //2..6
        //AB1234GHIJKL
        hot_file.write(Bytes::from("zz"), 9).await; //9..11
        //AB1234GHIzzL

        // 构建读取范围: 0-12
        let mask = FileMultiRange::try_from([0..12].as_slice()).unwrap();
        let result = hot_file.read(mask).await.unwrap();
        assert_eq!(result.len(), 5);
        // 拼接结果
        let mut final_data = Vec::new();
        for chunk in result {
            final_data.extend_from_slice(chunk.as_slice());
        }

        // 验证数据组合正确
        assert_eq!(
            final_data,
            vec![
                b'A', b'B', // 0-2 (磁盘)
                b'1', b'2', b'3', b'4', // 2-6 (缓存)
                b'G', b'H', b'I', // 6-9 (磁盘)
                b'z', b'z', // 9-11 (缓存)
                b'L'  // 11-12 (磁盘)
            ]
        );
    }

    #[tokio::test]
    async fn test_complex_merge() {
        let temp_dir = tempdir().unwrap();
        let hot_file = HotFile::open_new(temp_dir.path().join("complex_merge"))
            .await
            .unwrap();

        // 初始写入
        hot_file.write(Bytes::from("hello"), 0).await; // 0..5
        hot_file.write(Bytes::from("world"), 3).await; // 3..8
        hot_file.write(Bytes::from("rust"), 7).await; // 7..11

        // 验证合并结果
        {
            let dirty = hot_file.dirty.lock().await;
            assert_eq!(dirty.len(), 1);
            let (range, data) = dirty.iter().next().unwrap();
            assert_eq!(range.start, 0);
            assert_eq!(range.end, 11);
            assert_eq!(data.as_ref(), b"helworlrust");
        }

        // 同步并验证磁盘内容
        hot_file.sync().await.unwrap();
        let mut file = File::open(temp_dir.path().join("complex_merge"))
            .await
            .unwrap();
        let mut contents = Vec::new();
        file.read_to_end(&mut contents).await.unwrap();
        assert_eq!(contents, b"helworlrust");
    }
}
