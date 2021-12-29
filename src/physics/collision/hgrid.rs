//! The spatial index is responsible for detecting pairs of possibly
//! intersecting objects for further, more accurate narrow phase inspection.

use super::{MaskMatrix, Ray, AABB};
use crate::{
    graph::NodeKey,
    math as m,
    physics::{
        bitmatrix::{BitMatrix, BitMatrixParams, EntryIntersection, EntryIter},
        Collider,
    },
};

/// A hierarchical grid spatial index.
///
/// This is optimized for fairly small worlds with a low object count (in the thousands at most).
/// If the object count or world size is very large, it will eat up a lot of memory.
///
/// No other spatial index algorithms are currently implemented,
/// so if this doesn't work for you you're out of luck for now.
#[derive(Debug)]
pub struct HGrid {
    pub(crate) bounds: AABB,
    pub(crate) grids: Vec<Grid>,
    spacing_ratio: usize,
    // timestamping used to keep track of which colliders were already checked by a query.
    last_timestamp: u16,
    timestamps: Vec<u16>,
    // cache AABBs that colliders were inserted with, their layers to cull ignored layer pairs
    // quickly, and their generations in the graph to allow safe user-facing queries
    aabbs: Vec<AABB>,
    layers: Vec<usize>,
    generations: Vec<usize>,
}

#[derive(Clone, Debug)]
pub(crate) struct Grid {
    pub(crate) spacing: f64,
    pub(crate) column_count: usize,
    pub(crate) row_count: usize,
    column_bits: BitMatrix,
    row_bits: BitMatrix,
    // bitmask with bit per cell of a grid,
    // indicating whether objects exist below that cell on lower grid levels.
    subgrid_mask: Option<BitMatrix>,
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
    /// usually speeds up raycasts (particularly ones over long distances) and point queries.
    /// It may also speed up collision detection if there's high variance in object size.
    ///
    /// Two or three should be sufficient depending on the size distribution of your objects.
    /// Anything more is probably too much.
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
            level_count: 2,
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

        let mut grids: Vec<Grid> = spacings
            .iter()
            .map(|&spacing| {
                let column_count = (bounds_w / spacing).round() as usize;
                let row_count = (bounds_h / spacing).round() as usize;
                Grid {
                    spacing,
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
                    subgrid_mask: None,
                }
            })
            .collect();

        // set subgrid mask for all except smallest grid
        for grid in grids.iter_mut().skip(1) {
            grid.subgrid_mask = Some(BitMatrix::new(BitMatrixParams {
                bits_per_entry: grid.row_count,
                entry_count: grid.column_count,
            }))
        }

        HGrid {
            bounds,
            grids,
            spacing_ratio: params.spacing_ratio,
            last_timestamp: 0,
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
            if let Some(mask) = &mut grid.subgrid_mask {
                mask.clear();
            }
        }

        self.last_timestamp = 0;
        for ts in &mut self.timestamps {
            *ts = 0;
        }
        self.timestamps.resize(collider_count, 0);
        self.aabbs.resize(collider_count, AABB::zero());
        self.layers.resize(collider_count, 0);
        self.generations.resize(collider_count, 0);
    }

    pub(crate) fn insert(&mut self, node: NodeKey<Collider>, aabb: AABB, layer: usize) {
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
        let last_grid_idx = self.grids.len() - 1;
        // iterator that starts at the first grid level larger than the object
        let mut grids = self
            .grids
            .iter_mut()
            .enumerate()
            .skip_while(|(i, g)| g.spacing < aabb_size && *i < last_grid_idx)
            .map(|(_, g)| g);

        let placement_grid = grids.next().unwrap();

        let first_column = (aabb.min.x / placement_grid.spacing) as isize;
        let last_column = (aabb.max.x / placement_grid.spacing) as isize;
        for col in first_column..=last_column {
            // toroidal wrapping for things outside the grid
            let col = col.rem_euclid(placement_grid.column_count as isize) as usize;
            placement_grid.column_bits.entry_mut(col).set(id);
        }

        let first_row = (aabb.min.y / placement_grid.spacing) as isize;
        let last_row = (aabb.max.y / placement_grid.spacing) as isize;
        for row in first_row..=last_row {
            let row = row.rem_euclid(placement_grid.row_count as isize) as usize;
            placement_grid.row_bits.entry_mut(row).set(id);
        }

        // mark above grids as having something below them.
        // we can get cells on subsequent grids by dividing by spacing ratio
        let mut spacing = 1;
        for grid in grids {
            spacing *= self.spacing_ratio;
            // masks are guaranteed to exist because this can't be the first grid
            let mask = grid.subgrid_mask.as_mut().unwrap();
            let fst_c = first_column / spacing as isize;
            let last_c = last_column / spacing as isize;
            let fst_r = first_row / spacing as isize;
            let last_r = last_row / spacing as isize;
            for col in fst_c..=last_c {
                let col = col.rem_euclid(grid.column_count as isize) as usize;
                for row in fst_r..=last_r {
                    let row = row.rem_euclid(grid.row_count as isize) as usize;
                    mask.entry_mut(col).set(row);
                }
            }
        }
    }

    pub(crate) fn test_and_insert<'a>(
        &'a mut self,
        node: NodeKey<Collider>,
        aabb: AABB,
        layer: usize,
        mask_matrix: &'a MaskMatrix,
    ) -> impl 'a + Iterator<Item = NodeKey<Collider>> {
        self.insert(node, aabb, layer);
        // pre-increment timestamp so it's ignored by the check
        self.timestamps[node.idx] = self.last_timestamp + 1;
        self.test_aabb(aabb, mask_matrix.get_mask(layer))
    }

    pub(crate) fn test_aabb(
        &mut self,
        aabb: AABB,
        layer_mask: super::LayerMask,
    ) -> impl '_ + Iterator<Item = NodeKey<Collider>> {
        let aabb_worldspace = aabb;
        let aabb = AABB {
            min: aabb.min - self.bounds.min,
            max: aabb.max - self.bounds.min,
        };

        // destructuring still needed here (in 2021 edition)
        // to make lifetimes work by moving the right things
        let timestamps = &mut self.timestamps;
        let aabbs = &self.aabbs;
        let layers = &self.layers;
        let generations = &self.generations;

        self.last_timestamp += 1;
        let curr_timestamp = self.last_timestamp;

        self.grids
            .iter()
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
                if timestamps[id] == curr_timestamp {
                    return None;
                }
                timestamps[id] = curr_timestamp;
                if !layer_mask.get(layers[id]) {
                    return None;
                }
                // aabb check to quickly cull things that are in the same square because of
                // wrapping or just far enough apart
                aabb_worldspace.intersection(&aabbs[id]).map(|_| NodeKey {
                    idx: id,
                    gen: generations[id],
                    _marker: std::marker::PhantomData,
                })
            })
    }

    pub(crate) fn test_point(
        &self,
        point: m::Vec2,
    ) -> impl '_ + Iterator<Item = NodeKey<Collider>> {
        let point_worldspace = point;
        let point = point - self.bounds.min;

        // a bit of a monster iterator but I really didn't feel like
        // making a new type and impling Iterator by hand :^)
        self.grids
            .iter()
            .rev()
            .scan(false, move |empty_below, grid| {
                if *empty_below {
                    return None;
                }
                let col = (point.x / grid.spacing) as isize;
                let col = col.rem_euclid(grid.column_count as isize) as usize;
                let row = (point.y / grid.spacing) as isize;
                let row = row.rem_euclid(grid.row_count as isize) as usize;
                if let Some(mask) = &grid.subgrid_mask {
                    if !mask.entry(col).has(row) {
                        *empty_below = true;
                    }
                }
                Some(
                    grid.column_bits
                        .entry(col)
                        .intersection(grid.row_bits.entry(row))
                        .iter(),
                )
            })
            .flatten()
            .filter(move |&id| self.aabbs[id].contains_point(point_worldspace))
            .map(move |id| NodeKey {
                idx: id,
                gen: self.generations[id],
                _marker: std::marker::PhantomData,
            })
    }

    pub(crate) fn traverse_ray(&mut self, ray: Ray, dist_limit: f64) -> RayTraversal<'_> {
        let ray_worldspace = ray;
        let ray_start_in_grid = ray_worldspace.start - self.bounds.min;
        // wrap ray start to the grid
        let ray_start_in_grid = m::Vec2::new(
            ray_start_in_grid.x.rem_euclid(self.bounds.width()),
            ray_start_in_grid.y.rem_euclid(self.bounds.height()),
        );
        let ray_gridspace = Ray {
            start: ray_start_in_grid,
            dir: ray_worldspace.dir,
        };

        let top_grid = &self.grids[self.grids.len() - 1];
        // no need to wrap these because we wrapped ray.start
        let start_col = (ray_gridspace.start.x / top_grid.spacing) as usize;
        let start_row = (ray_gridspace.start.y / top_grid.spacing) as usize;

        let ray_iter_stack = Vec::with_capacity(self.grids.len());
        RayTraversal {
            hgrid: self,
            t_limit: dist_limit,
            ray_gridspace,
            curr_cell: (start_col, start_row),
            last_axis_crossed: None,
            t: 0.0,
            t_past: 0.0,
            first_returned: false,
            ray_iter_stack,
        }
    }

    // for debug visualization
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
pub(crate) struct GridCell {
    pub grid_idx: usize,
    pub col_idx: usize,
    pub row_idx: usize,
}

//
// raycasting
// (woah, this took a lot more lines than I expected)
//

// track last crossed axis to figure out where we are when recursing to lower grid levels
#[derive(Clone, Copy, Debug)]
enum Axis {
    X,
    Y,
}

pub(crate) struct RayTraversal<'a> {
    // params
    hgrid: &'a mut HGrid,
    t_limit: f64,

    // state
    ray_gridspace: Ray,
    curr_cell: (usize, usize),
    last_axis_crossed: Option<Axis>,
    /// ray_gridspace is moved whenever it wraps around, t since that last happened
    t: f64,
    /// total t stored to determine stop condition
    t_past: f64,
    first_returned: bool,
    // Grid iterators are recursive and need some form of indirection
    // to represent that recursion.
    // Use a stack where recursion is represented by pushing and popping.
    // This way we only have to allocate once per raycast
    // (as opposed to boxing per iterator which would allocate a whole lot).
    //
    // Note: This stack could even be stored in the top-level HGrid and allocated
    // at grid creation time, but the cost of storing it with the iterator
    // isn't that big, and I might want parallel raycasts in the future
    // which would require this anyway. (requires replacing timestamping with something else)
    ray_iter_stack: Vec<SubgridRayIter>,
}

impl<'a> RayTraversal<'a> {
    /// Step to the next cell of the top-level grid
    /// and return an iterator over all colliders in that area on any of the grid levels.
    pub fn step(&mut self) -> Option<CellRayIter<'_>> {
        if !self.first_returned {
            self.first_returned = true;
            return Some(CellRayIter::new(self));
        }
        let top_grid = &self.hgrid.grids[self.hgrid.grids.len() - 1];

        let next_xlim = if self.ray_gridspace.dir.x >= 0.0 {
            self.curr_cell.0 + 1
        } else {
            // we're at the rightmost edge of the cell coming backwards
            self.curr_cell.0
        };
        let next_xlim = next_xlim as f64 * top_grid.spacing;
        let next_ylim = if self.ray_gridspace.dir.y >= 0.0 {
            self.curr_cell.1 + 1
        } else {
            self.curr_cell.1
        };
        let next_ylim = next_ylim as f64 * top_grid.spacing;

        let t_to_x = (next_xlim - self.ray_gridspace.start.x) / self.ray_gridspace.dir.x;
        let t_to_y = (next_ylim - self.ray_gridspace.start.y) / self.ray_gridspace.dir.y;
        // pick the closer boundary.
        // if dir.y is 0, x is picked.
        // if dir.x is 0, dir.y isn't 0 and t_to_x is inf -> both these are false so y is picked.
        if self.ray_gridspace.dir.y == 0.0 || t_to_x < t_to_y {
            self.t = t_to_x;
            self.last_axis_crossed = Some(Axis::Y);
            if self.t + self.t_past >= self.t_limit {
                return None;
            }
            if self.ray_gridspace.dir.x >= 0.0 {
                if self.curr_cell.0 + 1 == top_grid.column_count {
                    // wrap right to left
                    self.curr_cell.0 = 0;
                    self.ray_gridspace.start.y += self.t * self.ray_gridspace.dir.y;
                    self.ray_gridspace.start.x = 0.0;
                    self.t_past += self.t;
                    self.t = 0.0;
                } else {
                    // move right
                    self.curr_cell.0 += 1;
                }
            } else if self.curr_cell.0 == 0 {
                // wrap left to right
                self.curr_cell.0 = top_grid.column_count - 1;
                self.ray_gridspace.start.y += self.t * self.ray_gridspace.dir.y;
                self.ray_gridspace.start.x = self.hgrid.bounds.width();
                self.t_past += self.t;
                self.t = 0.0;
            } else {
                // move left
                self.curr_cell.0 -= 1;
            }
        } else {
            self.t = t_to_y;
            self.last_axis_crossed = Some(Axis::X);
            if self.t + self.t_past >= self.t_limit {
                return None;
            }
            if self.ray_gridspace.dir.y >= 0.0 {
                if self.curr_cell.1 + 1 == top_grid.row_count {
                    // wrap top to bottom
                    self.curr_cell.1 = 0;
                    self.ray_gridspace.start.x += self.t * self.ray_gridspace.dir.x;
                    self.ray_gridspace.start.y = 0.0;
                    self.t_past += self.t;
                    self.t = 0.0;
                } else {
                    // move upward
                    self.curr_cell.1 += 1;
                }
            } else if self.curr_cell.1 == 0 {
                // wrap bottom to top
                self.curr_cell.1 = top_grid.row_count - 1;
                self.ray_gridspace.start.x += self.t * self.ray_gridspace.dir.x;
                self.ray_gridspace.start.y = self.hgrid.bounds.height();
                self.t_past += self.t;
                self.t = 0.0;
            } else {
                // move downward
                self.curr_cell.1 -= 1;
            }
        }

        Some(CellRayIter::new(self))
    }
}

/// Iterator that recursively traverses every grid level below a cell of the top-level grid.
pub(crate) struct CellRayIter<'a> {
    // destructured borrows from hgrid and ray needed for lifetime purposes
    ray: Ray,
    grids: &'a [Grid],
    ray_timestamp: u16,
    timestamps: &'a mut [u16],
    generations: &'a [usize],
    spacing_ratio: usize,
    t_limit: f64,

    curr_cell_iter: EntryIter<EntryIntersection<'a>>,
    grid_iter_stack: &'a mut Vec<SubgridRayIter>,
}

impl<'a> CellRayIter<'a> {
    fn new(ray_tr: &'a mut RayTraversal<'_>) -> Self {
        let grids = &ray_tr.hgrid.grids;
        let top_grid_lvl = grids.len() - 1;
        let top_grid = &grids[top_grid_lvl];
        let ray = ray_tr.ray_gridspace;
        let top_lvl_cell_iter = top_grid
            .column_bits
            .entry(ray_tr.curr_cell.0)
            .intersection(top_grid.row_bits.entry(ray_tr.curr_cell.1))
            .iter();

        ray_tr.hgrid.last_timestamp += 1;

        let grid_iter_stack = &mut ray_tr.ray_iter_stack;

        grid_iter_stack.push(SubgridRayIter {
            grid_level: top_grid_lvl,
            // use current cell as bounds, this will stop traversal immediately regardless of direction
            bound_cells: ray_tr.curr_cell,
            curr_cell: ray_tr.curr_cell,
            t: ray_tr.t,
            last_axis_crossed: ray_tr.last_axis_crossed,
            already_recursed_this_cell: false,
        });
        Self {
            ray,
            grids,
            ray_timestamp: ray_tr.hgrid.last_timestamp,
            timestamps: &mut ray_tr.hgrid.timestamps,
            generations: &ray_tr.hgrid.generations,
            spacing_ratio: ray_tr.hgrid.spacing_ratio,
            t_limit: ray_tr.t_limit - ray_tr.t_past,

            curr_cell_iter: top_lvl_cell_iter,
            grid_iter_stack,
        }
    }
}

impl<'a> Iterator for CellRayIter<'a> {
    type Item = NodeKey<Collider>;

    fn next(&mut self) -> Option<Self::Item> {
        // return info from recursion, used to step for free on upper level
        let mut rec_ret_info: Option<StepReturn> = None;
        loop {
            // finish checking current cell
            for coll_idx in self.curr_cell_iter {
                if self.timestamps[coll_idx] == self.ray_timestamp {
                    continue;
                }
                self.timestamps[coll_idx] = self.ray_timestamp;
                // probably not worth doing an AABB check here, ray-shape queries
                // for the actual collider shapes are close to as fast
                return Some(NodeKey {
                    idx: coll_idx,
                    gen: self.generations[coll_idx],
                    _marker: std::marker::PhantomData,
                });
            }

            // step to next cell, possibly up or down a grid level

            // unwrap is safe here because we check for empty stack on pop
            let curr_iter_lvl = self.grid_iter_stack.iter_mut().last().unwrap();
            let step = curr_iter_lvl.step(self.ray, self.grids, self.spacing_ratio, rec_ret_info);
            // take these out here so we can drop the reference
            // and possibly push or pop from stack in the match
            let curr_lvl_curr_cell = curr_iter_lvl.curr_cell;
            let curr_grid_lvl = curr_iter_lvl.grid_level;
            let curr_t = curr_iter_lvl.t;
            match step {
                SubgridStep::Recurse(next_iter) => {
                    let grid = &self.grids[next_iter.grid_level];
                    self.curr_cell_iter = grid
                        .column_bits
                        .entry(next_iter.curr_cell.0)
                        .intersection(grid.row_bits.entry(next_iter.curr_cell.1))
                        .iter();
                    self.grid_iter_stack.push(next_iter);
                }
                SubgridStep::Step => {
                    // early out in case we hit the distance limit.
                    // because we iterate grids from big to small,
                    // any subsequent cells will be farther along the ray than this.
                    if curr_t > self.t_limit {
                        return None;
                    }
                    let grid = &self.grids[curr_grid_lvl];
                    self.curr_cell_iter = grid
                        .column_bits
                        .entry(curr_lvl_curr_cell.0)
                        .intersection(grid.row_bits.entry(curr_lvl_curr_cell.1))
                        .iter();
                }
                SubgridStep::Return(ret_info) => {
                    rec_ret_info = Some(ret_info);
                    self.grid_iter_stack.pop();
                    if self.grid_iter_stack.is_empty() {
                        return None;
                    }
                    // loop back to the start and step the previous level iter
                }
            }
        }
    }
}

/// The recursive part of the cell iterator, iterating over a region in a subgrid.
#[derive(Debug)]
struct SubgridRayIter {
    grid_level: usize,
    bound_cells: (usize, usize),
    curr_cell: (usize, usize),
    t: f64,
    last_axis_crossed: Option<Axis>,
    already_recursed_this_cell: bool,
}

#[derive(Debug)]
enum SubgridStep {
    Recurse(SubgridRayIter),
    Step,
    Return(StepReturn),
}

/// Return where a lower-level iterator left its area
/// to step on the upper level for free
#[derive(Debug, Clone, Copy)]
struct StepReturn {
    t: f64,
    leave_axis: Axis,
}

impl SubgridRayIter {
    fn step(
        &mut self,
        ray: Ray,
        grids: &[Grid],
        spacing_ratio: usize,
        recurse_return: Option<StepReturn>,
    ) -> SubgridStep {
        if !self.already_recursed_this_cell {
            self.already_recursed_this_cell = true;
            // recurse to lower levels if there are any with stuff
            match &grids[self.grid_level].subgrid_mask {
                Some(mask) if mask.entry(self.curr_cell.0).has(self.curr_cell.1) => {
                    let next_lvl = self.grid_level - 1;
                    // bounds are inclusive
                    // so that we don't end up with negative numbers at the low edges
                    let bound_col = if ray.dir.x >= 0.0 {
                        (self.curr_cell.0 + 1) * spacing_ratio - 1
                    } else {
                        self.curr_cell.0 * spacing_ratio
                    };
                    let bound_row = if ray.dir.y >= 0.0 {
                        (self.curr_cell.1 + 1) * spacing_ratio - 1
                    } else {
                        self.curr_cell.1 * spacing_ratio
                    };

                    let first_subcell_col = match self.last_axis_crossed {
                        Some(Axis::Y) if ray.dir.x >= 0.0 => self.curr_cell.0 * spacing_ratio,
                        Some(Axis::Y) => (self.curr_cell.0 + 1) * spacing_ratio - 1,
                        Some(Axis::X) | None => {
                            (ray.point_at_t(self.t).x / grids[next_lvl].spacing) as usize
                        }
                    };
                    let first_subcell_row = match self.last_axis_crossed {
                        Some(Axis::X) if ray.dir.y >= 0.0 => self.curr_cell.1 * spacing_ratio,
                        Some(Axis::X) => (self.curr_cell.1 + 1) * spacing_ratio - 1,
                        Some(Axis::Y) | None => {
                            (ray.point_at_t(self.t).y / grids[next_lvl].spacing) as usize
                        }
                    };

                    return SubgridStep::Recurse(SubgridRayIter {
                        grid_level: next_lvl,
                        bound_cells: (bound_col, bound_row),
                        curr_cell: (first_subcell_col, first_subcell_row),
                        t: self.t,
                        last_axis_crossed: self.last_axis_crossed,
                        already_recursed_this_cell: false,
                    });
                }
                _ => {
                    // nothing here, step to next cell
                }
            }
        }

        // step to next cell

        self.already_recursed_this_cell = false;

        let t_to_x;
        let t_to_y;
        match recurse_return {
            None => {
                // didn't come back from recursion, compute next intersection
                let next_xlim = if ray.dir.x >= 0.0 {
                    self.curr_cell.0 + 1
                } else {
                    self.curr_cell.0
                };
                let next_xlim = next_xlim as f64 * grids[self.grid_level].spacing;
                let next_ylim = if ray.dir.y >= 0.0 {
                    self.curr_cell.1 + 1
                } else {
                    self.curr_cell.1
                };
                let next_ylim = next_ylim as f64 * grids[self.grid_level].spacing;

                t_to_x = (next_xlim - ray.start.x) / ray.dir.x;
                t_to_y = (next_ylim - ray.start.y) / ray.dir.y;
            }
            Some(rec_ret) => {
                // came back from recursion, we already have next intersection from that.
                // use same variables so we don't have to write the upcoming branch twice
                match rec_ret.leave_axis {
                    Axis::Y => {
                        // recall that Axis is "axis crossed", and t_to_<axis> is
                        // "time until a boundary along axis".
                        // too lazy to rewrite these to be consistent, this is fine
                        t_to_x = rec_ret.t;
                        t_to_y = f64::MAX;
                    }
                    Axis::X => {
                        t_to_x = f64::MAX;
                        t_to_y = rec_ret.t;
                    }
                }
            }
        }
        // see comment on equivalent part of RayTraversal::step
        if ray.dir.y == 0.0 || t_to_x < t_to_y {
            if self.curr_cell.0 == self.bound_cells.0 {
                // bound reached, return to the previous grid level
                return SubgridStep::Return(StepReturn {
                    t: t_to_x,
                    leave_axis: Axis::Y,
                });
            }
            self.t = t_to_x;
            self.last_axis_crossed = Some(Axis::Y);
            if ray.dir.x >= 0.0 {
                self.curr_cell.0 += 1;
            } else {
                self.curr_cell.0 -= 1;
            }
        } else {
            self.t = t_to_y;
            self.last_axis_crossed = Some(Axis::X);
            if self.curr_cell.1 == self.bound_cells.1 {
                return SubgridStep::Return(StepReturn {
                    t: t_to_y,
                    leave_axis: Axis::X,
                });
            }
            if ray.dir.y >= 0.0 {
                self.curr_cell.1 += 1;
            } else {
                self.curr_cell.1 -= 1;
            }
        }
        SubgridStep::Step
    }
}
