//! The spatial index is responsible for detecting pairs of possibly
//! intersecting objects for further, more accurate narrow phase inspection.

use super::AABB;
use crate::math as m;

/// A hierarchical grid spatial index.
/// Currently the only type of index implemented.
///
/// This is optimized for fairly small worlds with a low object count (in the thousands at most).
/// If the object count or world size is very large, it will eat up a lot of memory.
#[derive(Clone, Debug)]
pub struct HGrid {
    bounds: AABB,
    // bitsets for grid cells are stored as a contiguous buffer,
    // interpreted in chunks of <mask_size> u64s.
    // this lets us increase the size of a bitset at runtime
    // when number of objects increases past the current limit.
    bitset_size: usize,
    grids: Vec<Grid>,
    // timestamping used to keep track of which colliders were already checked by a query.
    curr_timestamp: u16,
    timestamps: Vec<u16>,
    // cache AABBs that colliders were inserted with
    aabbs: Vec<AABB>,
}

#[derive(Clone, Debug)]
struct Grid {
    spacing: f64,
    has_objects: bool,
    column_count: usize,
    row_count: usize,
    column_bits: Vec<u64>,
    row_bits: Vec<u64>,
}

/// TODO: document all the params, these are not self-explanatory
pub struct HGridParams {
    pub approx_bounds: AABB,
    pub smallest_obj_radius: f64,
    pub largest_obj_radius: f64,
    pub expected_obj_count: usize,
}

impl HGrid {
    /// Create a new HGrid. See [`HGridParams`][self::HGridParams] for explanation.
    pub fn new(params: HGridParams) -> Self {
        let mut spacings = Vec::new();
        let mut spacing = params.smallest_obj_radius;
        while spacing < params.largest_obj_radius {
            spacings.push(spacing);
            spacing *= 2.0;
        }
        // last spacing is the one bigger than everything
        spacings.push(spacing);

        let largest_spacing = spacing;
        let bounds = AABB {
            min: params.approx_bounds.min,
            max: m::Vec2::new(
                (params.approx_bounds.width() / largest_spacing).ceil() * largest_spacing,
                (params.approx_bounds.height() / largest_spacing).ceil() * largest_spacing,
            ),
        };
        let bounds_w = bounds.width();
        let bounds_h = bounds.height();

        let bitset_size = params.expected_obj_count / 64 + 1;

        HGrid {
            bounds,
            bitset_size,
            grids: spacings
                .iter()
                .map(|&spacing| {
                    let column_count = (bounds_w / spacing).round() as usize;
                    let row_count = (bounds_h / spacing).round() as usize;
                    Grid {
                        spacing,
                        has_objects: false,
                        column_count,
                        row_count,
                        column_bits: vec![0; column_count * bitset_size],
                        row_bits: vec![0; row_count * bitset_size],
                    }
                })
                .collect(),
            curr_timestamp: 0,
            timestamps: vec![0; params.expected_obj_count],
            aabbs: vec![AABB::zero(); params.expected_obj_count],
        }
    }

    #[inline]
    pub(crate) fn get_aabb(&self, id: usize) -> AABB {
        self.aabbs[id]
    }

    /// At least for now, recreating the whole grid every frame.
    /// Also allocate more space if we need bigger bitsets and reset timestamps.
    pub(crate) fn prepare(&mut self, collider_count: usize) {
        for grid in &mut self.grids {
            for col in &mut grid.column_bits {
                *col = 0;
            }
            for row in &mut grid.row_bits {
                *row = 0;
            }
            grid.has_objects = false;
        }

        let required_bitset_size = collider_count / 64 + 1;
        if required_bitset_size > self.bitset_size {
            self.bitset_size = required_bitset_size;
            for grid in &mut self.grids {
                grid.column_bits
                    .resize(required_bitset_size * grid.column_count, 0);
                grid.row_bits
                    .resize(required_bitset_size * grid.row_count, 0);
            }
        }

        self.curr_timestamp = 0;
        for ts in &mut self.timestamps {
            *ts = 0;
        }
        self.timestamps.resize(collider_count, 0);
        self.aabbs.resize(collider_count, AABB::zero());
    }

    pub(crate) fn insert(&mut self, aabb: AABB, id: usize) {
        self.aabbs[id] = aabb;

        let aabb_size = aabb.width().max(aabb.height());
        let grid_level = match self.grids.iter_mut().find(|g| g.spacing > aabb_size) {
            Some(first_bigger) => first_bigger,
            None => self.grids.last_mut().unwrap(),
        };
        grid_level.has_objects = true;

        let first_column = (aabb.min.x / grid_level.spacing) as usize;
        let last_column = (aabb.max.x / grid_level.spacing) as usize;
        for col in first_column..=last_column {
            // toroidal wrapping for things outside the grid
            let col = col % grid_level.column_count;
            let m_start = col * self.bitset_size;
            let mut col_bitset =
                BitsetMut(&mut grid_level.column_bits[m_start..(m_start + self.bitset_size)]);
            col_bitset.set(id);
        }

        let first_row = (aabb.min.y / grid_level.spacing) as usize;
        let last_row = (aabb.max.y / grid_level.spacing) as usize;
        for row in first_row..=last_row {
            let row = row % grid_level.row_count;
            let m_start = row * self.bitset_size;
            let mut row_bitset =
                BitsetMut(&mut grid_level.row_bits[m_start..(m_start + self.bitset_size)]);
            row_bitset.set(id);
        }
    }

    pub(crate) fn test_and_insert<'a>(
        &'a mut self,
        aabb: AABB,
        id: usize,
    ) -> impl 'a + Iterator<Item = usize> {
        self.insert(aabb, id);
        self.timestamps[id] = self.curr_timestamp + 1;
        self.test_aabb(aabb)
    }

    pub(crate) fn test_aabb<'a>(&'a mut self, aabb: AABB) -> impl 'a + Iterator<Item = usize> {
        // destructuring to move into closures
        let bitset_size = self.bitset_size;
        let timestamps = &mut self.timestamps;

        self.curr_timestamp += 1;
        let curr_timestamp = self.curr_timestamp;

        self.grids
            .iter()
            .filter(|grid| grid.has_objects)
            .flat_map(move |grid| {
                let col_range =
                    ((aabb.min.x / grid.spacing) as usize)..=((aabb.max.x / grid.spacing) as usize);
                let row_range =
                    ((aabb.min.y / grid.spacing) as usize)..=((aabb.max.y / grid.spacing) as usize);
                col_range.flat_map(move |col| {
                    let col = col % grid.column_count;
                    row_range.clone().flat_map(move |row| {
                        let row = row % grid.row_count;

                        let col_b = col * bitset_size;
                        let row_b = row * bitset_size;
                        BitsetIntersection(
                            &grid.column_bits[col_b..col_b + bitset_size],
                            &grid.row_bits[row_b..row_b + bitset_size],
                        )
                        .iter()
                    })
                })
            })
            .filter_map(move |id| {
                if timestamps[id] != curr_timestamp {
                    timestamps[id] = curr_timestamp;
                    Some(id)
                } else {
                    None
                }
            })
    }
}

//
// bitset ops
//

trait IterableBitset {
    fn len(&self) -> usize;
    fn get_word(&self, idx: usize) -> u64;
}

#[derive(Clone, Copy)]
struct Bitset<'a>(&'a [u64]);

impl<'a> Bitset<'a> {
    pub fn iter(&self) -> BitsetIter<Self> {
        BitsetIter {
            m: *self,
            word_idx: 0,
            seen_bits: 0,
        }
    }

    pub fn intersection(self, other: Self) -> BitsetIntersection<'a> {
        BitsetIntersection(self.0, other.0)
    }
}

impl<'a> IterableBitset for Bitset<'a> {
    fn len(&self) -> usize {
        self.0.len()
    }

    fn get_word(&self, idx: usize) -> u64 {
        self.0[idx]
    }
}

struct BitsetMut<'a>(&'a mut [u64]);

impl<'a> BitsetMut<'a> {
    /// Set the bit at an index.
    ///
    /// # Panics
    /// Panics if the slice given to the bitmask is too small.
    pub fn set(&mut self, idx: usize) {
        let word_idx = idx / 64;
        let bit_idx = idx % 64;
        self.0[word_idx] |= 1_u64 << bit_idx;
    }
}

#[derive(Clone, Copy)]
struct BitsetIntersection<'a>(&'a [u64], &'a [u64]);

impl<'a> BitsetIntersection<'a> {
    pub fn iter(&self) -> BitsetIter<Self> {
        BitsetIter {
            m: *self,
            word_idx: 0,
            seen_bits: 0,
        }
    }
}

impl<'a> IterableBitset for BitsetIntersection<'a> {
    fn len(&self) -> usize {
        self.0.len().min(self.1.len())
    }

    fn get_word(&self, idx: usize) -> u64 {
        self.0[idx] & self.1[idx]
    }
}

#[derive(Clone, Copy)]
struct BitsetIter<Mask: IterableBitset> {
    m: Mask,
    word_idx: usize,
    seen_bits: u64,
}

impl<Mask: IterableBitset> Iterator for BitsetIter<Mask> {
    type Item = usize;

    fn next(&mut self) -> Option<Self::Item> {
        while self.word_idx < self.m.len() {
            let unseen_bits = self.m.get_word(self.word_idx) - self.seen_bits;
            if unseen_bits > 0 {
                let first_bit_idx = unseen_bits.trailing_zeros();
                self.seen_bits |= 1 << first_bit_idx;

                return Some(self.word_idx * 64 + first_bit_idx as usize);
            }

            self.word_idx += 1;
            self.seen_bits = 0;
        }
        None
    }
}

//
// tests
//

#[cfg(test)]
mod tests {
    use super::*;

    fn make_bitset(idxs: &[usize]) -> Vec<u64> {
        let mut ret = Vec::new();
        for idx in idxs {
            let word_idx = idx / 64;
            if word_idx >= ret.len() {
                ret.resize(word_idx + 1, 0);
            }
            BitsetMut(&mut ret).set(*idx);
        }
        ret
    }

    #[test]
    fn set_iter() {
        let m = make_bitset(&[0, 5, 3, 130, 120]);
        itertools::assert_equal(Bitset(&m).iter(), [0, 3, 5, 120, 130].iter().cloned());
    }

    #[test]
    fn set_intersection() {
        let m1 = make_bitset(&[0, 5, 128, 191, 2500]);
        let m2 = make_bitset(&[2, 5, 130, 120, 0, 191, 3000]);
        let isect = Bitset(&m1).intersection(Bitset(&m2));
        itertools::assert_equal(isect.iter(), [0, 5, 191].iter().cloned());
    }
}
