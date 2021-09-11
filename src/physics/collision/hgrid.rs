//! The spatial index is responsible for detecting pairs of possibly
//! intersecting objects for further, more accurate narrow phase inspection.

use super::{MaskMatrix, AABB};
use crate::{
    math as m,
    physics::bitmatrix::{BitMatrix, BitMatrixParams},
};

/// A hierarchical grid spatial index.
/// Currently the only type of index implemented.
///
/// This is optimized for fairly small worlds with a low object count (in the thousands at most).
/// If the object count or world size is very large, it will eat up a lot of memory.
#[derive(Clone, Debug)]
pub struct HGrid {
    pub(crate) bounds: AABB,
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
    column_bits: BitMatrix,
    row_bits: BitMatrix,
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

        HGrid {
            bounds,
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
                        column_bits: BitMatrix::new(BitMatrixParams {
                            bits_per_entry: params.initial_capacity,
                            entry_count: column_count,
                        }),
                        row_bits: BitMatrix::new(BitMatrixParams {
                            bits_per_entry: params.initial_capacity,
                            entry_count: row_count,
                        }),
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
            grid.column_bits.clear_and_resize(collider_count);
            grid.row_bits.clear_and_resize(collider_count);
            grid.has_objects = false;
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
            grid_level.column_bits.entry_mut(col).set(id);
        }

        let first_row = (aabb.min.y / grid_level.spacing) as isize;
        let last_row = (aabb.max.y / grid_level.spacing) as isize;
        for row in first_row..=last_row {
            let row = row.rem_euclid(grid_level.row_count as isize) as usize;
            grid_level.row_bits.entry_mut(row).set(id);
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
                        grid.column_bits
                            .entry(col)
                            .intersection(grid.row_bits.entry(row))
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
                grid.column_bits
                    .entry(col)
                    .intersection(grid.row_bits.entry(row))
                    .iter()
            })
            .filter(move |&id| aabbs[id].contains_point(point_worldspace))
            .map(move |id| StoredNode {
                idx: id,
                gen: generations[id],
            })
    }

    pub(crate) fn populated_cells(&self) -> impl '_ + Iterator<Item = GridCell> {
        self.grids
            .iter()
            .enumerate()
            .flat_map(move |(grid_idx, grid)| {
                grid.column_bits
                    .iter()
                    .enumerate()
                    .flat_map(move |(col_idx, col_entry)| {
                        grid.row_bits
                            .iter()
                            .enumerate()
                            .filter_map(move |(row_idx, row_entry)| {
                                col_entry
                                    .intersection(row_entry)
                                    .iter()
                                    .next()
                                    .map(|_| GridCell {
                                        grid_idx,
                                        col_idx,
                                        row_idx,
                                    })
                            })
                    })
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
