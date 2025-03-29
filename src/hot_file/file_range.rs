use smallvec::{SmallVec, smallvec};
use std::ops::{Bound, Range, RangeInclusive};
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

#[derive(Debug, PartialEq, Clone, Hash, Copy)]
pub struct FileRange {
    pub start: usize,
    pub end: usize,
}

impl FileRange {
    pub fn try_new(start: usize, end: usize) -> Option<Self> {
        if start < end {
            Some(Self { start, end })
        } else {
            None
        }
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.end - self.start
    }

    #[inline]
    pub fn intersect(&self, other: &Self) -> Option<Self> {
        if self.start < other.end && other.start < self.end {
            Self::try_new(self.start.max(other.start), self.end.min(other.end))
        } else {
            None
        }
    }

    #[inline]
    pub fn union(&self, other: &Self) -> Option<Self> {
        if self.end >= other.start && other.end >= self.start {
            Self::try_new(self.start.min(other.start), self.end.max(other.end))
        } else {
            None
        }
    }

    #[inline]
    pub fn subtract(&self, other: &Self) -> [Option<FileRange>; 2] {
        let mut result = [None, None];
        let intersection = match self.intersect(other) {
            Some(v) => v,
            None => {
                result[0] = Some(*self);
                return result;
            }
        };
        if self.start < intersection.start {
            result[0] = FileRange::try_new(self.start, intersection.start)
        }
        if self.end > intersection.end {
            result[1] = FileRange::try_new(intersection.end, self.end)
        }
        result
    }

    #[inline]
    pub fn contains(&self, other: &Self) -> bool {
        self.start <= other.start && self.end >= other.end
    }
}

impl Eq for FileRange {}

impl From<FileRange> for FileMultiRange {
    fn from(rgn: FileRange) -> Self {
        Self {
            ranges: smallvec![rgn],
        }
    }
}

impl From<FileRange> for Range<usize> {
    fn from(rgn: FileRange) -> Self {
        rgn.start..rgn.end
    }
}

impl From<FileRange> for RangeInclusive<usize> {
    fn from(rgn: FileRange) -> Self {
        rgn.start..=rgn.end - 1
    }
}

impl PartialOrd for FileRange {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        self.start.partial_cmp(&other.start)
    }
}

impl Ord for FileRange {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.start.cmp(&other.start)
    }
}

impl TryFrom<Range<usize>> for FileRange {
    type Error = FileRangeError;
    fn try_from(rgn: Range<usize>) -> Result<Self, Self::Error> {
        let (start, end) = extract_range_bounds(&rgn)?;
        Ok(FileRange { start, end })
    }
}

impl TryFrom<RangeInclusive<usize>> for FileRange {
    type Error = FileRangeError;
    fn try_from(rgn: RangeInclusive<usize>) -> Result<Self, Self::Error> {
        let (start, end) = extract_range_bounds(&rgn)?;
        Ok(FileRange { start, end })
    }
}

impl TryFrom<(Bound<usize>, Bound<usize>)> for FileRange {
    type Error = FileRangeError;
    fn try_from(rgn: (Bound<usize>, Bound<usize>)) -> Result<Self, Self::Error> {
        let (start, end) = extract_range_bounds(&rgn)?;
        Ok(FileRange { start, end })
    }
}

pub trait ToRangeBoundPair {
    fn to_bound_pair(&self) -> (Bound<usize>, Bound<usize>);
}

impl ToRangeBoundPair for Range<usize> {
    fn to_bound_pair(&self) -> (Bound<usize>, Bound<usize>) {
        (Bound::Included(self.start), Bound::Excluded(self.end))
    }
}

impl ToRangeBoundPair for RangeInclusive<usize> {
    fn to_bound_pair(&self) -> (Bound<usize>, Bound<usize>) {
        (Bound::Included(*self.start()), Bound::Included(*self.end()))
    }
}

impl ToRangeBoundPair for (Bound<usize>, Bound<usize>) {
    fn to_bound_pair(&self) -> (Bound<usize>, Bound<usize>) {
        (self.0, self.1)
    }
}

impl ToRangeBoundPair for FileRange {
    fn to_bound_pair(&self) -> (Bound<usize>, Bound<usize>) {
        (Bound::Included(self.start), Bound::Excluded(self.end))
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct FileMultiRange {
    ranges: SmallVec<[FileRange; STACK_BUFFERED]>,
}

impl FileMultiRange {
    pub fn new() -> Self {
        Self {
            ranges: smallvec![],
        }
    }

    fn add_checked(&mut self, start: usize, end: usize) -> Result<(), FileRangeError> {
        let range = FileRange::try_new(start, end).ok_or(FileRangeError::InvalidRange {
            start: Bound::Included(start),
            end: Bound::Excluded(end),
        })?;

        let pos = self.ranges.partition_point(|r| r.start <= range.start);
        self.ranges.insert(pos, range);
        self.merge_around(pos);
        Ok(())
    }

    fn merge_around(&mut self, pos: usize) {
        let mut merge_pos = pos;

        // 向前合并
        while merge_pos > 0 && self.ranges[merge_pos - 1].end >= self.ranges[merge_pos].start {
            self.ranges[merge_pos - 1].end = self.ranges[merge_pos - 1]
                .end
                .max(self.ranges[merge_pos].end);
            self.ranges.remove(merge_pos);
            merge_pos -= 1;
        }

        // 向后合并
        while merge_pos < self.ranges.len().saturating_sub(1)
            && self.ranges[merge_pos].end >= self.ranges[merge_pos + 1].start
        {
            self.ranges[merge_pos].end = self.ranges[merge_pos]
                .end
                .max(self.ranges[merge_pos + 1].end);
            self.ranges.remove(merge_pos + 1);
        }
    }

    pub fn merge(&mut self) {
        if self.ranges.is_empty() {
            return;
        }

        let mut merged = smallvec![];
        let mut current = self.ranges[0];

        for range in &self.ranges[1..] {
            if range.start <= current.end {
                current.end = current.end.max(range.end);
            } else {
                merged.push(current);
                current = *range;
            }
        }

        merged.push(current);
        self.ranges = merged;
    }

    pub fn intersect(&self, other: &Self) -> Self {
        let mut result = Self::new();
        let (mut i, mut j) = (0, 0);

        while i < self.len() && j < other.len() {
            let a = self.ranges[i];
            let b = other.ranges[j];

            let start = a.start.max(b.start);
            let end = a.end.min(b.end);

            if start < end {
                result.ranges.push(FileRange { start, end });
            }

            if a.end <= b.end {
                i += 1;
            } else {
                j += 1;
            }
        }

        result
    }

    pub fn subtract(&self, other: &Self) -> Self {
        let mut result = Self::new();
        let mut other_idx = 0;

        for &range in &self.ranges {
            let mut current = range;

            while other_idx < other.len() && current.start < current.end {
                let sub = other.ranges[other_idx];

                if sub.end <= current.start {
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

// 调整泛型实现以提升可读性
impl<T> TryFrom<&[T]> for FileMultiRange
where
    T: ToRangeBoundPair + Clone,
{
    type Error = FileRangeError;

    fn try_from(ranges: &[T]) -> Result<Self, Self::Error> {
        let mut rgns = Self::new();
        for range in ranges {
            let (start, end) = extract_range_bounds(range)?;
            rgns.add_checked(start, end)?;
        }
        rgns.merge();
        Ok(rgns)
    }
}

// 区间解析工具函数
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

    if start < end {
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
