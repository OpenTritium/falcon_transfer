use smallvec::{SmallVec, smallvec};
use std::{
    hint::{likely, unlikely},
    ops::{Bound, Range, RangeInclusive},
};
use thiserror::Error;

const STACK_BUFFERED: usize = 8;

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
    pub const fn len(&self) -> usize {
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
        if likely(self.end >= other.start && other.end >= self.start) {
            Some(Self {
                start: self.start.min(other.start),
                end: self.end.max(other.end),
            })
        } else {
            None
        }
    }

    #[inline]
    pub fn subtract(&self, other: &Self) -> [Option<FileRange>; 2] {
        let intersection = match self.intersect(other) {
            Some(v) => v,
            None => return [Some(*self), None],
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
            ranges: smallvec![rgn],
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
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for FileRange {
    #[inline]
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
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

pub trait ToRangeBoundPair {
    fn to_bound_pair(&self) -> (Bound<usize>, Bound<usize>);
}

impl ToRangeBoundPair for Range<usize> {
    #[inline]
    fn to_bound_pair(&self) -> (Bound<usize>, Bound<usize>) {
        (Bound::Included(self.start), Bound::Excluded(self.end))
    }
}

impl ToRangeBoundPair for RangeInclusive<usize> {
    #[inline]
    fn to_bound_pair(&self) -> (Bound<usize>, Bound<usize>) {
        (Bound::Included(*self.start()), Bound::Included(*self.end()))
    }
}

impl ToRangeBoundPair for (Bound<usize>, Bound<usize>) {
    #[inline]
    fn to_bound_pair(&self) -> (Bound<usize>, Bound<usize>) {
        (self.0, self.1)
    }
}

impl ToRangeBoundPair for FileRange {
    #[inline]
    fn to_bound_pair(&self) -> (Bound<usize>, Bound<usize>) {
        (Bound::Included(self.start), Bound::Excluded(self.end))
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct FileMultiRange {
    ranges: SmallVec<[FileRange; STACK_BUFFERED]>,
}

impl FileMultiRange {
    #[inline]
    pub fn new() -> Self {
        Self {
            ranges: SmallVec::new(),
        }
    }

    fn add_checked(&mut self, start: usize, end: usize) -> Result<(), FileRangeError> {
        let range = FileRange::try_new(start, end).ok_or(FileRangeError::InvalidRange {
            start: Bound::Included(start),
            end: Bound::Excluded(end),
        })?;
        if unlikely(self.ranges.is_empty()) {
            self.ranges.push(range);
            return Ok(());
        }
        let pos = self.ranges.partition_point(|r| r.start <= range.start);
        let mut merge_left = false;
        let mut merge_right = false;
        if pos > 0 && likely(self.ranges[pos - 1].end >= range.start) {
            self.ranges[pos - 1].end = self.ranges[pos - 1].end.max(range.end);
            merge_left = true;
        }
        if pos < self.ranges.len() && self.ranges[pos].start <= range.end {
            self.ranges[pos].start = self.ranges[pos].start.min(range.start);
            self.ranges[pos].end = self.ranges[pos].end.max(range.end);
            merge_right = true;
        }

        match (merge_left, merge_right) {
            // 左右均合并：删除右侧区间，保留左侧合并后的区间
            (true, true) => {
                self.ranges[pos - 1].end = self.ranges[pos].end;
                self.ranges.remove(pos);
            }
            // 仅合并左侧：直接更新区间
            (true, false) => {}
            // 仅合并右侧：直接更新区间
            (false, true) => {}
            // 无合并：插入新区间
            (false, false) => {
                self.ranges.insert(pos, range);
            }
        }

        Ok(())
    }

    #[inline(never)]
    fn merge_around(&mut self, pos: usize) {
        let ranges = self.ranges.as_mut_slice();
        let len = ranges.len();
        if len <= 1 {
            return;
        }

        let mut left = pos;
        let mut right = pos;

        // 向左合并
        while likely(left > 0) {
            let prev = unsafe { ranges.get_unchecked(left - 1) };
            let current = unsafe { ranges.get_unchecked(left) };
            if prev.end >= current.start {
                left -= 1;
            } else {
                break;
            }
        }

        // 向右合并
        while likely(right < len.saturating_sub(1)) {
            let current = unsafe { ranges.get_unchecked(right) };
            let next = unsafe { ranges.get_unchecked(right + 1) };
            if current.end >= next.start {
                right += 1;
            } else {
                break;
            }
        }

        if left < right {
            // 计算合并后的范围
            let merged_start = unsafe { ranges.get_unchecked(left).start };
            let merged_end = unsafe { ranges.get_unchecked(right).end };
            self.ranges.drain(left..=right);
            self.ranges.insert(
                left,
                FileRange {
                    start: merged_start,
                    end: merged_end,
                },
            );
        }
    }

    pub fn intersect(&self, other: &Self) -> Self {
        let mut result = Self::new();
        let (mut a_ptr, mut b_ptr) = (self.ranges.as_ptr(), other.ranges.as_ptr());
        let (a_end, b_end) = (unsafe { a_ptr.add(self.ranges.len()) }, unsafe {
            b_ptr.add(other.ranges.len())
        });

        while a_ptr < a_end && b_ptr < b_end {
            // 直接解引用指针，免去 bound check
            let a = unsafe { &*a_ptr };
            let b = unsafe { &*b_ptr };

            let start = a.start.max(b.start);
            let end = a.end.min(b.end);

            if likely(start < end) {
                result.ranges.push(FileRange { start, end });
            }

            match a.end.cmp(&b.end) {
                std::cmp::Ordering::Less => a_ptr = unsafe { a_ptr.add(1) },
                std::cmp::Ordering::Greater => b_ptr = unsafe { b_ptr.add(1) },
                _ => {
                    a_ptr = unsafe { a_ptr.add(1) };
                    b_ptr = unsafe { b_ptr.add(1) };
                }
            }
        }

        result
    }

    pub fn subtract(&self, other: &Self) -> Self {
        let mut result = Self::new();
        let mut other_idx = 0;
        let other_ranges = other.ranges.as_ptr(); // 改为指针操作
        let other_len = other.ranges.len();

        for &range in &self.ranges {
            let mut current = range;
            while likely(other_idx < other_len) && likely(current.start < current.end) {
                // 两个条件通常都成立
                let sub = unsafe { &*other_ranges.add(other_idx) };
                if unlikely(sub.end <= current.start) {
                    // 当前区间在sub之后是特殊情况
                    other_idx += 1;
                    continue;
                }
                if sub.start >= current.end {
                    break;
                }
                if current.start < sub.start {
                    result.ranges.push(FileRange {
                        start: current.start,
                        end: sub.start,
                    });
                }
                current.start = current.start.max(sub.end);
                if sub.end > current.end {
                    break;
                }
                other_idx += 1;
            }
            if current.start < current.end {
                result.ranges.push(current);
            }
        }

        result
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.ranges.len()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.ranges.is_empty()
    }

    #[inline]
    pub fn total_len(&self) -> usize {
        self.ranges.iter().map(|r| r.len()).sum()
    }
}

impl<T> TryFrom<&[T]> for FileMultiRange
where
    T: ToRangeBoundPair,
{
    type Error = FileRangeError;

    fn try_from(ranges: &[T]) -> Result<Self, Self::Error> {
        let mut rgns = Self::new();
        for range in ranges {
            let (start, end) = extract_range_bounds(range)?;
            rgns.add_checked(start, end)?;
        }
        Ok(rgns)
    }
}

#[inline]
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

    #[test]
    fn filerange_try_new() {
        assert_eq!(
            FileRange::try_new(1, 3),
            Some(FileRange { start: 1, end: 3 })
        );
        assert_eq!(FileRange::try_new(2, 2), None);
        assert_eq!(FileRange::try_new(3, 1), None);
    }

    #[test]
    fn filerange_intersect() {
        let r1 = FileRange { start: 1, end: 5 };
        let r2 = FileRange { start: 3, end: 7 };
        assert_eq!(r1.intersect(&r2), FileRange::try_new(3, 5));

        let r3 = FileRange { start: 5, end: 10 };
        assert_eq!(r1.intersect(&r3), None);
    }

    #[test]
    fn filerange_union() {
        let r1 = FileRange { start: 1, end: 3 };
        let r2 = FileRange { start: 2, end: 4 };
        assert_eq!(r1.union(&r2), FileRange::try_new(1, 4));

        let r1 = FileRange { start: 1, end: 3 };
        let r2 = FileRange { start: 3, end: 4 };
        assert_eq!(r1.union(&r2), FileRange::try_new(1, 4));

        let r3 = FileRange { start: 5, end: 7 };
        assert_eq!(r1.union(&r3), None);
    }

    #[test]
    fn filerange_subtract() {
        let r1 = FileRange { start: 1, end: 10 };
        let r2 = FileRange { start: 3, end: 7 };
        let res = r1.subtract(&r2);
        assert_eq!(res, [FileRange::try_new(1, 3), FileRange::try_new(7, 10)]);

        let r3 = FileRange { start: 0, end: 5 };
        let res2 = r1.subtract(&r3);
        assert_eq!(res2, [None, FileRange::try_new(5, 10)]);

        let r4 = FileRange { start: 1, end: 10 };
        let res3 = r1.subtract(&r4);
        assert_eq!(res3, [None, None]);
    }

    #[test]
    fn filerange_contains() {
        let r1 = FileRange { start: 2, end: 8 };
        let r2 = FileRange { start: 3, end: 5 };
        assert!(r1.contains(&r2));

        let r3 = FileRange { start: 1, end: 9 };
        assert!(!r1.contains(&r3));
    }

    #[test]
    fn extract_valid_range() {
        assert_eq!(extract_range_bounds(&(1..5)), Ok((1, 5)));
        assert_eq!(extract_range_bounds(&(2..=6)), Ok((2, 7)));
        assert_eq!(
            extract_range_bounds(&(Included(3), Excluded(5))),
            Ok((3, 5))
        );
    }

    #[test]
    fn parse_invalid_range() {
        assert_eq!(
            extract_range_bounds(&(5..3)),
            Err(FileRangeError::InvalidRange {
                start: Included(5),
                end: Excluded(3)
            })
        );
        assert_eq!(
            extract_range_bounds(&(Included(usize::MAX), Excluded(0))),
            Err(FileRangeError::InvalidRange {
                start: Included(usize::MAX),
                end: Excluded(0)
            })
        );
        assert_eq!(
            extract_range_bounds(&(Included(0), Excluded(0))),
            Err(FileRangeError::InvalidRange {
                start: Included(0),
                end: Excluded(0)
            })
        );
    }

    #[test]
    fn multirange_add_and_merge() {
        let mut mr = FileMultiRange::new();
        mr.add_checked(1, 3).unwrap();
        mr.add_checked(2, 5).unwrap();
        assert_eq!(mr.ranges, smallvec_inline![FileRange { start: 1, end: 5 }]);

        mr.add_checked(7, 10).unwrap();
        assert_eq!(
            mr.ranges,
            smallvec_inline![
                FileRange { start: 1, end: 5 },
                FileRange { start: 7, end: 10 }
            ]
        );
    }

    #[test]
    fn multirange_intersect() {
        let mr1 = FileMultiRange::try_from([1..5, 8..12].as_slice()).unwrap();
        let mr2 = FileMultiRange::try_from([3..10].as_slice()).unwrap();
        let res = mr1.intersect(&mr2);
        assert_eq!(
            res.ranges,
            smallvec_inline![
                FileRange { start: 3, end: 5 },
                FileRange { start: 8, end: 10 }
            ]
        );
    }

    #[test]
    fn test_multirange_subtract() {
        let mr1 = FileMultiRange::try_from([1..10].as_slice()).unwrap();
        let mr2 = FileMultiRange::try_from([3..5, 7..9].as_slice()).unwrap();
        let res = mr1.subtract(&mr2);
        assert_eq!(
            res.ranges,
            smallvec_inline![
                FileRange { start: 1, end: 3 },
                FileRange { start: 5, end: 7 },
                FileRange { start: 9, end: 10 }
            ]
        );
    }

    #[test]
    fn bound_checks() {
        assert!(matches!(
            FileRange::try_from((Unbounded, Included(5))),
            Err(FileRangeError::IndexUnbounded)
        ));
        assert!(matches!(
            FileRange::try_from((Included(5), Unbounded)),
            Err(FileRangeError::IndexUnbounded)
        ));
    }
}
