//! Graph structure used to separate bodies into islands
//! for parallel computation and sleeping.

#[derive(Clone, Copy, Debug)]
pub enum Edge {
    Rope { body_idx: usize, rope_slot: usize },
    Constraint { body_idx: usize, constr_idx: usize },
    Contact { body_idx: usize, pair_idx: usize },
    // marking possible contacts and constraints with static objects as well
    // so that we can get this knowledge into the island solver
    StaticConstraint { constr_idx: usize },
    StaticContact { pair_idx: usize },
}

#[derive(Clone, Copy, Debug)]
pub struct EdgeListNode {
    pub next: Option<usize>,
    pub edge: Edge,
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

    pub fn insert(&mut self, body_idx: usize, edge: Edge) {
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

    pub fn iter(&self, body_idx: usize) -> ConstraintGraphIter<'_> {
        ConstraintGraphIter {
            graph: self,
            body_idx,
            curr_node_idx: None,
        }
    }
}

pub struct ConstraintGraphIter<'a> {
    graph: &'a ConstraintGraph,
    body_idx: usize,
    curr_node_idx: Option<usize>,
}

impl<'a> Iterator for ConstraintGraphIter<'a> {
    type Item = &'a Edge;

    fn next(&mut self) -> Option<Self::Item> {
        self.curr_node_idx = match self.curr_node_idx {
            Some(node_idx) => self.graph.nodes[node_idx].next,
            None => self.graph.first_nodes_per_body[self.body_idx],
        };
        self.curr_node_idx.map(|ni| &self.graph.nodes[ni].edge)
    }
}
