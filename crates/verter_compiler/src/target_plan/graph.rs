use std::cmp::Reverse;
use std::collections::BinaryHeap;

use crate::framework_common::structure::{Arena, Id, StructureError, TargetScratch};

/// Compact directed graph: `Copy` node records in one table, edges of each
/// node in one contiguous range. Built once, then read-only.
pub struct RelationGraph<K, N: Copy, E: Copy> {
    nodes: Arena<K, N, TargetScratch>,
    /// `offsets[i]..offsets[i + 1]` is node `i`'s range in `edges`.
    offsets: Vec<u32>,
    edges: Vec<(Id<K>, E)>,
}

impl<K, N: Copy, E: Copy> RelationGraph<K, N, E> {
    pub fn node(&self, id: Id<K>) -> Option<&N> {
        self.nodes.get(id)
    }
    /// Outgoing edges of `id`, in insertion order.
    pub fn edges(&self, id: Id<K>) -> &[(Id<K>, E)] {
        let index = id.index();
        match (self.offsets.get(index), self.offsets.get(index + 1)) {
            (Some(&start), Some(&end)) => &self.edges[start as usize..end as usize],
            _ => &[],
        }
    }
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }
    pub fn edge_count(&self) -> usize {
        self.edges.len()
    }
    /// Node identities in an order where every edge goes forward; `None`
    /// when the graph has a cycle. Among nodes ready at the same time, the
    /// lower node index comes first.
    pub fn topological_order(&self) -> Option<Vec<Id<K>>> {
        let ids: Vec<Id<K>> = self.nodes.ids().collect();
        let mut in_degree = vec![0u32; ids.len()];
        for (to, _) in &self.edges {
            in_degree[to.index()] += 1;
        }
        let mut ready: BinaryHeap<Reverse<usize>> = in_degree
            .iter()
            .enumerate()
            .filter(|(_, degree)| **degree == 0)
            .map(|(index, _)| Reverse(index))
            .collect();
        let mut order = Vec::with_capacity(ids.len());
        while let Some(Reverse(index)) = ready.pop() {
            let id = ids[index];
            order.push(id);
            for (to, _) in self.edges(id) {
                let degree = &mut in_degree[to.index()];
                *degree -= 1;
                if *degree == 0 {
                    ready.push(Reverse(to.index()));
                }
            }
        }
        (order.len() == ids.len()).then_some(order)
    }
}

pub struct RelationGraphBuilder<K, N: Copy, E: Copy> {
    nodes: Arena<K, N, TargetScratch>,
    /// `(source index, target, edge)` in insertion order.
    pending: Vec<(u32, Id<K>, E)>,
}

impl<K, N: Copy, E: Copy> RelationGraphBuilder<K, N, E> {
    pub fn new() -> Self {
        Self {
            nodes: Arena::new(),
            pending: Vec::new(),
        }
    }
    pub fn add_node(&mut self, node: N) -> Result<Id<K>, StructureError> {
        self.nodes.push(node)
    }
    /// Both ends must be nodes of this builder.
    pub fn add_edge(&mut self, from: Id<K>, to: Id<K>, edge: E) -> Result<(), StructureError> {
        if self.nodes.get(from).is_none() || self.nodes.get(to).is_none() {
            return Err(StructureError::UnknownId);
        }
        if self.pending.len() >= u32::MAX as usize {
            return Err(StructureError::Capacity);
        }
        // `from` indexes a stored node, and node identities fit in `u32`.
        self.pending.push((from.index() as u32, to, edge));
        Ok(())
    }
    /// Group the edges by source node, keeping insertion order within each
    /// node.
    pub fn finish(self) -> RelationGraph<K, N, E> {
        let Self { nodes, mut pending } = self;
        pending.sort_by_key(|(from, _, _)| *from);
        let mut offsets = vec![0u32; nodes.len() + 1];
        for (from, _, _) in &pending {
            offsets[*from as usize + 1] += 1;
        }
        let mut running = 0;
        for offset in &mut offsets {
            running += *offset;
            *offset = running;
        }
        let edges = pending
            .into_iter()
            .map(|(_, to, edge)| (to, edge))
            .collect();
        RelationGraph {
            nodes,
            offsets,
            edges,
        }
    }
}

impl<K, N: Copy, E: Copy> Default for RelationGraphBuilder<K, N, E> {
    fn default() -> Self {
        Self::new()
    }
}

// An edge is a node identity and a one-byte label in one 8-byte slot.
const _: () = assert!(std::mem::size_of::<(Id<()>, u8)>() <= 8);

#[cfg(test)]
mod tests {
    use super::*;

    enum Node {}
    type Builder = RelationGraphBuilder<Node, char, u8>;

    fn nodes(builder: &mut Builder, names: &str) -> Vec<Id<Node>> {
        names
            .chars()
            .map(|name| builder.add_node(name).unwrap())
            .collect()
    }

    #[test]
    fn edges_keep_insertion_order_per_node() {
        let mut builder = Builder::new();
        let n = nodes(&mut builder, "abc");
        builder.add_edge(n[2], n[0], 1).unwrap();
        builder.add_edge(n[0], n[2], 2).unwrap();
        builder.add_edge(n[2], n[1], 3).unwrap();
        builder.add_edge(n[0], n[1], 4).unwrap();
        builder.add_edge(n[2], n[2], 5).unwrap();

        let mut foreign = Builder::new();
        let outside = nodes(&mut foreign, "abcd")[3];
        assert_eq!(
            builder.add_edge(n[0], outside, 0),
            Err(StructureError::UnknownId)
        );

        let graph = builder.finish();
        assert_eq!(graph.node_count(), 3);
        assert_eq!(graph.edge_count(), 5);
        assert_eq!(graph.node(n[1]), Some(&'b'));
        assert_eq!(graph.edges(n[0]), &[(n[2], 2), (n[1], 4)]);
        assert!(graph.edges(n[1]).is_empty());
        assert_eq!(graph.edges(n[2]), &[(n[0], 1), (n[1], 3), (n[2], 5)]);
        assert!(graph.edges(outside).is_empty());
        assert_eq!(graph.node(outside), None);
    }

    #[test]
    fn a_cycle_has_no_topological_order() {
        let mut builder = Builder::new();
        let n = nodes(&mut builder, "abcd");
        builder.add_edge(n[0], n[1], 0).unwrap();
        builder.add_edge(n[1], n[2], 0).unwrap();
        builder.add_edge(n[2], n[1], 0).unwrap();
        builder.add_edge(n[3], n[0], 0).unwrap();
        assert_eq!(builder.finish().topological_order(), None);

        let mut looped = Builder::new();
        let n = nodes(&mut looped, "a");
        looped.add_edge(n[0], n[0], 0).unwrap();
        assert_eq!(looped.finish().topological_order(), None);
    }

    #[test]
    fn a_dag_orders_every_edge_forward() {
        let mut builder = Builder::new();
        let n = nodes(&mut builder, "abcde");
        // e -> c -> a, d -> a, b free: ready ties resolve by node index.
        builder.add_edge(n[4], n[2], 0).unwrap();
        builder.add_edge(n[2], n[0], 0).unwrap();
        builder.add_edge(n[3], n[0], 0).unwrap();
        let graph = builder.finish();

        let order = graph.topological_order().unwrap();
        assert_eq!(order, [n[1], n[3], n[4], n[2], n[0]]);
        let position = |id: Id<Node>| order.iter().position(|o| *o == id).unwrap();
        for from in &order {
            for (to, _) in graph.edges(*from) {
                assert!(position(*from) < position(*to));
            }
        }
        assert_eq!(Builder::new().finish().topological_order(), Some(vec![]));
    }
}
