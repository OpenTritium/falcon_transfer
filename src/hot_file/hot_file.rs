use super::FileRangeError;
use super::{FileMultiRange, FileRange};
use bytes::{Bytes, BytesMut};
use futures::FutureExt;
use futures::future::{join_all, try_join_all};
use std::collections::BTreeMap;
use std::collections::HashMap;
use std::hash::Hasher;
use std::hint::likely;
use std::hint::unlikely;
use std::io::Result as IoResult;
use std::io::SeekFrom;
use std::ops::Bound;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::usize;
use thiserror::Error;
use tokio::fs::{File, OpenOptions};
use tokio::io::{self, AsyncReadExt, AsyncSeekExt, AsyncWriteExt};
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
    #[error("Reading bytes beyond the file boundary.")]
    OutOfFile,
}

pub struct HotFile {
    disk: Mutex<File>,
    dirty: Mutex<BTreeMap<FileRange, Bytes>>,
    pub sync_len_state: AtomicUsize,
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
        let len = file.metadata().await?.len() as usize;
        Ok(Self {
            disk: Mutex::new(file),
            dirty: Default::default(),
            sync_len_state: AtomicUsize::new(len),
        })
    }

    pub async fn open_existed<P: AsRef<Path>>(path: P) -> IoResult<Self> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(path)
            .await?;
        let len = file.metadata().await?.len() as usize;
        Ok(Self {
            disk: Mutex::new(file),
            dirty: Default::default(),
            sync_len_state: AtomicUsize::new(len),
        })
    }

    /// 保证不插入重叠的 range
    /// todo 锁优化
    pub async fn write(&self, buf: Bytes, offset: Offset) -> Result<(), HotFileError> {
        let buf_len = buf.len();
        let buf_rgn = FileRange::try_new(offset, offset + buf_len)?;
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
        self.sync_len_state
            .fetch_max(merged_rgn.end, Ordering::Relaxed);
        let merged_start = offset - merged_start;
        merged_buf[merged_start..merged_start + buf_len].copy_from_slice(&buf);
        let mut dirty_guard = self.dirty.lock().await;
        for (rgn, _) in overlapped {
            dirty_guard.remove(&rgn);
        }
        dirty_guard.insert(merged_rgn, merged_buf.freeze());
        Ok(())
    }

    pub async fn sync(&self) -> IoResult<()> {
        let dirty_guard = self.dirty.lock().await;
        if unlikely(dirty_guard.is_empty()) {
            return Ok(());
        }
        let target_len = self.sync_len_state.load(Ordering::Relaxed);
        let snapshot = dirty_guard
            .iter()
            .map(|(&rgn, data)| (rgn, data.clone()))
            .collect::<Vec<_>>();
        drop(dirty_guard);
        let mut disk_guard = self.disk.lock().await;
        if disk_guard.metadata().await?.len() < target_len as u64 {
            disk_guard.set_len(target_len as u64).await?;
        }
        for (rgn, buf) in &snapshot {
            disk_guard.seek(SeekFrom::Start(rgn.start as u64)).await?;
            disk_guard.write_all(buf).await?;
        }
        disk_guard.sync_all().await?;
        drop(disk_guard);
        let mut dirty_guard = self.dirty.lock().await;
        for (rgn, _) in snapshot.iter() {
            dirty_guard.remove(rgn);
        }
        Ok(())
    }

    async fn read_disk_by_range(&self, rgn: FileRange) -> Result<Bytes, HotFileError> {
        let logical_len = self.sync_len_state.load(Ordering::Relaxed);
        if rgn.end > logical_len {
            return Err(HotFileError::OutOfFile);
        }

        let mut disk_guard = self.disk.lock().await;
        let disk_len = disk_guard.metadata().await?.len() as usize;

        // Calculate the actual part of the range that exists on disk
        let read_start = rgn.start;
        let read_end = disk_len.min(rgn.end);
        let read_rgn = FileRange::new(read_start, read_end);

        // Prepare buffer initialized with zeros
        let mut buf = BytesMut::with_capacity(rgn.interval());
        buf.resize(rgn.interval(), 0);

        if read_rgn.interval() > 0 {
            disk_guard
                .seek(SeekFrom::Start(read_rgn.start as u64))
                .await?;
            disk_guard
                .read_exact(&mut buf[0..read_rgn.interval()])
                .await?;
        }

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
                    match src {
                        BufferSource::Dirty(buf) => Ok((rgn, buf)),
                        BufferSource::Disk => match self.read_disk_by_range(rgn).await {
                            Ok(buf) => Ok((rgn, buf)),
                            Err(e) => Err(e),
                        },
                    }
                });
            let chunk = try_join_all(fut_iter)
                .await?
                .into_iter()
                .collect::<BTreeMap<_, _>>();
            rst.append(&mut chunk.values().cloned().collect()); // 可能会造成重复
        }
        Ok(rst)
    }

    async fn hash(&self, buf_chunk: &[impl AsRef<[u8]>]) -> IoResult<u64> {
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
    use super::*;
    use bytes::Bytes;
    use tempfile::tempdir;
    use tokio::io::AsyncReadExt;

    #[tokio::test]
    async fn test_create_new_file() {
        let temp_dir = tempdir().unwrap();
        let file_path = temp_dir.path().join("new_file");

        // 成功创建新文件
        let _ = HotFile::open_new(&file_path).await.unwrap();

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
        let _ = hot_file.write(Bytes::from("hello"), 0).await; // 0..5
        let _ = hot_file.write(Bytes::from("world"), 10).await; // 10..15
        {
            let dirty = hot_file.dirty.lock().await;
            assert_eq!(dirty.len(), 2);
        }

        // 写入重叠区域
        let _ = hot_file.write(Bytes::from("XXXX"), 8).await; // 8..12
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
        let _ = hot_file.write(Bytes::from("test data"), 0).await;
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
        let _ = hot_file.write(Bytes::from("1234"), 2).await; //2..6
        //AB1234GHIJKL
        let _ = hot_file.write(Bytes::from("zz"), 9).await; //9..11
        //AB1234GHIzzL

        // 构建读取范围: 0-12
        let mask = FileMultiRange::try_from([0..12].as_slice()).unwrap();
        let result = hot_file.read(mask).await.unwrap();
        assert_eq!(result.len(), 5);
        // 拼接结果
        let mut final_data = Vec::new();
        for chunk in result {
            final_data.extend_from_slice(&chunk);
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
        let _ = hot_file.write(Bytes::from("hello"), 0).await; // 0..5
        let _ = hot_file.write(Bytes::from("world"), 3).await; // 3..8
        let _ = hot_file.write(Bytes::from("rust"), 7).await; // 7..11

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

    #[tokio::test]
    async fn test_write_full_overlap() {
        let temp_dir = tempdir().unwrap();
        let hot_file = HotFile::open_new(temp_dir.path().join("full_overlap"))
            .await
            .unwrap();

        // 初始写入 0..5
        let _ = hot_file.write(Bytes::from("hello"), 0).await;
        // 完全覆盖写入 0..5
        let _ = hot_file.write(Bytes::from("world"), 0).await;

        {
            let dirty = hot_file.dirty.lock().await;
            assert_eq!(dirty.len(), 1);
            let (range, data) = dirty.iter().next().unwrap();
            assert_eq!(range, &FileRange::new(0, 5));
            assert_eq!(data.as_ref(), b"world");
        }
    }

    #[tokio::test]
    async fn test_read_beyond_file_length() {
        let temp_dir = tempdir().unwrap();
        let file_path = temp_dir.path().join("read_beyond");

        // 初始文件内容为5字节 "hello"
        let hot_file = HotFile::open_new(&file_path).await.unwrap();
        let _ = hot_file.write(Bytes::from("hello"), 0).await;
        hot_file.sync().await.unwrap();

        // 尝试读取 0..10 (超过文件长度)
        let mask = FileMultiRange::try_from([0..10].as_slice()).unwrap();
        let result = hot_file.read(mask).await;

        // 根据实现，可能返回错误或截断数据
        // 假设实现中允许读取超出部分，但磁盘读取会失败
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_concurrent_write() {
        let temp_dir = tempdir().unwrap();
        let hot_file = std::sync::Arc::new(
            HotFile::open_new(temp_dir.path().join("concurrent_write"))
                .await
                .unwrap(),
        );

        let handles: Vec<_> = (0..10)
            .map(|i| {
                let hot_file = hot_file.clone();
                tokio::spawn(async move {
                    let _ = hot_file
                        .write(Bytes::from(format!("block{}", i)), i * 10)
                        .await;
                })
            })
            .collect();

        futures::future::join_all(handles).await;

        // 验证所有写入都被合并或独立存在
        let dirty = hot_file.dirty.lock().await;
        // 因为每个写入间隔10字节，互不重叠，应有10个独立块
        assert_eq!(dirty.len(), 10);
    }

    #[tokio::test]
    async fn test_hash_calculation() {
        let temp_dir = tempdir().unwrap();
        let hot_file = HotFile::open_new(temp_dir.path().join("hash_test"))
            .await
            .unwrap();

        let data1 = Bytes::from("hello");
        let data2 = Bytes::from("world");
        let hash1 = hot_file.hash(&[&data1]).await.unwrap();
        let hash2 = hot_file.hash(&[&data2]).await.unwrap();
        let hash_combined = hot_file.hash(&[&data1, &data2]).await.unwrap();

        let mut hasher = Xxh3::new();
        hasher.update(b"hello");
        let expected_hash1 = hasher.finish();
        hasher.update(b"world");
        let expected_hash_combined = hasher.finish();

        assert_eq!(hash1, expected_hash1);
        assert_eq!(hash_combined, expected_hash_combined);
        assert_ne!(hash1, hash2);
    }

    #[tokio::test]
    async fn test_multiple_syncs() {
        let temp_dir = tempdir().unwrap();
        let file_path = temp_dir.path().join("multiple_syncs");

        let hot_file = HotFile::open_new(&file_path).await.unwrap();

        // 第一次写入和同步
        let _ = hot_file.write(Bytes::from("test1"), 0).await;
        hot_file.sync().await.unwrap();

        // 验证第一次同步
        let mut file = File::open(&file_path).await.unwrap();
        let mut contents = vec![0u8; 5];
        file.read_exact(&mut contents).await.unwrap();
        assert_eq!(contents, b"test1");

        // 第二次写入和同步
        let _ = hot_file.write(Bytes::from("test2"), 5).await;
        hot_file.sync().await.unwrap();

        // 验证第二次同步
        let mut contents = vec![0u8; 10];
        file.seek(SeekFrom::Start(0)).await.unwrap();
        file.read_exact(&mut contents).await.unwrap();
        assert_eq!(&contents[..5], b"test1");
        assert_eq!(&contents[5..10], b"test2");
    }

    #[tokio::test]
    async fn test_write_zero_length() {
        let temp_dir = tempdir().unwrap();
        let hot_file = HotFile::open_new(temp_dir.path().join("zero_length"))
            .await
            .unwrap();

        // 尝试写入0字节
        let _ = hot_file.write(Bytes::new(), 0).await;

        {
            let dirty = hot_file.dirty.lock().await;
            assert!(dirty.is_empty(), "0长度写入不应产生脏数据");
        }
    }

    #[tokio::test]
    async fn test_read_complex_ranges() {
        let temp_dir = tempdir().unwrap();
        let file_path = temp_dir.path().join("complex_ranges");

        // 初始化文件内容: 0..12 = "ABCDEFGHIJKL"
        let hot_file = HotFile::open_new(&file_path).await.unwrap();
        let _ = hot_file.write(Bytes::from("ABCDEFGHIJKL"), 0).await;
        hot_file.sync().await.unwrap();

        // 写入多个脏数据块
        let _ = hot_file.write(Bytes::from("1234"), 2).await; // 2..6
        let _ = hot_file.write(Bytes::from("zz"), 9).await; // 9..11
        let _ = hot_file.write(Bytes::from("X"), 15).await; // 15..16

        // 构建复杂读取范围：0..3, 5..8, 10..16
        let mask = FileMultiRange::try_from([0..3, 5..8, 10..16].as_slice()).unwrap();
        let result = hot_file.read(mask).await.unwrap();
        // AB CDEF GHI JK L0000
        // 00 1234 000 zz 0000X
        let expected = vec![
            Bytes::from_static(b"AB"),   // 0..2 from DISK
            Bytes::from_static(b"1"),    // 2..3 from DIRTY
            Bytes::from_static(b"4"),    // 5..6 from DIRTY
            Bytes::from_static(b"GH"),  // 6..8 from DISK
            Bytes::from_static(b"z"),    // 10..11 from DISK
            Bytes::from_static(b"L\0\0\0"),    // 11..12 from DISK
            Bytes::from_static(b"X"), // 12..16 from DIRTY
        ];

        assert_eq!(result.len(), expected.len());
        for (actual, expected) in result.iter().zip(expected.iter()) {
            assert_eq!(actual, expected);
        }
    }
}

