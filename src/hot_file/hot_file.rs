use super::{FileMultiRange, FileRange, StackAllocatedPefered};
use bytes::{Bytes, BytesMut};
use dashmap::DashMap;
use futures::FutureExt;
use std::collections::BTreeMap;
use std::hash::Hasher;
use std::hint::unlikely;
use std::io::Result as IoResult;
use std::io::SeekFrom;
use std::{path::Path, sync::Arc};
use tokio::fs::{File, OpenOptions};
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};
use tokio::sync::Mutex;
use xxhash_rust::xxh3::Xxh3;

pub type Offset = usize;

// 来个接口用于抽象文件和缓存

pub struct HotFile {
    disk: Mutex<File>,
    dirty: Mutex<BTreeMap<FileRange, Bytes>>,
}

impl HotFile {
    // todo 优化初始化setlen
    pub async fn open<P: AsRef<Path>>(path: P) -> IoResult<Self> {
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

    pub async fn write(&self, buf: Bytes, offset: Offset) {
        self.dirty.lock().await.insert(
            FileRange {
                start: offset,
                end: offset + buf.len(),
            },
            buf,
        );
    }

    pub async fn sync(&self) -> IoResult<()> {
        let mut dirty_guard = self.dirty.lock().await;
        if unlikely(dirty_guard.is_empty()) {
            return Ok(());
        }
        let mut file_guard = self.disk.lock().await;
        let tmp = std::mem::take(&mut *dirty_guard);
        for (rgn, buf) in tmp.into_iter() {
            file_guard.seek(SeekFrom::Start(rgn.start as u64)).await?;
            file_guard.write_all(&buf).await?;
        }
        file_guard.sync_all().await?;
        Ok(())
    }

    #[inline]
    async fn read_file_by_rangify(&self, itv: FileRange) -> IoResult<Bytes> {
        let mut file_guard = self.disk.lock().await;
        file_guard.seek(SeekFrom::Start(itv.start as u64)).await?;
        let mut buf = BytesMut::with_capacity(itv.len());
        file_guard.read_buf(&mut buf).await?;
        Ok(buf.freeze())
    }

    pub async fn read(&self, mask: FileMultiRange) -> IoResult<Vec<Bytes>> {
        let mut result = Vec::new();
        for itv in mask.inner {
            let dirty_blocks: BTreeMap<FileRange, Bytes> = self
                .dirty
                .iter()
                .filter_map(|entry| {
                    let (dirty_range, data) = entry.pair();
                    dirty_range.intersect(&itv).map(|overlap| {
                        let slice = data.slice(
                            (overlap.start - dirty_range.start)..(overlap.end - dirty_range.start),
                        );
                        (overlap, slice)
                    })
                })
                .collect();
            let full_mask: FileMultiRange = itv.into();
            let dirty_mask = FileMultiRange::new(
                dirty_blocks
                    .keys()
                    .cloned()
                    .collect::<StackAllocatedPefered>()
                    .as_slice(),
            );
            let disk_mask = full_mask.subtract(&dirty_mask);
            let mut segments = dirty_blocks
                .into_iter()
                .map(|(itv, data)| (itv, Source::Dirty(data)))
                .collect::<BTreeMap<_, _>>();
            segments.extend(
                disk_mask
                    .inner
                    .iter()
                    .map(|itv| (*itv, Source::Disk))
                    .collect::<BTreeMap<_, _>>(),
            );
            let mut data_futures = Vec::new();
            for (range, source) in &segments {
                match source {
                    Source::Dirty(data) => {
                        data_futures.push(async { Ok(data.clone()) }.boxed());
                    }
                    Source::Disk => {
                        data_futures
                            .push(async move { self.read_file_by_rangify(*range).await }.boxed());
                    }
                }
            }
            let mut chunk_results = Vec::with_capacity(data_futures.len());
            for future in data_futures {
                chunk_results.push(future.await?);
            }
            result.extend(chunk_results);
        }
        Ok(result)
    }

    async fn compute_hash(&self, mask: FileMultiRange) -> IoResult<u64> {
        let chunks = self.read(mask).await?;
        let mut hasher = Xxh3::new();
        for chunk in chunks {
            hasher.update(chunk.as_ref());
        }
        Ok(hasher.finish())
    }
}

/// 数据源标识
enum Source {
    Dirty(Bytes),
    Disk,
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;
    use xxhash_rust::xxh3::xxh3_64;

    use super::*;

    fn concat_bytes(chunks: Vec<Bytes>) -> Bytes {
        chunks
            .iter()
            .fold(BytesMut::new(), |mut acc, chunk| {
                acc.extend_from_slice(chunk);
                acc
            })
            .freeze()
    }
    #[tokio::test]
    async fn read_entirely_from_disk() {
        let temp_dir = tempdir().unwrap();
        let path = temp_dir.path().join("test1");

        // 初始化文件内容
        tokio::fs::write(&path, b"abcdefghij").await.unwrap();

        let hot_file = HotFile::open(&path).await.unwrap();
        let mask = FileMultiRange::new(vec![rangify!(0..10).unwrap()].as_slice());
        let result = hot_file.read(mask).await.unwrap();

        assert_eq!(concat_bytes(result), Bytes::from("abcdefghij"));
    }

    #[tokio::test]
    async fn read_entirely_from_dirty() {
        let temp_dir = tempdir().unwrap();
        let hot_file = HotFile::open(temp_dir.path().join("test2")).await.unwrap();

        // 写入脏数据覆盖整个区间
        hot_file.write(Bytes::from("12345"), 0);
        let mask = FileMultiRange::new(vec![rangify!(0..5).unwrap()].as_slice());
        let result = hot_file.read(mask).await.unwrap();

        assert_eq!(concat_bytes(result), Bytes::from("12345"));
    }

    #[tokio::test]
    async fn read_mixed_dirty_and_disk() {
        let temp_dir = tempfile::tempdir().unwrap();
        let path = temp_dir.path().join("test3");

        // 初始化文件内容
        tokio::fs::write(&path, b"abcdefghij").await.unwrap();
        let hot_file = HotFile::open(&path).await.unwrap();

        // 写入部分脏数据
        hot_file.write(Bytes::from("123"), 2); // 覆盖 2-5
        hot_file.write(Bytes::from("45"), 7); // 覆盖 7-9

        let mask = FileMultiRange::new(vec![rangify!(0..10).unwrap()].as_slice());
        let result = hot_file.read(mask).await.unwrap();

        // 预期结果: ab(disk) + 123(dirty) + fg(disk) + 45(dirty) + j(disk)
        assert_eq!(concat_bytes(result), Bytes::from("ab123fg45j"));
    }

    #[tokio::test]
    async fn read_multiple_rangifys() {
        let temp_dir = tempfile::tempdir().unwrap();
        let path = temp_dir.path().join("test4");

        tokio::fs::write(&path, b"abcdefghijklmnopqrst")
            .await
            .unwrap();

        let hot_file = HotFile::open(&path).await.unwrap();

        // 写入两个脏块
        hot_file.write(Bytes::from("123"), 3); // 3-6
        hot_file.write(Bytes::from("45"), 8); // 8-10

        let mask = FileMultiRange::new(
            vec![
                rangify!(0..4).unwrap(),  // 0-4 (0-3 clean + 3-4 dirty)
                rangify!(6..10).unwrap(), // 6-8 clean + 8-10 dirty
            ]
            .as_slice(),
        );

        let result = hot_file.read(mask).await.unwrap();

        assert_eq!(concat_bytes(result), Bytes::from("abc1gh45"));
    }

    #[tokio::test]
    async fn test_full_disk_hash() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test1");

        let original_data = b"abcdefghijklmnopqrstuvwxyz";
        tokio::fs::write(&path, original_data).await.unwrap();

        let hot_file = Arc::new(HotFile::open(&path).await.unwrap());
        let mask = FileMultiRange::new(vec![rangify!(0..26).unwrap()].as_slice());

        let computed = hot_file.compute_hash(mask).await.unwrap();
        let expected = xxh3_64(original_data);

        assert_eq!(computed, expected);
    }

    #[tokio::test]
    async fn test_dirty_data_override() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test2");

        tokio::fs::write(&path, b"base_data").await.unwrap();
        let hot_file = Arc::new(HotFile::open(&path).await.unwrap());

        // 写入覆盖全区的脏数据
        hot_file.write(Bytes::from("new_data"), 0);
        let mask = FileMultiRange::new(vec![rangify!(0..8).unwrap()].as_slice());

        let computed = hot_file.compute_hash(mask).await.unwrap();
        let expected = xxh3_64(b"new_data");

        assert_eq!(computed, expected);
    }

    #[tokio::test]
    async fn test_merged_hash() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test3");

        tokio::fs::write(&path, b"hello_world").await.unwrap();
        let hot_file = Arc::new(HotFile::open(&path).await.unwrap());

        // 写入两个脏块
        hot_file.write(Bytes::from("HELLO"), 0); // 覆盖 0..5
        hot_file.write(Bytes::from("D"), 10); // 覆盖 10

        let mask = FileMultiRange::new(vec![rangify!(0..11).unwrap()].as_slice());
        let computed = hot_file.compute_hash(mask).await.unwrap();

        let expected = xxh3_64(b"HELLO_worlD");
        assert_eq!(computed, expected);
    }

    #[tokio::test]
    async fn test_sparse_rangifys() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test4");

        tokio::fs::write(&path, b"abcdefghijklmnopqrst")
            .await
            .unwrap();
        let hot_file = Arc::new(HotFile::open(&path).await.unwrap());

        // 写入不连续脏块
        hot_file.write(Bytes::from("123"), 3); // 3-6
        hot_file.write(Bytes::from("45"), 8); // 8-10

        let mask = FileMultiRange::new(
            vec![
                rangify!(0..4).unwrap(),  // 0-3 clean + 3-4 dirty
                rangify!(6..10).unwrap(), // 6-8 clean + 8-10 dirty
            ]
            .as_slice(),
        );

        let computed = hot_file.compute_hash(mask).await.unwrap();

        // 预期数据组合：abc1 + gh45
        let expected = xxh3_64(b"abc1gh45");
        assert_eq!(computed, expected);
    }

    #[tokio::test]
    async fn sync_basic_operation() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test_sync_basic");

        // 初始化对象
        let hot_file = HotFile::open(&path).await.unwrap();

        // 写入脏数据并同步
        hot_file.write(Bytes::from("hello"), 0);
        hot_file.sync().await.unwrap();

        // 验证文件内容
        let content = tokio::fs::read(&path).await.unwrap();
        assert_eq!(content, b"hello");

        // 检查脏数据已清除
        assert!(hot_file.dirty.is_empty());
    }

    #[tokio::test]
    async fn sync_multiple_blocks() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test_sync_multiple");

        let hot_file = HotFile::open(&path).await.unwrap();

        // 写入多个不连续区块
        hot_file.write(Bytes::from("head"), 0);
        hot_file.write(Bytes::from("tail"), 10);
        hot_file.sync().await.unwrap();

        // 验证文件内容
        let content = tokio::fs::read(&path).await.unwrap();
        assert_eq!(&content[0..4], b"head");
        assert_eq!(&content[10..14], b"tail");
    }

    #[tokio::test]
    async fn sync_overlapping_writes() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test_overlap");

        let hot_file = HotFile::open(&path).await.unwrap();

        // 第一次写入
        hot_file.write(Bytes::from("aaaaa"), 0);
        hot_file.sync().await.unwrap();

        // 覆盖写入
        hot_file.write(Bytes::from("bbbbb"), 0);
        hot_file.sync().await.unwrap();

        let content = tokio::fs::read(&path).await.unwrap();
        assert_eq!(content, b"bbbbb");
    }

    #[tokio::test]
    async fn sync_with_concurrent_writes() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test_concurrent");

        let hot_file = Arc::new(HotFile::open(&path).await.unwrap());

        // 初始同步
        hot_file.write(Bytes::from("base"), 0);
        hot_file.sync().await.unwrap();

        // 同步后写入新数据
        hot_file.write(Bytes::from("new"), 5);
        hot_file.sync().await.unwrap();

        let content = tokio::fs::read(&path).await.unwrap();
        assert_eq!(&content[0..4], b"base");
        assert_eq!(&content[5..8], b"new");
    }

    #[tokio::test]
    async fn sync_empty_dirty() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test_empty");

        let hot_file = HotFile::open(&path).await.unwrap();

        // 空同步不应报错
        let result = hot_file.sync().await;
        assert!(result.is_ok());

        // 文件应保持空状态
        let metadata = tokio::fs::metadata(&path).await.unwrap();
        assert_eq!(metadata.len(), 0);
    }
}
