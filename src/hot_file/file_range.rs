use smallvec::SmallVec;
use std::ops::{Bound, Range, RangeBounds, RangeInclusive};
use thiserror::Error;

#[macro_export]
macro_rules! rangify {
    ($range:expr) => {{ $crate::hot_file::FileRange::try_from($crate::hot_file::RangeBoundsWrapper($range)) }};
    (start = $start:expr, end = $end:expr) => {{ $crate::hot_file::FileRange::try_from($crate::hot_file::RangeBoundsWrapper($start..$end)) }};
    (.. $end:expr) => {{ $crate::hot_file::rgnInclusive::try_from($crate::RangeBoundsWrapper(..$end)) }};
}

#[derive(Debug)]
pub struct RangeBoundsWrapper<T>(pub T);

#[derive(Debug, PartialEq, Clone, Hash, Copy)]
/// 左闭右开
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
        if self.start <= other.end || self.end >= other.start {
            Self::try_new(self.start.min(other.start), self.end.max(other.end))
        } else {
            None
        }
    }

    #[inline]
    pub fn subtract(&self, other: &Self) -> Option<Self> {
        let intersection = self.intersect(other)?;
        if intersection == *self {
            return None;
        }
        let left_subtract = intersection.start == self.start && intersection.end < self.end;
        let right_subtract = intersection.end == self.end && intersection.start > self.start;
        match (left_subtract, right_subtract) {
            (true, false) => FileRange::try_new(intersection.end, self.end),
            (false, true) => FileRange::try_new(self.start, intersection.start),
            _ => None,
        }
    }

    #[inline]
    pub fn contains(&self, other: &Self) -> bool {
        self.start <= other.start && self.end >= other.end
    }

    // todo 废弃
    pub fn get<T>(self, slice: &[T]) -> Option<&[T]> {
        let range: Range<usize> = self.into();
        slice.get(range)
    }

    //todo 废弃
    pub fn get_mut<T>(self, slice: &mut [T]) -> Option<&mut [T]> {
        let range: Range<usize> = self.into();
        slice.get_mut(range)
    }
}

impl Eq for FileRange {}

impl From<FileRange> for FileMultiRange {
    fn from(rgn: FileRange) -> Self {
        use smallvec::smallvec;
        FileMultiRange {
            inner: smallvec![rgn],
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

impl RangeBounds<usize> for FileRange {
    fn start_bound(&self) -> Bound<&usize> {
        Bound::Included(&self.start)
    }

    fn end_bound(&self) -> Bound<&usize> {
        Bound::Excluded(&self.end)
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

#[derive(Debug, Error, PartialEq)]
pub enum FileRangeError {
    #[error("Invalid range: {start:?} - {end:?}")]
    InvalidRange {
        start: Bound<usize>,
        end: Bound<usize>,
    },
    #[error("Index overflow")]
    IndexOverflow,
}

impl<T: RangeBounds<usize>> TryFrom<RangeBoundsWrapper<T>> for FileRange {
    type Error = FileRangeError;

    fn try_from(range: RangeBoundsWrapper<T>) -> Result<Self, Self::Error> {
        let range = range.0;
        let start = match range.start_bound() {
            Bound::Included(&s) => Some(s),
            Bound::Excluded(&s) => Some(s.checked_add(1).ok_or(FileRangeError::IndexOverflow)?),
            Bound::Unbounded => None,
        };
        let end = match range.end_bound() {
            Bound::Included(&e) => Some(e.checked_add(1).ok_or(FileRangeError::IndexOverflow)?),
            Bound::Excluded(&e) => Some(e),
            Bound::Unbounded => None,
        };
        if let (Some(start), Some(end)) = (start, end)
            && start < end
        {
            Ok(Self { start, end })
        } else {
            Err(FileRangeError::InvalidRange {
                start: range.start_bound().cloned(),
                end: range.end_bound().cloned(),
            })
        }
    }
}

pub type StackAllocatedPefered = SmallVec<[FileRange; 16]>;

#[derive(Debug, Clone)]
pub struct FileMultiRange {
    pub inner: StackAllocatedPefered,
}

impl FileMultiRange {
    pub fn new(rngs: &[impl RangeBounds<usize> + Clone]) -> Self {
        let rgns: StackAllocatedPefered = rngs
            .into_iter()
            .map(|rng| {
                FileRange::try_from(RangeBoundsWrapper(rng.clone())).expect("Invalid range bounds")
            })
            .collect();
        let mut mask = Self { inner: rgns };
        mask.merge();
        mask
    }

    pub fn add(&mut self, rng: impl RangeBounds<usize>) -> Result<(), FileRangeError> {
        let rgn = rangify!(rng)?;
        self.inner.push(rgn);
        self.merge();
        Ok(())
    }

    #[inline]
    pub fn merge(&mut self) {
        self.inner.sort_unstable_by_key(|rgn| rgn.start);
        let mut merged: StackAllocatedPefered = SmallVec::new();
        for rgn in std::mem::take(&mut self.inner) {
            if let Some(last) = merged.last_mut() {
                if rgn.start <= last.end {
                    last.end = last.end.max(rgn.end);
                } else {
                    merged.push(rgn);
                }
            } else {
                merged.push(rgn);
            }
        }
        self.inner = merged;
    }

    pub fn intersect(&self, other: &Self) -> Self {
        let mut rgns = StackAllocatedPefered::new();
        let (mut i, mut j) = (0, 0);

        while i != self.inner.len() && j != other.inner.len() {
            let a = &self.inner[i];
            let b = &other.inner[j];
            if let Some(intersection) = a.intersect(b) {
                rgns.push(intersection);
            }
            if a.end <= b.end {
                i += 1;
            } else {
                j += 1;
            }
        }
        Self { inner: rgns }
    }

    pub fn union(&self, other: &Self) -> Self {
        let mut merged = self.inner.clone();
        merged.extend(other.inner.iter().cloned());
        let mut res = Self { inner: merged };
        res.merge();
        res
    }

    pub fn subtract(&self, other: &Self) -> Self {
        let mut cur_rgns = self.inner.clone();
        for sub in &other.inner {
            let mut next_rgns = StackAllocatedPefered::new();
            for current in cur_rgns {
                let mut tmp = current;
                let left_end = std::cmp::min(sub.start, tmp.end);
                if let Some(left) = FileRange::try_new(tmp.start, left_end) {
                    if left.start < left.end {
                        next_rgns.push(left);
                        tmp.start = left_end;
                    }
                }
                let right_start = std::cmp::max(sub.end, tmp.start);
                if let Some(right) = FileRange::try_new(right_start, tmp.end) {
                    if right.start < right.end {
                        next_rgns.push(right);
                        tmp.end = right_start;
                    }
                }
                if tmp.start < tmp.end {
                    if let Some(remaining) = tmp.subtract(sub) {
                        next_rgns.push(remaining);
                    }
                }
            }
            cur_rgns = next_rgns;
        }
        let mut rst = Self { inner: cur_rgns };
        rst.merge();
        rst
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use smallvec::smallvec_inline;
    use std::ops::Bound;

    #[test]
    fn valid_rgn_conversion() {
        let rgn = rangify!(1..=5).unwrap();
        assert_eq!(rgn, FileRange::try_new(1, 6).unwrap());

        let rgn = rangify!(1..5).unwrap();
        assert_eq!(rgn, FileRange::try_new(1, 5).unwrap());

        let result = rangify!(..5);
        assert_eq!(
            result,
            Err(FileRangeError::InvalidRange {
                start: Bound::Unbounded,
                end: Bound::Excluded(5)
            })
        );
    }

    #[test]
    fn start_moreover_end() {
        let result = rangify!(5..1);
        assert_eq!(
            result,
            Err(FileRangeError::InvalidRange {
                start: Bound::Included(5),
                end: Bound::Excluded(1)
            })
        );

        let result = rangify!(2..2);
        assert_eq!(
            result,
            Err(FileRangeError::InvalidRange {
                start: Bound::Included(2),
                end: Bound::Excluded(2)
            })
        );
    }

    #[test]
    fn mask_merging() {
        let mask = FileMultiRange::new(smallvec_inline![1..3, 4..6, 7..9].as_slice());
        assert_eq!(
            mask.inner,
            smallvec_inline![
                FileRange::try_new(1, 3).unwrap(),
                FileRange::try_new(4, 6).unwrap(),
                FileRange::try_new(7, 9).unwrap()
            ]
        );

        let mask = FileMultiRange::new(smallvec_inline![1..5, 3..7, 6..9].as_slice());
        assert_eq!(
            mask.inner,
            smallvec_inline![FileRange::try_new(1, 9).unwrap()]
        );

        let mask = FileMultiRange::new(smallvec_inline![1..10, 2..5, 3..6].as_slice());
        assert_eq!(
            mask.inner,
            smallvec_inline![FileRange::try_new(1, 10).unwrap()]
        );

        let mask = FileMultiRange::new(smallvec_inline![1..5, 5..8].as_slice());
        assert_eq!(
            mask.inner,
            smallvec_inline![FileRange::try_new(1, 8).unwrap()]
        );

        let mask = FileMultiRange::new(smallvec_inline![5..8, 1..3, 2..6, 10..12].as_slice());
        assert_eq!(
            mask.inner,
            smallvec_inline![
                FileRange::try_new(1, 8).unwrap(),
                FileRange::try_new(10, 12).unwrap()
            ]
        );
    }

    #[test]
    #[should_panic(expected = "Invalid range bounds")]
    fn invalid_range() {
        let _ = FileMultiRange::new(smallvec_inline![100..=50].as_slice());
    }

    #[test]
    fn inclusive_range_merging() {
        let rgn = rangify!(5..=5).unwrap();
        assert_eq!(rgn, FileRange::try_new(5, 6).unwrap());

        let mask = FileMultiRange::new(smallvec_inline![1..=5, 5..=8].as_slice());
        assert_eq!(
            mask.inner,
            smallvec_inline![FileRange::try_new(1, 9).unwrap()]
        );

        let mask = FileMultiRange::new(smallvec_inline![1..=5, 7..=9].as_slice());
        assert_eq!(
            mask.inner,
            smallvec_inline![
                FileRange::try_new(1, 6).unwrap(),
                FileRange::try_new(7, 10).unwrap()
            ]
        );

        let mask = FileMultiRange::new(smallvec_inline![1..=5, 5..=8, 8..=10].as_slice());
        assert_eq!(
            mask.inner,
            smallvec_inline![FileRange::try_new(1, 11).unwrap()]
        );
    }

    #[test]
    #[should_panic]
    fn edge_cases() {
        let mask = FileMultiRange::new(smallvec_inline![usize::MAX - 1..=usize::MAX].as_slice());
        assert_eq!(
            mask.inner,
            smallvec_inline![FileRange::try_new(usize::MAX - 1, usize::MAX).unwrap()]
        );
    }

    #[test]
    fn macro_gen() {
        assert_eq!(rangify!(1..5).unwrap(), FileRange { start: 1, end: 5 });

        assert_eq!(rangify!(1..=5).unwrap(), FileRange { start: 1, end: 6 });

        assert_eq!(
            rangify!(start = 3, end = 7).unwrap(),
            FileRange { start: 3, end: 7 }
        );

        let result = rangify!(..5);
        assert!(matches!(
            result,
            Err(FileRangeError::InvalidRange { start: _, end: _ })
        ));

        let result = rangify!(5..1);
        assert!(matches!(
            result,
            Err(FileRangeError::InvalidRange { start: _, end: _ })
        ));
    }

    #[test]
    fn macro_integration() {
        let rg1: RangeInclusive<usize> = rangify!(1..5).unwrap().into();
        let rg2: RangeInclusive<usize> = rangify!(start = 5, end = 8).unwrap().into();

        let mask = FileMultiRange::new(smallvec_inline![rg1, rg2].as_slice());

        assert_eq!(mask.inner, smallvec_inline![FileRange { start: 1, end: 8 }]);
    }

    #[test]
    fn push_range() {
        let mut mask = FileMultiRange::new(smallvec_inline![3..5, 1..2].as_slice()); // 乱序输入
        assert_eq!(
            mask.inner,
            smallvec_inline![
                FileRange::try_new(1, 2).unwrap(),
                FileRange::try_new(3, 5).unwrap()
            ]
        );

        mask.add(2..4).unwrap();
        assert_eq!(
            mask.inner,
            smallvec_inline![FileRange::try_new(1, 5).unwrap()]
        );

        mask.add(6..8).unwrap();
        assert_eq!(
            mask.inner,
            smallvec_inline![
                FileRange::try_new(1, 5).unwrap(),
                FileRange::try_new(6, 8).unwrap()
            ]
        );
    }

    #[test]
    fn intersection() {
        let rgn1 = FileRange::try_new(1, 5).unwrap();
        let rgn2 = FileRange::try_new(3, 7).unwrap();
        let intersection = rgn1.intersect(&rgn2);
        assert_eq!(intersection, Some(FileRange::try_new(3, 5).unwrap()));

        let rgn3 = FileRange::try_new(6, 10).unwrap();
        let intersection = rgn1.intersect(&rgn3);
        assert_eq!(intersection, None);

        let rgn4 = FileRange::try_new(2, 4).unwrap();
        let rgn5 = FileRange::try_new(4, 6).unwrap();
        let intersection = rgn4.intersect(&rgn5);
        assert_eq!(intersection, None);
    }

    #[test]
    fn union() {
        let rgn1 = FileRange::try_new(1, 5).unwrap();
        let rgn2 = FileRange::try_new(3, 7).unwrap();
        let union = rgn1.union(&rgn2);
        assert_eq!(union, Some(FileRange::try_new(1, 7).unwrap()));

        let rgn3 = FileRange::try_new(6, 10).unwrap();
        let union = rgn1.union(&rgn3);
        assert_eq!(union, Some(FileRange::try_new(1, 10).unwrap()));

        let rgn4 = FileRange::try_new(2, 4).unwrap();
        let rgn5 = FileRange::try_new(4, 6).unwrap();
        let union = rgn4.union(&rgn5);
        assert_eq!(union, Some(FileRange::try_new(2, 6).unwrap()));
    }

    #[test]
    fn contains() {
        let rgn1 = FileRange::try_new(1, 5).unwrap();
        let rgn2 = FileRange::try_new(2, 4).unwrap();
        assert!(rgn1.contains(&rgn2));

        let rgn3 = FileRange::try_new(0, 6).unwrap();
        assert!(!rgn1.contains(&rgn3));

        let rgn4 = FileRange::try_new(1, 5).unwrap();
        assert!(rgn1.contains(&rgn4));
    }

    #[test]
    fn subtract() {
        // 右端差集
        let a = FileRange::try_new(1, 6).unwrap();
        let b = FileRange::try_new(4, 8).unwrap();
        assert_eq!(a.subtract(&b), Some(FileRange::try_new(1, 4).unwrap()));
        assert_eq!(b.subtract(&a), Some(FileRange::try_new(6, 8).unwrap()));

        // 左端差集
        let c = FileRange::try_new(5, 9).unwrap();
        let d = FileRange::try_new(3, 7).unwrap();
        assert_eq!(c.subtract(&d), Some(FileRange::try_new(7, 9).unwrap()));
        assert_eq!(d.subtract(&c), Some(FileRange::try_new(3, 5).unwrap()));

        // 中间交集
        let e = FileRange::try_new(2, 8).unwrap();
        let f = FileRange::try_new(4, 6).unwrap();
        assert_eq!(e.subtract(&f), None);

        // 完全覆盖
        let g = FileRange::try_new(3, 5).unwrap();
        let h = FileRange::try_new(2, 6).unwrap();
        assert_eq!(g.subtract(&h), None);
    }

    #[test]
    fn multi_set_operations() {
        // 测试交集
        let a = FileMultiRange::new(&[1..5, 8..12]);
        let b = FileMultiRange::new(&[3..10, 15..20]);
        let intersection = a.intersect(&b);
        assert_eq!(
            intersection.inner,
            smallvec_inline![
                FileRange::try_new(3, 5).unwrap(),
                FileRange::try_new(8, 10).unwrap()
            ]
        );

        // 测试并集
        let union = a.union(&b);
        assert_eq!(
            union.inner,
            smallvec_inline![
                FileRange::try_new(1, 12).unwrap(),
                FileRange::try_new(15, 20).unwrap()
            ]
        );

        // 测试差集（A - B）
        let subtraction = a.subtract(&b);
        assert_eq!(
            subtraction.inner,
            smallvec_inline![
                FileRange::try_new(1, 3).unwrap(),
                FileRange::try_new(10, 12).unwrap()
            ]
        );

        // 复杂差集测试
        let complex_a = FileMultiRange::new(&[0..20]);
        let complex_b = FileMultiRange::new(&[2..5, 8..12, 15..18]);
        let diff = complex_a.subtract(&complex_b);
        assert_eq!(
            diff.inner,
            smallvec_inline![
                FileRange::try_new(0, 2).unwrap(),
                FileRange::try_new(5, 8).unwrap(),
                FileRange::try_new(12, 15).unwrap(),
                FileRange::try_new(18, 20).unwrap()
            ]
        );

        // 完全包含测试
        let full_a = FileMultiRange::new(&[5..15]);
        let full_b = FileMultiRange::new(&[8..12]);
        assert_eq!(
            full_a.subtract(&full_b).inner,
            smallvec_inline![
                FileRange::try_new(5, 8).unwrap(),
                FileRange::try_new(12, 15).unwrap()
            ]
        );
    }
}
