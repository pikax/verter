//! Framework-neutral, snapshot-local compiler storage.
//!
//! Frameworks supply their own node and region payloads and collection marker
//! types. IDs are dense indices scoped to the originating snapshot/collection,
//! never public/cache identities or authored offsets. There is no framework
//! dispatch, artifact assembly, fallback traversal, or semantic node taxonomy.
//! Raw text borrows the source; only explicit interning allocates text.
//!
//! Build in preorder with [`Structure`], then freeze into [`TopologyStore`].
//! Storage is O(nodes + regions + relations), independent of source length.
//! Persistent node side tables cost 36 bytes/node (20 topology, 8 span,
//! 8 sorted-start entry), plus the framework payload and region storage.
//! Construction pushes/pops each node once; parent lookup is O(log depth),
//! and final source-index sorting is O(n log n). Scratch ancestor/last-child
//! tables are released at freeze. No parse, resolve, emit or text copy occurs.
//! Lifetime markers keep separately owned storage classes explicit; they do not
//! extend Rust borrows or permit mixing classes. Topology is frontend-owned.

mod storage;
pub use storage::*;
use verter_span::Span;

/// Invalid input never publishes a partial node or reopens a closed subtree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StructureError {
    Capacity,
    InvalidSpan,
    UnknownId,
    ClosedParent,
    RegionOwnership,
}

pub type NodeId<K> = Id<K>;
/// Separate namespace for control-flow regions, without universal region kinds.
pub struct RegionKey<K>(std::marker::PhantomData<fn() -> K>);
pub type RegionId<K> = Id<RegionKey<K>>;

/// Compact topology side table; payload and source spans are separate dense
/// tables. Preorder is the node index; subtree_end is an exclusive index.
pub struct Topology<K> {
    parent: Option<NodeId<K>>,
    first_child: Option<NodeId<K>>,
    next_sibling: Option<NodeId<K>>,
    subtree_end: u32,
    region: u32,
}
impl<K> Copy for Topology<K> {}
impl<K> Clone for Topology<K> {
    fn clone(&self) -> Self {
        *self
    }
}
struct Region<K, R: Copy> {
    parent: Option<RegionId<R>>,
    owner: Option<NodeId<K>>,
    data: R,
}
impl<K, R: Copy> Copy for Region<K, R> {}
impl<K, R: Copy> Clone for Region<K, R> {
    fn clone(&self) -> Self {
        *self
    }
}

/// Preorder builder. Parent nodes must still be on the open ancestor stack.
/// A region's owner is the node immediately outside its body; nested regions
/// must name that owner's region as their parent. Root regions have no owner.
/// Framework-specific branch/body meaning stays in the region payload `R`.
pub struct Structure<'s, K, R: Copy, N: Copy> {
    store: TopologyStore<'s, K, R, N>,
    open: Vec<NodeId<K>>,
    last_child: Vec<Option<NodeId<K>>>,
    last_root: Option<NodeId<K>>,
}
/// Frozen topology, regions, compact node facts, and source-position side tables.
/// No mutation or fallback can change identity after construction.
pub struct TopologyStore<'s, K, R: Copy, N: Copy> {
    source: &'s str,
    nodes: Arena<K, N, Frontend>,
    topology: Vec<Topology<K>>,
    spans: Vec<Span>,
    starts: Vec<(u32, NodeId<K>)>,
    regions: Arena<RegionKey<R>, Region<K, R>, Frontend>,
}
impl<'s, K, R: Copy, N: Copy> Structure<'s, K, R, N> {
    pub fn new(source: &'s str) -> Self {
        Self {
            store: TopologyStore {
                source,
                nodes: Arena::new(),
                topology: Vec::new(),
                spans: Vec::new(),
                starts: Vec::new(),
                regions: Arena::new(),
            },
            open: Vec::new(),
            last_child: Vec::new(),
            last_root: None,
        }
    }
    pub fn add_region(
        &mut self,
        parent: Option<RegionId<R>>,
        owner: Option<NodeId<K>>,
        data: R,
    ) -> Result<RegionId<R>, StructureError> {
        match (parent, owner) {
            (None, None) => {}
            (Some(parent), Some(owner)) => {
                if self.store.regions.get(parent).is_none()
                    || self.store.region(owner) != Some(parent)
                {
                    return Err(StructureError::RegionOwnership);
                }
            }
            _ => return Err(StructureError::RegionOwnership),
        }
        self.store.regions.push(Region {
            parent,
            owner,
            data,
        })
    }
    pub fn push(
        &mut self,
        parent: Option<NodeId<K>>,
        region: RegionId<R>,
        span: Span,
        data: N,
    ) -> Result<NodeId<K>, StructureError> {
        if span.start > span.end
            || self
                .store
                .source
                .get(span.start as usize..span.end as usize)
                .is_none()
        {
            return Err(StructureError::InvalidSpan);
        }
        let keep = match parent {
            Some(p) => self
                .open
                .binary_search_by_key(&p.index(), |n| n.index())
                .ok()
                .map(|i| i + 1)
                .ok_or(StructureError::ClosedParent)?,
            None => 0,
        };
        let r = self
            .store
            .regions
            .get(region)
            .ok_or(StructureError::UnknownId)?;
        let valid = match parent {
            None => r.parent.is_none() && r.owner.is_none(),
            Some(p) => self.store.region(p) == Some(region) || r.owner == Some(p),
        };
        if !valid {
            return Err(StructureError::RegionOwnership);
        }
        let id = Id::at(self.store.nodes.len())?;
        while self.open.len() > keep {
            let closed = self.open.pop().expect("open ancestor");
            self.store.topology[closed.index()].subtree_end = id.index() as u32;
        }
        let previous = match parent {
            Some(p) => self.last_child[p.index()].replace(id),
            None => self.last_root.replace(id),
        };
        if let Some(previous) = previous {
            self.store.topology[previous.index()].next_sibling = Some(id);
        } else if let Some(p) = parent {
            self.store.topology[p.index()].first_child = Some(id);
        }
        self.store.nodes.push(data)?;
        self.store.spans.push(span);
        self.store.starts.push((span.start, id));
        self.store.topology.push(Topology {
            parent,
            first_child: None,
            next_sibling: None,
            subtree_end: id.index() as u32 + 1,
            region: region.index() as u32,
        });
        self.last_child.push(None);
        self.open.push(id);
        Ok(id)
    }
    pub fn source_text(&self, id: NodeId<K>) -> Option<&'s str> {
        self.store.source_text(id)
    }
    pub fn finish(mut self) -> TopologyStore<'s, K, R, N> {
        let end = self.store.nodes.len() as u32;
        for id in self.open {
            self.store.topology[id.index()].subtree_end = end;
        }
        // Sort equal starts by dense identity for deterministic duplicate positions.
        self.store
            .starts
            .sort_unstable_by_key(|&(start, id)| (start, id.index()));
        self.store
    }
}
impl<'s, K, R: Copy, N: Copy> TopologyStore<'s, K, R, N> {
    pub fn node(&self, id: NodeId<K>) -> Option<&N> {
        self.nodes.get(id)
    }
    pub fn span(&self, id: NodeId<K>) -> Option<Span> {
        self.spans.get(id.index()).copied()
    }
    pub fn source_text(&self, id: NodeId<K>) -> Option<&'s str> {
        let span = self.span(id)?;
        self.source.get(span.start as usize..span.end as usize)
    }
    /// Exact authored-start lookup: O(log n + matching nodes), including duplicates.
    pub fn at_start(&self, offset: u32) -> impl Iterator<Item = NodeId<K>> + '_ {
        let start = self.starts.partition_point(|&(pos, _)| pos < offset);
        self.starts[start..]
            .iter()
            .take_while(move |&&(pos, _)| pos == offset)
            .map(|&(_, id)| id)
    }
    pub fn parent(&self, id: NodeId<K>) -> Option<NodeId<K>> {
        self.topology.get(id.index())?.parent
    }
    pub fn children(&self, id: NodeId<K>) -> impl Iterator<Item = NodeId<K>> + '_ {
        let first = self.topology.get(id.index()).and_then(|n| n.first_child);
        std::iter::successors(first, |n| self.topology[n.index()].next_sibling)
    }
    pub fn subtree(&self, id: NodeId<K>) -> Option<impl Iterator<Item = NodeId<K>>> {
        let end = self.topology.get(id.index())?.subtree_end;
        Some((id.index()..end as usize).map(|i| Id::at(i).expect("stored node index")))
    }
    pub fn region(&self, id: NodeId<K>) -> Option<RegionId<R>> {
        Id::at(self.topology.get(id.index())?.region as usize).ok()
    }
    pub fn region_data(&self, id: RegionId<R>) -> Option<&R> {
        Some(&self.regions.get(id)?.data)
    }
    pub fn region_parent(&self, id: RegionId<R>) -> Option<RegionId<R>> {
        self.regions.get(id)?.parent
    }
    pub fn region_owner(&self, id: RegionId<R>) -> Option<NodeId<K>> {
        self.regions.get(id)?.owner
    }
    pub fn len(&self) -> usize {
        self.nodes.len()
    }
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }
}
// Layout bounds are compile-time contracts, including in non-test builds.
const _: () = {
    assert!(std::mem::size_of::<NodeId<()>>() == 4);
    assert!(std::mem::size_of::<Option<NodeId<()>>>() == 4);
    assert!(std::mem::size_of::<Topology<()>>() == 20);
    assert!(std::mem::size_of::<Span>() == 8);
    assert!(std::mem::size_of::<(u32, NodeId<()>)>() == 8);
    assert!(std::mem::size_of::<FlatRange<()>>() == 8);
};
#[cfg(test)]
mod tests;
