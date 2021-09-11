//! The spatial index is responsible for detecting pairs of possibly
//! intersecting objects for further, more accurate narrow phase inspection.

use super::{MaskMatrix, AABB};
use crate::math as m;

/// A hierarchical grid spatial index.
/// Currently the only type of index implemented.
///
/// This is optimized for fairly small worlds with a low object count (in the thousands at most).
/// If the object count or world size is very large, it will eat up a lot of memory.
#[derive(Clone, Debug)]
pub struct HGrid {
    pub(crate) bounds: AABB,
    // bitsets for grid cells are stored as a contiguous buffer,
    // interpreted in chunks of <mask_size> u64s.
    // this lets us increase the size of a bitset at runtime
    // when number of objects increases past the current limit.
    bitset_size: usize,
    pub(crate) grids: Vec<Grid>,
    // timestamping used to keep track of which colliders were already checked by a query.
    curr_timestamp: u16,
    timestamps: Vec<u16>,
    // cache AABBs that colliders were inserted with, their layers to cull ignored layer pairs
    // quickly, and their generations in the graph to allow safe user-facing queries
    aabbs: Vec<AABB>,
    layers: Vec<u64>,
    generations: Vec<usize>,
}

#[derive(Clone, Debug)]
pub(crate) struct Grid {
    pub(crate) spacing: f64,
    has_objects: bool,
    pub(crate) column_count: usize,
    pub(crate) row_count: usize,
    column_bits: Vec<u64>,
    row_bits: Vec<u64>,
}

/// Parameters for the creation of a hierarchical grid.
pub struct HGridParams {
    /// Approximate bounds of the grid. Approximate because the actual bounds
    /// are rounded to fit a multiple of the largest grid level's spacing.
    /// The bottom left bound is used as-is and the top right is extended.
    ///
    /// Naturally, the larger the bounds, the more memory the grid takes.
    /// The grid doesn't need to cover the whole world because it wraps around
    /// toroidally to cover all of space, however, the larger the grid, the less
    /// far-apart objects will be tested due to said wrapping.
    /// Adjust the bounds to find a good tradeoff between speed and memory.
    pub approx_bounds: AABB,
    /// Spacing of the lowest grid level. Reducing it increases required memory
    /// and reduces unnecessary collision checks.
    ///
    /// A likely good value is a little (10-50%) larger than the smallest objects in your scene.
    pub lowest_spacing: f64,
    /// Number of grid levels. Increasing it increases required memory and
    /// may speed up collision detection if there's high variance in object size.
    ///
    /// NOTE: for now, little optimization involving multiple grid levels has been done.
    /// You're probably best off setting this to 1.
    pub level_count: usize,
    /// The number to multiply spacing by for subsequent grid levels after `lowest_spacing`.
    ///
    /// A good number depends on the distribution of object sizes.  2 is good for most cases.
    pub spacing_ratio: usize,
    /// How many objects to initially allocate space for.
    /// More space will be allocated as needed.
    pub initial_capacity: usize,
}

impl Default for HGridParams {
    fn default() -> Self {
        Self {
            approx_bounds: AABB {
                min: m::Vec2::new(-40.0, -10.0),
                max: m::Vec2::new(40.0, 10.0),
            },
            lowest_spacing: 1.0,
            level_count: 1,
            spacing_ratio: 2,
            initial_capacity: 0,
        }
    }
}

impl HGrid {
    /// Create a new HGrid. See [`HGridParams`][self::HGridParams] for explanation.
    pub fn new(params: HGridParams) -> Self {
        let mut spacings: Vec<f64> = Vec::new();
        let mut spacing = params.lowest_spacing;
        spacings.push(spacing);
        for _i in 1..params.level_count {
            spacing *= params.spacing_ratio as f64;
            spacings.push(spacing);
        }

        let largest_spacing = spacing;
        let bounds = AABB {
            min: params.approx_bounds.min,
            max: params.approx_bounds.min
                + m::Vec2::new(
                    (params.approx_bounds.width() / largest_spacing).ceil() * largest_spacing,
                    (params.approx_bounds.height() / largest_spacing).ceil() * largest_spacing,
                ),
        };
        let bounds_w = bounds.width();
        let bounds_h = bounds.height();

        let bitset_size = params.initial_capacity / 64 + 1;

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
            timestamps: vec![0; params.initial_capacity],
            aabbs: vec![AABB::zero(); params.initial_capacity],
            layers: vec![0; params.initial_capacity],
            generations: vec![0; params.initial_capacity],
        }
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
        self.layers.resize(collider_count, 0);
        self.generations.resize(collider_count, 0);
    }

    pub(crate) fn insert(&mut self, node: StoredNode, aabb: AABB, layer: u64) {
        let id = node.idx;
        self.aabbs[id] = aabb;
        self.layers[id] = layer;
        self.generations[id] = node.gen;

        let aabb = AABB {
            min: aabb.min - self.bounds.min,
            max: aabb.max - self.bounds.min,
        };
        // select grid level based on smaller extent of the aabb
        let aabb_size = aabb.width().min(aabb.height());
        let grid_level = match self.grids.iter_mut().find(|g| g.spacing > aabb_size) {
            Some(first_bigger) => first_bigger,
            None => self.grids.last_mut().unwrap(),
        };
        grid_level.has_objects = true;

        let first_column = (aabb.min.x / grid_level.spacing) as isize;
        let last_column = (aabb.max.x / grid_level.spacing) as isize;
        for col in first_column..=last_column {
            // toroidal wrapping for things outside the grid
            let col = col.rem_euclid(grid_level.column_count as isize) as usize;
            let m_start = col * self.bitset_size;
            let mut col_bitset =
                BitsetMut(&mut grid_level.column_bits[m_start..(m_start + self.bitset_size)]);
            col_bitset.set(id);
        }

        let first_row = (aabb.min.y / grid_level.spacing) as isize;
        let last_row = (aabb.max.y / grid_level.spacing) as isize;
        for row in first_row..=last_row {
            let row = row.rem_euclid(grid_level.row_count as isize) as usize;
            let m_start = row * self.bitset_size;
            let mut row_bitset =
                BitsetMut(&mut grid_level.row_bits[m_start..(m_start + self.bitset_size)]);
            row_bitset.set(id);
        }
    }

    pub(crate) fn test_and_insert<'a>(
        &'a mut self,
        node: StoredNode,
        aabb: AABB,
        layer: u64,
        mask_matrix: &'a MaskMatrix,
    ) -> impl 'a + Iterator<Item = StoredNode> {
        self.insert(node, aabb, layer);
        self.timestamps[node.idx] = self.curr_timestamp + 1;
        self.test_aabb(aabb, layer, mask_matrix)
    }

    pub(crate) fn test_aabb<'a>(
        &'a mut self,
        aabb: AABB,
        layer: u64,
        mask_matrix: &'a MaskMatrix,
    ) -> impl 'a + Iterator<Item = StoredNode> {
        let aabb_worldspace = aabb;
        let aabb = AABB {
            min: aabb.min - self.bounds.min,
            max: aabb.max - self.bounds.min,
        };

        // destructuring to move into closures
        let bitset_size = self.bitset_size;
        let timestamps = &mut self.timestamps;
        let aabbs = &self.aabbs;
        let layers = &self.layers;
        let generations = &self.generations;

        self.curr_timestamp += 1;
        let curr_timestamp = self.curr_timestamp;

        self.grids
            .iter()
            .filter(|grid| grid.has_objects)
            .flat_map(move |grid| {
                let col_range =
                    ((aabb.min.x / grid.spacing) as isize)..=((aabb.max.x / grid.spacing) as isize);
                let row_range =
                    ((aabb.min.y / grid.spacing) as isize)..=((aabb.max.y / grid.spacing) as isize);
                col_range.flat_map(move |col| {
                    let col = col.rem_euclid(grid.column_count as isize) as usize;
                    row_range.clone().flat_map(move |row| {
                        let row = row.rem_euclid(grid.row_count as isize) as usize;

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
                    if mask_matrix.get(layers[id], layer) {
                        // aabb check to quickly cull things that are in the same square because of
                        // wrapping or just far enough apart
                        aabb_worldspace
                            .intersection(&aabbs[id])
                            .map(|_| StoredNode {
                                idx: id,
                                gen: generations[id],
                            })
                    } else {
                        None
                    }
                } else {
                    None
                }
            })
    }

    pub(crate) fn test_point(&self, point: m::Vec2) -> impl '_ + Iterator<Item = StoredNode> {
        let point_worldspace = point;
        let point = point - self.bounds.min;

        let bitset_size = self.bitset_size;
        let aabbs = &self.aabbs;
        let generations = &self.generations;
        self.grids
            .iter()
            .filter(|grid| grid.has_objects)
            .flat_map(move |grid| {
                let col = (point.x / grid.spacing) as isize;
                let col = col.rem_euclid(grid.column_count as isize) as usize;
                let row = (point.y / grid.spacing) as isize;
                let row = row.rem_euclid(grid.row_count as isize) as usize;
                let col_b = col * bitset_size;
                let row_b = row * bitset_size;
                BitsetIntersection(
                    &grid.column_bits[col_b..col_b + bitset_size],
                    &grid.row_bits[row_b..row_b + bitset_size],
                )
                .iter()
            })
            .filter(move |&id| aabbs[id].contains_point(point_worldspace))
            .map(move |id| StoredNode {
                idx: id,
                gen: generations[id],
            })
    }

    pub(crate) fn populated_cells(&self) -> impl '_ + Iterator<Item = GridCell> {
        let bitset_size = self.bitset_size;
        self.grids
            .iter()
            .enumerate()
            .flat_map(move |(grid_idx, grid)| {
                grid.column_bits.chunks(bitset_size).enumerate().flat_map(
                    move |(col_idx, col_bits)| {
                        grid.row_bits.chunks(bitset_size).enumerate().filter_map(
                            move |(row_idx, row_bits)| {
                                if BitsetIntersection(col_bits, row_bits)
                                    .iter()
                                    .next()
                                    .is_none()
                                {
                                    None
                                } else {
                                    Some(GridCell {
                                        grid_idx,
                                        col_idx,
                                        row_idx,
                                    })
                                }
                            },
                        )
                    },
                )
            })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct StoredNode {
    pub idx: usize,
    pub gen: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct GridCell {
    pub grid_idx: usize,
    pub col_idx: usize,
    pub row_idx: usize,
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

// these methods are useful in tests even though they're not used anywhere public atm
#[allow(dead_code)]
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
