//! Graph structure used to separate bodies into islands
//! for parallel computation and sleeping.

/// An edge in the constraint graph.
///
/// This edge is directed; the body it starts at is implicitly defined in the structure.
/// For every body participating in a constraint,
/// as many of these are created as there are other bodies in the constraint,
/// or at a minimum one edge with other_body_idx set to None
/// if it's a single-body constraint or a contact against static geometry.
#[derive(Clone, Copy, Debug)]
pub enum ConstraintGraphEdge {
    Constraint {
        /// Index of the body this edge points towards in the global buffer of bodies.
        /// If None, that means a single-body constraint.
        other_body_idx: Option<usize>,
        /// Index of the constraint in the global buffer of constraints.
        constr_idx: usize,
        /// Index of the present body in the constraint's target list.
        instance_idx: usize,
    },
    Contact {
        /// Index of the body this edge points towards in the global buffer of bodies.
        /// If None, that means this is a contact against static geometry.
        other_body_idx: Option<usize>,
        /// Index of the constraint in the global buffer of constraints.
        pair_idx: usize,
        /// Index of the present body in the constraint's target list.
        instance_idx: usize,
    },
}

#[derive(Clone, Copy, Debug)]
pub struct EdgeListNode {
    pub next: Option<usize>,
    pub edge: ConstraintGraphEdge,
}

/// A set of single-linked lists stored in a Vec
/// for a memory-efficient graph representation
pub struct ConstraintGraph {
    // indices to the start of the list for reading and end for writing
    pub first_nodes_per_body: Vec<Option<usize>>,
    // last node exists for sure if first does, we always check first first
    pub last_nodes_per_body: Vec<usize>,
    pub nodes: Vec<EdgeListNode>,
}

impl ConstraintGraph {
    pub fn clear(&mut self) {
        self.first_nodes_per_body.clear();
        self.last_nodes_per_body.clear();
        self.nodes.clear();
    }

    pub fn resize(&mut self, new_len: usize) {
        self.first_nodes_per_body.resize(new_len, None);
        self.last_nodes_per_body.resize(new_len, 0);
    }

    pub fn insert(&mut self, body_idx: usize, edge: ConstraintGraphEdge) {
        let node_idx = self.nodes.len();
        self.nodes.push(EdgeListNode { next: None, edge });

        match self.first_nodes_per_body[body_idx] {
            Some(_first) => {
                self.nodes[self.last_nodes_per_body[body_idx]].next = Some(node_idx);
                self.last_nodes_per_body[body_idx] = node_idx;
            }
            None => {
                self.first_nodes_per_body[body_idx] = Some(node_idx);
                self.last_nodes_per_body[body_idx] = node_idx;
            }
        }
    }

    /// Iterate over constraints attached to the body at the given index.
    pub fn body_iter(&self, body_idx: usize) -> ConstraintGraphIter<'_> {
        ConstraintGraphIter {
            graph: self,
            body_idx,
            curr_node_idx: None,
        }
    }
}

/// Iterator over constraints attached to a specific body.
pub struct ConstraintGraphIter<'a> {
    graph: &'a ConstraintGraph,
    body_idx: usize,
    curr_node_idx: Option<usize>,
}

impl<'a> Iterator for ConstraintGraphIter<'a> {
    type Item = ConstraintGraphEdge;

    fn next(&mut self) -> Option<Self::Item> {
        self.curr_node_idx = match self.curr_node_idx {
            Some(node_idx) => self.graph.nodes[node_idx].next,
            None => self.graph.first_nodes_per_body[self.body_idx],
        };
        self.curr_node_idx.map(|ni| self.graph.nodes[ni].edge)
    }
}
