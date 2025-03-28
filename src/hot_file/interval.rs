use smallvec::SmallVec;
use std::ops::{Bound, Range, RangeBounds, RangeInclusive};
use thiserror::Error;

#[macro_export]
macro_rules! interval {
    ($range:expr) => {{ $crate::hot_file::Interval::try_from($crate::hot_file::RangeBoundsWrapper($range)) }};
    (start = $start:expr, end = $end:expr) => {{ $crate::hot_file::Interval::try_from($crate::hot_file::RangeBoundsWrapper($start..$end)) }};
    (.. $end:expr) => {{ $crate::hot_file::IntervalInclusive::try_from($crate::RangeBoundsWrapper(..$end)) }};
}

#[derive(Debug)]
pub struct RangeBoundsWrapper<T>(pub T);

#[derive(Debug, PartialEq, Clone, Hash, Copy)]
/// 左闭右开
pub struct Interval {
    pub start: usize,
    pub end: usize,
}

impl Interval {
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
            (true, false) => Interval::try_new(intersection.end, self.end),
            (false, true) => Interval::try_new(self.start, intersection.start),
            _ => None,
        }
    }

    #[inline]
    pub fn contains(&self, other: &Self) -> bool {
        self.start <= other.start && self.end >= other.end
    }

    pub fn get<T>(self, slice: &[T]) -> Option<&[T]> {
        let range: Range<usize> = self.into();
        slice.get(range)
    }

    pub fn get_mut<T>(self, slice: &mut [T]) -> Option<&mut [T]> {
        let range: Range<usize> = self.into();
        slice.get_mut(range)
    }
}

impl Eq for Interval {}

impl From<Interval> for MultiInterval {
    fn from(interval: Interval) -> Self {
        use smallvec::smallvec;
        MultiInterval {
            intervals: smallvec![interval],
        }
    }
}
impl From<Interval> for Range<usize> {
    fn from(interval: Interval) -> Self {
        interval.start..interval.end
    }
}

impl From<Interval> for RangeInclusive<usize> {
    fn from(interval: Interval) -> Self {
        interval.start..=interval.end - 1
    }
}

impl RangeBounds<usize> for Interval {
    fn start_bound(&self) -> Bound<&usize> {
        Bound::Included(&self.start)
    }

    fn end_bound(&self) -> Bound<&usize> {
        Bound::Excluded(&self.end)
    }
}

impl PartialOrd for Interval {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        self.start.partial_cmp(&other.start)
    }
}
impl Ord for Interval {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.start.cmp(&other.start)
    }
}

#[derive(Debug, Error, PartialEq)]
pub enum IntervalError {
    #[error("Invalid range: {start:?} - {end:?}")]
    InvalidRange {
        start: Bound<usize>,
        end: Bound<usize>,
    },
    #[error("Index overflow")]
    IndexOverflow,
}

impl<T: RangeBounds<usize>> TryFrom<RangeBoundsWrapper<T>> for Interval {
    type Error = IntervalError;

    fn try_from(range: RangeBoundsWrapper<T>) -> Result<Self, Self::Error> {
        let range = range.0;
        let start = match range.start_bound() {
            Bound::Included(&s) => Some(s),
            Bound::Excluded(&s) => Some(s.checked_add(1).ok_or(IntervalError::IndexOverflow)?),
            Bound::Unbounded => None,
        };
        let end = match range.end_bound() {
            Bound::Included(&e) => Some(e.checked_add(1).ok_or(IntervalError::IndexOverflow)?),
            Bound::Excluded(&e) => Some(e),
            Bound::Unbounded => None,
        };
        if let (Some(start), Some(end)) = (start, end)
            && start < end
        {
            Ok(Self { start, end })
        } else {
            Err(IntervalError::InvalidRange {
                start: range.start_bound().cloned(),
                end: range.end_bound().cloned(),
            })
        }
    }
}

pub type IntervalsStackAllocatedPefered = SmallVec<[Interval; 8]>;

#[derive(Debug, Clone)]
pub struct MultiInterval {
    pub intervals: IntervalsStackAllocatedPefered,
}

impl MultiInterval {
    pub fn new(rngs: &[impl RangeBounds<usize> + Clone]) -> Self {
        let intervals: IntervalsStackAllocatedPefered = rngs
            .into_iter()
            .map(|rng| {
                Interval::try_from(RangeBoundsWrapper(rng.clone())).expect("Invalid range bounds")
            })
            .collect();
        let mut mask = Self { intervals };
        mask.merge();
        mask
    }

    pub fn add(&mut self, rng: impl RangeBounds<usize>) -> Result<(), IntervalError> {
        let interval = interval!(rng)?;
        self.intervals.push(interval);
        self.merge();
        Ok(())
    }

    #[inline]
    pub fn merge(&mut self) {
        self.intervals
            .sort_unstable_by_key(|interval| interval.start);
        let mut merged: IntervalsStackAllocatedPefered = SmallVec::new();
        for interval in std::mem::take(&mut self.intervals) {
            if let Some(last) = merged.last_mut() {
                if interval.start <= last.end {
                    last.end = last.end.max(interval.end);
                } else {
                    merged.push(interval);
                }
            } else {
                merged.push(interval);
            }
        }
        self.intervals = merged;
    }

    pub fn intersect(&self, other: &Self) -> Self {
        let mut res = IntervalsStackAllocatedPefered::new();
        let (mut i, mut j) = (0, 0);

        while i != self.intervals.len() && j != other.intervals.len() {
            let a = &self.intervals[i];
            let b = &other.intervals[j];
            if let Some(intersection) = a.intersect(b) {
                res.push(intersection);
            }
            if a.end <= b.end {
                i += 1;
            } else {
                j += 1;
            }
        }
        Self { intervals: res }
    }

    pub fn union(&self, other: &Self) -> Self {
        let mut merged = self.intervals.clone();
        merged.extend(other.intervals.iter().cloned());
        let mut res = Self { intervals: merged };
        res.merge();
        res
    }

    pub fn subtract(&self, other: &Self) -> Self {
        let mut current_intervals = self.intervals.clone();
        for sub in &other.intervals {
            let mut next_intervals = IntervalsStackAllocatedPefered::new();
            for current in current_intervals {
                let mut temp = current;
                let left_end = std::cmp::min(sub.start, temp.end);
                if let Some(left) = Interval::try_new(temp.start, left_end) {
                    if left.start < left.end {
                        next_intervals.push(left);
                        temp.start = left_end;
                    }
                }
                let right_start = std::cmp::max(sub.end, temp.start);
                if let Some(right) = Interval::try_new(right_start, temp.end) {
                    if right.start < right.end {
                        next_intervals.push(right);
                        temp.end = right_start;
                    }
                }
                if temp.start < temp.end {
                    if let Some(remaining) = temp.subtract(sub) {
                        next_intervals.push(remaining);
                    }
                }
            }
            current_intervals = next_intervals;
        }

        let mut rst = Self {
            intervals: current_intervals,
        };
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
    fn valid_interval_conversion() {
        let interval = interval!(1..=5).unwrap();
        assert_eq!(interval, Interval::try_new(1, 6).unwrap());

        let interval = interval!(1..5).unwrap();
        assert_eq!(interval, Interval::try_new(1, 5).unwrap());

        let result = interval!(..5);
        assert_eq!(
            result,
            Err(IntervalError::InvalidRange {
                start: Bound::Unbounded,
                end: Bound::Excluded(5)
            })
        );
    }

    #[test]
    fn start_moreover_end() {
        let result = interval!(5..1);
        assert_eq!(
            result,
            Err(IntervalError::InvalidRange {
                start: Bound::Included(5),
                end: Bound::Excluded(1)
            })
        );

        let result = interval!(2..2);
        assert_eq!(
            result,
            Err(IntervalError::InvalidRange {
                start: Bound::Included(2),
                end: Bound::Excluded(2)
            })
        );
    }

    #[test]
    fn mask_merging() {
        let mask = MultiInterval::new(smallvec_inline![1..3, 4..6, 7..9].as_slice());
        assert_eq!(
            mask.intervals,
            smallvec_inline![
                Interval::try_new(1, 3).unwrap(),
                Interval::try_new(4, 6).unwrap(),
                Interval::try_new(7, 9).unwrap()
            ]
        );

        let mask = MultiInterval::new(smallvec_inline![1..5, 3..7, 6..9].as_slice());
        assert_eq!(
            mask.intervals,
            smallvec_inline![Interval::try_new(1, 9).unwrap()]
        );

        let mask = MultiInterval::new(smallvec_inline![1..10, 2..5, 3..6].as_slice());
        assert_eq!(
            mask.intervals,
            smallvec_inline![Interval::try_new(1, 10).unwrap()]
        );

        let mask = MultiInterval::new(smallvec_inline![1..5, 5..8].as_slice());
        assert_eq!(
            mask.intervals,
            smallvec_inline![Interval::try_new(1, 8).unwrap()]
        );

        let mask = MultiInterval::new(smallvec_inline![5..8, 1..3, 2..6, 10..12].as_slice());
        assert_eq!(
            mask.intervals,
            smallvec_inline![
                Interval::try_new(1, 8).unwrap(),
                Interval::try_new(10, 12).unwrap()
            ]
        );
    }

    #[test]
    #[should_panic(expected = "Invalid range bounds")]
    fn invalid_range() {
        let _ = MultiInterval::new(smallvec_inline![100..=50].as_slice());
    }

    #[test]
    fn inclusive_range_merging() {
        let interval = interval!(5..=5).unwrap();
        assert_eq!(interval, Interval::try_new(5, 6).unwrap());

        let mask = MultiInterval::new(smallvec_inline![1..=5, 5..=8].as_slice());
        assert_eq!(
            mask.intervals,
            smallvec_inline![Interval::try_new(1, 9).unwrap()]
        );

        let mask = MultiInterval::new(smallvec_inline![1..=5, 7..=9].as_slice());
        assert_eq!(
            mask.intervals,
            smallvec_inline![
                Interval::try_new(1, 6).unwrap(),
                Interval::try_new(7, 10).unwrap()
            ]
        );

        let mask = MultiInterval::new(smallvec_inline![1..=5, 5..=8, 8..=10].as_slice());
        assert_eq!(
            mask.intervals,
            smallvec_inline![Interval::try_new(1, 11).unwrap()]
        );
    }

    #[test]
    #[should_panic]
    fn edge_cases() {
        let mask = MultiInterval::new(smallvec_inline![usize::MAX - 1..=usize::MAX].as_slice());
        assert_eq!(
            mask.intervals,
            smallvec_inline![Interval::try_new(usize::MAX - 1, usize::MAX).unwrap()]
        );
    }

    #[test]
    fn macro_gen() {
        assert_eq!(interval!(1..5).unwrap(), Interval { start: 1, end: 5 });

        assert_eq!(interval!(1..=5).unwrap(), Interval { start: 1, end: 6 });

        assert_eq!(
            interval!(start = 3, end = 7).unwrap(),
            Interval { start: 3, end: 7 }
        );

        let result = interval!(..5);
        assert!(matches!(
            result,
            Err(IntervalError::InvalidRange { start: _, end: _ })
        ));

        let result = interval!(5..1);
        assert!(matches!(
            result,
            Err(IntervalError::InvalidRange { start: _, end: _ })
        ));
    }

    #[test]
    fn macro_integration() {
        let rg1: RangeInclusive<usize> = interval!(1..5).unwrap().into();
        let rg2: RangeInclusive<usize> = interval!(start = 5, end = 8).unwrap().into();

        let mask = MultiInterval::new(smallvec_inline![rg1, rg2].as_slice());

        assert_eq!(
            mask.intervals,
            smallvec_inline![Interval { start: 1, end: 8 }]
        );
    }

    #[test]
    fn push_range() {
        let mut mask = MultiInterval::new(smallvec_inline![3..5, 1..2].as_slice()); // 乱序输入
        assert_eq!(
            mask.intervals,
            smallvec_inline![
                Interval::try_new(1, 2).unwrap(),
                Interval::try_new(3, 5).unwrap()
            ]
        );

        mask.add(2..4).unwrap();
        assert_eq!(
            mask.intervals,
            smallvec_inline![Interval::try_new(1, 5).unwrap()]
        );

        mask.add(6..8).unwrap();
        assert_eq!(
            mask.intervals,
            smallvec_inline![
                Interval::try_new(1, 5).unwrap(),
                Interval::try_new(6, 8).unwrap()
            ]
        );
    }

    #[test]
    fn intersection() {
        let interval1 = Interval::try_new(1, 5).unwrap();
        let interval2 = Interval::try_new(3, 7).unwrap();
        let intersection = interval1.intersect(&interval2);
        assert_eq!(intersection, Some(Interval::try_new(3, 5).unwrap()));

        let interval3 = Interval::try_new(6, 10).unwrap();
        let intersection = interval1.intersect(&interval3);
        assert_eq!(intersection, None);

        let interval4 = Interval::try_new(2, 4).unwrap();
        let interval5 = Interval::try_new(4, 6).unwrap();
        let intersection = interval4.intersect(&interval5);
        assert_eq!(intersection, None);
    }

    #[test]
    fn union() {
        let interval1 = Interval::try_new(1, 5).unwrap();
        let interval2 = Interval::try_new(3, 7).unwrap();
        let union = interval1.union(&interval2);
        assert_eq!(union, Some(Interval::try_new(1, 7).unwrap()));

        let interval3 = Interval::try_new(6, 10).unwrap();
        let union = interval1.union(&interval3);
        assert_eq!(union, Some(Interval::try_new(1, 10).unwrap()));

        let interval4 = Interval::try_new(2, 4).unwrap();
        let interval5 = Interval::try_new(4, 6).unwrap();
        let union = interval4.union(&interval5);
        assert_eq!(union, Some(Interval::try_new(2, 6).unwrap()));
    }

    #[test]
    fn contains() {
        let interval1 = Interval::try_new(1, 5).unwrap();
        let interval2 = Interval::try_new(2, 4).unwrap();
        assert!(interval1.contains(&interval2));

        let interval3 = Interval::try_new(0, 6).unwrap();
        assert!(!interval1.contains(&interval3));

        let interval4 = Interval::try_new(1, 5).unwrap();
        assert!(interval1.contains(&interval4));
    }

    #[test]
    fn subtract() {
        // 右端差集
        let a = Interval::try_new(1, 6).unwrap();
        let b = Interval::try_new(4, 8).unwrap();
        assert_eq!(a.subtract(&b), Some(Interval::try_new(1, 4).unwrap()));
        assert_eq!(b.subtract(&a), Some(Interval::try_new(6, 8).unwrap()));

        // 左端差集
        let c = Interval::try_new(5, 9).unwrap();
        let d = Interval::try_new(3, 7).unwrap();
        assert_eq!(c.subtract(&d), Some(Interval::try_new(7, 9).unwrap()));
        assert_eq!(d.subtract(&c), Some(Interval::try_new(3, 5).unwrap()));

        // 中间交集
        let e = Interval::try_new(2, 8).unwrap();
        let f = Interval::try_new(4, 6).unwrap();
        assert_eq!(e.subtract(&f), None);

        // 完全覆盖
        let g = Interval::try_new(3, 5).unwrap();
        let h = Interval::try_new(2, 6).unwrap();
        assert_eq!(g.subtract(&h), None);
    }

    #[test]
    fn multi_set_operations() {
        // 测试交集
        let a = MultiInterval::new(&[1..5, 8..12]);
        let b = MultiInterval::new(&[3..10, 15..20]);
        let intersection = a.intersect(&b);
        assert_eq!(
            intersection.intervals,
            smallvec_inline![
                Interval::try_new(3, 5).unwrap(),
                Interval::try_new(8, 10).unwrap()
            ]
        );

        // 测试并集
        let union = a.union(&b);
        assert_eq!(
            union.intervals,
            smallvec_inline![
                Interval::try_new(1, 12).unwrap(),
                Interval::try_new(15, 20).unwrap()
            ]
        );

        // 测试差集（A - B）
        let subtraction = a.subtract(&b);
        assert_eq!(
            subtraction.intervals,
            smallvec_inline![
                Interval::try_new(1, 3).unwrap(),
                Interval::try_new(10, 12).unwrap()
            ]
        );

        // 复杂差集测试
        let complex_a = MultiInterval::new(&[0..20]);
        let complex_b = MultiInterval::new(&[2..5, 8..12, 15..18]);
        let diff = complex_a.subtract(&complex_b);
        assert_eq!(
            diff.intervals,
            smallvec_inline![
                Interval::try_new(0, 2).unwrap(),
                Interval::try_new(5, 8).unwrap(),
                Interval::try_new(12, 15).unwrap(),
                Interval::try_new(18, 20).unwrap()
            ]
        );

        // 完全包含测试
        let full_a = MultiInterval::new(&[5..15]);
        let full_b = MultiInterval::new(&[8..12]);
        assert_eq!(
            full_a.subtract(&full_b).intervals,
            smallvec_inline![
                Interval::try_new(5, 8).unwrap(),
                Interval::try_new(12, 15).unwrap()
            ]
        );
    }
}
