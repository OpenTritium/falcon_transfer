use smallvec::{SmallVec, smallvec};
use std::{
    cmp::Ordering,
    hint::{likely, unlikely},
    ops::{Bound, Range, RangeInclusive},
};
use thiserror::Error;

pub const STACK_BUFFERED: usize = 8;

#[derive(Debug, Error, PartialEq)]
pub enum FileRangeError {
    #[error("Invalid range: {start:?} - {end:?}")]
    InvalidRange {
        start: Bound<usize>,
        end: Bound<usize>,
    },
    #[error("Index overflow")]
    IndexOverflow,
    #[error("Index out of bounds")]
    IndexUnbounded,
}

#[derive(Debug, PartialEq, Clone, Hash, Copy, Eq)]
pub struct FileRange {
    pub start: usize,
    pub end: usize,
}

impl FileRange {
    #[inline]
    pub fn try_new(start: usize, end: usize) -> Option<Self> {
        likely(start < end).then(|| Self { start, end })
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.end - self.start
    }

    #[inline]
    pub fn intersect(&self, other: &Self) -> Option<Self> {
        let start = self.start.max(other.start);
        let end = self.end.min(other.end);
        likely(start < end).then(|| Self { start, end })
    }

    #[inline]
    pub fn union(&self, other: &Self) -> Option<Self> {
        likely(self.end >= other.start && other.end >= self.start).then(|| Self {
            start: self.start.min(other.start),
            end: self.end.max(other.end),
        })
    }

    #[inline]
    pub fn subtract(&self, other: &Self) -> [Option<FileRange>; 2] {
        let Some(intersection) = self.intersect(other) else {
            return [Some(*self), None];
        };
        let a_start = self.start;
        let a_end = self.end;
        let b_start = intersection.start;
        let b_end = intersection.end;
        [
            (a_start < b_start).then(|| FileRange {
                start: a_start,
                end: b_start,
            }),
            (a_end > b_end).then(|| FileRange {
                start: b_end,
                end: a_end,
            }),
        ]
    }

    #[inline]
    pub const fn contains(&self, other: &Self) -> bool {
        self.start <= other.start && self.end >= other.end
    }
}

impl From<FileRange> for FileMultiRange {
    #[inline]
    fn from(rgn: FileRange) -> Self {
        Self {
            inner: smallvec![rgn],
        }
    }
}

impl From<FileRange> for Range<usize> {
    #[inline]
    fn from(rgn: FileRange) -> Self {
        rgn.start..rgn.end
    }
}

impl From<FileRange> for RangeInclusive<usize> {
    #[inline]
    fn from(rgn: FileRange) -> Self {
        rgn.start..=rgn.end - 1
    }
}

impl PartialOrd for FileRange {
    #[inline]
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for FileRange {
    #[inline]
    fn cmp(&self, other: &Self) -> Ordering {
        self.start.cmp(&other.start)
    }
}

impl TryFrom<Range<usize>> for FileRange {
    type Error = FileRangeError;
    #[inline]
    fn try_from(rgn: Range<usize>) -> Result<Self, Self::Error> {
        let (start, end) = extract_range_bounds(&rgn)?;
        Ok(FileRange { start, end })
    }
}

impl TryFrom<RangeInclusive<usize>> for FileRange {
    type Error = FileRangeError;
    #[inline]
    fn try_from(rgn: RangeInclusive<usize>) -> Result<Self, Self::Error> {
        let (start, end) = extract_range_bounds(&rgn)?;
        Ok(FileRange { start, end })
    }
}

impl TryFrom<(Bound<usize>, Bound<usize>)> for FileRange {
    type Error = FileRangeError;
    #[inline]
    fn try_from(rgn: (Bound<usize>, Bound<usize>)) -> Result<Self, Self::Error> {
        let (start, end) = extract_range_bounds(&rgn)?;
        Ok(FileRange { start, end })
    }
}

impl FileRange {
    #[inline]
    pub fn get<'a>(&self, slice: &'a [u8]) -> Option<&'a [u8]> {
        if self.start <= self.end && self.end <= slice.len() {
            Some(&slice[self.start..self.end])
        } else {
            None
        }
    }

    #[inline]
    pub fn get_mut<'a>(&self, slice: &'a mut [u8]) -> Option<&'a mut [u8]> {
        if self.start <= self.end && self.end <= slice.len() {
            Some(&mut slice[self.start..self.end])
        } else {
            None
        }
    }

    #[inline]
    pub fn index<'a>(&self, slice: &'a [u8]) -> &'a [u8] {
        &slice[self.start..self.end]
    }

    #[inline]
    pub fn index_mut<'a>(&self, slice: &'a mut [u8]) -> &'a mut [u8] {
        &mut slice[self.start..self.end]
    }
}

pub trait ToRangeBoundPair {
    fn to_bound_pair(&self) -> (Bound<usize>, Bound<usize>);
}

impl ToRangeBoundPair for Range<usize> {
    #[inline(always)]
    fn to_bound_pair(&self) -> (Bound<usize>, Bound<usize>) {
        (Bound::Included(self.start), Bound::Excluded(self.end))
    }
}

impl ToRangeBoundPair for RangeInclusive<usize> {
    #[inline(always)]
    fn to_bound_pair(&self) -> (Bound<usize>, Bound<usize>) {
        (Bound::Included(*self.start()), Bound::Included(*self.end()))
    }
}

impl ToRangeBoundPair for (Bound<usize>, Bound<usize>) {
    #[inline(always)]
    fn to_bound_pair(&self) -> (Bound<usize>, Bound<usize>) {
        (self.0, self.1)
    }
}

impl ToRangeBoundPair for FileRange {
    #[inline(always)]
    fn to_bound_pair(&self) -> (Bound<usize>, Bound<usize>) {
        (Bound::Included(self.start), Bound::Excluded(self.end))
    }
}
impl ToRangeBoundPair for (usize, usize) {
    #[inline(always)]
    fn to_bound_pair(&self) -> (Bound<usize>, Bound<usize>) {
        (Bound::Included(self.0), Bound::Excluded(self.1))
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct FileMultiRange {
    pub inner: SmallVec<[FileRange; STACK_BUFFERED]>,
}

impl FileMultiRange {
    #[inline]
    pub fn new() -> Self {
        Self {
            inner: SmallVec::new(),
        }
    }

    pub fn add_checked(&mut self, start: usize, end: usize) -> Result<(), FileRangeError> {
        let range = FileRange::try_new(start, end).ok_or(FileRangeError::InvalidRange {
            start: Bound::Included(start),
            end: Bound::Excluded(end),
        })?;
        if unlikely(self.inner.is_empty()) {
            self.inner.push(range);
            return Ok(());
        }
        let left = self.inner.partition_point(|r| r.end < range.start);
        let right = self.inner.partition_point(|r| r.start <= range.end);
        unsafe {
            if likely(left < right) {
                let ranges = self.inner.as_mut_ptr();
                let first = &mut *ranges.add(left);
                first.start = first.start.min(range.start);
                let last = &*ranges.add(right - 1);
                first.end = last.end.max(range.end);
                if right - left > 1 {
                    let tail = self.inner.len() - right;
                    std::ptr::copy(ranges.add(right), ranges.add(left + 1), tail);
                    self.inner.set_len(left + 1 + tail);
                }
            } else {
                self.inner.insert(left, range);
            }
        }
        Ok(())
    }

    #[inline]
    pub fn intersect(&self, other: &Self) -> Self {
        let mut result = Self::new();
        let (mut a_iter, mut b_iter) =
            (self.inner.iter().peekable(), other.inner.iter().peekable());
        while let (Some(a), Some(b)) = (a_iter.peek(), b_iter.peek()) {
            let start = a.start.max(b.start);
            let end = a.end.min(b.end);
            if likely(start < end) {
                result.inner.push(FileRange { start, end });
            }
            use std::cmp::Ordering::*;
            match a.end.cmp(&b.end) {
                Less => {
                    a_iter.next();
                }
                Greater => {
                    b_iter.next();
                }
                _ => {
                    a_iter.next();
                    b_iter.next();
                }
            }
        }
        result
    }

    #[inline]
    pub fn subtract(&self, other: &Self) -> Self {
        let mut result = Self::new();
        let mut other_iter = other.inner.iter().peekable();
        for &range in &self.inner {
            let mut current = range;
            while let Some(&&sub) = other_iter.peek() {
                if likely(sub.end <= current.start) {
                    other_iter.next();
                    continue;
                }
                if unlikely(sub.start >= current.end) {
                    break;
                }
                if likely(current.start < sub.start) {
                    result.inner.push(FileRange {
                        start: current.start,
                        end: sub.start,
                    });
                }
                current.start = current.start.max(sub.end);
                if unlikely(current.start >= current.end) {
                    break;
                }
                if likely(sub.end > current.end) {
                    break;
                }
                other_iter.next();
            }
            if likely(current.start < current.end) {
                result.inner.push(current);
            }
        }
        result
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.inner.len()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    #[inline]
    pub fn total_len(&self) -> usize {
        self.inner.iter().map(|r| r.len()).sum()
    }
}

impl<T> TryFrom<&[T]> for FileMultiRange
where
    T: ToRangeBoundPair,
{
    type Error = FileRangeError;
    #[inline]
    fn try_from(ranges: &[T]) -> Result<Self, Self::Error> {
        let mut rgns = Self::new();
        for range in ranges {
            let (start, end) = extract_range_bounds(range)?;
            rgns.add_checked(start, end)?;
        }
        Ok(rgns)
    }
}

#[inline(always)]
fn extract_range_bounds(rgn: &impl ToRangeBoundPair) -> Result<(usize, usize), FileRangeError> {
    use Bound::*;
    use FileRangeError::*;
    let (start, end) = rgn.to_bound_pair();
    let start = match start {
        Included(s) => Ok(s),
        Excluded(s) => s.checked_add(1).ok_or(IndexOverflow),
        Unbounded => Err(IndexUnbounded),
    }?;
    let end = match end {
        Included(e) => e.checked_add(1).ok_or(IndexOverflow),
        Excluded(e) => Ok(e),
        Unbounded => Err(IndexUnbounded),
    }?;
    if likely(start < end) {
        Ok((start, end))
    } else {
        Err(InvalidRange {
            start: Included(start),
            end: Excluded(end),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use Bound::*;
    use smallvec::smallvec_inline;

    // FileRange 基础测试
    #[test]
    fn filerange_basics() {
        assert_eq!(FileRange::try_new(1, 3).unwrap().len(), 2);
        assert!(FileRange::try_new(5, 5).is_none());
    }

    // 边界条件测试
    #[test]
    fn edge_cases() {
        // 最小有效范围
        assert_eq!(
            FileRange::try_new(0, 1),
            Some(FileRange { start: 0, end: 1 })
        );

        // 最大值边界
        let max = usize::MAX;
        assert_eq!(
            FileRange::try_new(max - 1, max),
            Some(FileRange {
                start: max - 1,
                end: max
            })
        );
    }

    // 类型转换测试
    #[test]
    fn conversions() {
        // 从标准库Range转换
        assert_eq!(
            FileRange::try_from(1..5).unwrap(),
            FileRange { start: 1, end: 5 }
        );

        // 包含最大值的RangeInclusive
        let max = usize::MAX;
        assert!(FileRange::try_from(0..=max).is_err());

        // 元组转换
        assert_eq!(
            FileRange::try_from((Included(2), Excluded(5))).unwrap(),
            FileRange { start: 2, end: 5 }
        );
    }

    // 错误处理测试
    #[test]
    fn error_handling() {
        // 无效范围
        assert_eq!(
            FileRange::try_from(5..3),
            Err(FileRangeError::InvalidRange {
                start: Included(5),
                end: Excluded(3)
            })
        );

        // 溢出测试
        assert_eq!(
            extract_range_bounds(&(Included(usize::MAX), Excluded(0))),
            Err(FileRangeError::InvalidRange {
                start: Included(usize::MAX),
                end: Excluded(0)
            })
        );

        // 无界测试
        assert_eq!(
            FileRange::try_from((Unbounded, Included(5))),
            Err(FileRangeError::IndexUnbounded)
        );
    }

    // FileMultiRange 操作测试
    #[test]
    fn multirange_operations() {
        // 测试相邻范围合并
        let mut mr = FileMultiRange::new();
        mr.add_checked(1, 3).unwrap();
        mr.add_checked(3, 5).unwrap();
        assert_eq!(mr.inner, smallvec_inline![FileRange { start: 1, end: 5 }]);

        // 测试完全包含范围
        let mut mr = FileMultiRange::new();
        mr.add_checked(1, 10).unwrap();
        mr.add_checked(3, 5).unwrap();
        assert_eq!(mr.inner, smallvec_inline![FileRange { start: 1, end: 10 }]);

        // 测试多范围交集
        let mr1 = FileMultiRange::try_from([(1, 5), (8, 12)].as_slice()).unwrap();
        let mr2 = FileMultiRange::try_from([(3, 10)].as_slice()).unwrap();
        let res = mr1.intersect(&mr2);
        assert_eq!(
            res.inner,
            smallvec_inline![
                FileRange { start: 3, end: 5 },
                FileRange { start: 8, end: 10 }
            ]
        );
    }

    // 复杂减法测试
    #[test]
    fn complex_subtraction() {
        let base = FileMultiRange::try_from([(0, 100)].as_slice()).unwrap();
        let holes = FileMultiRange::try_from([(10, 20), (30, 40), (50, 60)].as_slice()).unwrap();
        let result = base.subtract(&holes);

        assert_eq!(
            result.inner,
            smallvec_inline![
                FileRange { start: 0, end: 10 },
                FileRange { start: 20, end: 30 },
                FileRange { start: 40, end: 50 },
                FileRange {
                    start: 60,
                    end: 100
                }
            ]
        );
    }

    // 空集合测试
    #[test]
    fn empty_operations() {
        let empty = FileMultiRange::new();
        let non_empty = FileMultiRange::try_from([(1, 5)].as_slice()).unwrap();

        // 空集合交集
        assert!(empty.intersect(&non_empty).is_empty());

        // 空集合减法
        assert_eq!(non_empty.subtract(&empty).inner, non_empty.inner);
    }

    // 排序测试
    #[test]
    fn ordering() {
        let r1 = FileRange { start: 1, end: 3 };
        let r2 = FileRange { start: 2, end: 4 };
        let r3 = FileRange { start: 5, end: 7 };

        let mut ranges = vec![r3, r1, r2];
        ranges.sort();

        assert_eq!(
            ranges,
            vec![
                FileRange { start: 1, end: 3 },
                FileRange { start: 2, end: 4 },
                FileRange { start: 5, end: 7 }
            ]
        );
    }

    // 溢出场景测试
    #[test]
    fn overflow_cases() {
        // 结束边界溢出
        assert_eq!(
            extract_range_bounds(&(Included(5), Included(usize::MAX))),
            Err(FileRangeError::IndexOverflow)
        );

        // 开始边界溢出
        assert_eq!(
            extract_range_bounds(&(Excluded(usize::MAX), Excluded(0))),
            Err(FileRangeError::IndexOverflow)
        );
    }

    // 完全覆盖测试
    #[test]
    fn full_coverage() {
        let base = FileMultiRange::try_from([(0, 100)].as_slice()).unwrap();
        let cover = FileMultiRange::try_from([(0, 100)].as_slice()).unwrap();
        assert!(base.subtract(&cover).is_empty());
    }

    // 稀疏范围测试
    #[test]
    fn sparse_ranges() {
        let mut mr = FileMultiRange::new();
        for i in (0..100).step_by(2) {
            mr.add_checked(i, i + 1).unwrap();
        }
        assert_eq!(mr.len(), 50);
        assert_eq!(mr.total_len(), 50);
    }
}
