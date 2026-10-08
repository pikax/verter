//! The prepared containment / read index of one function frame.
//!
//! A loop's dependencies are the writes and inferred declarations its span
//! contains and the bindings each of their value sites reads. Answering that
//! by filtering every site, write and binding of the frame per question costs
//! the frame's size per write; this index answers it from sorted span tables
//! instead, so a question inspects only the entries that START inside the span
//! asked about — exactly its descendants when spans nest, plus any
//! overlapping entry the containment check then rejects.
//!
//! The index is built once, in [`prepare_function_body_skeleton`], from the
//! same skeleton and binding map it indexes, and is retained with them. It
//! holds positions only: every answer is an id into the skeleton's own tables,
//! so the reads a caller follows are the sites' own resolved reads, never a
//! copied transitive summary.
//!
//! [`prepare_function_body_skeleton`]: super::skeleton::prepare_function_body_skeleton

use crate::flow::binding::{FlowBindingMap, FlowBindingRef};
use crate::flow::frame_span::FrameSpan;
use crate::flow::skeleton::{
    FunctionBodySkeleton, SkeletonBindingId, SkeletonExprSiteId, SkeletonPathSegment,
};
use rustc_hash::FxHashMap;
use std::sync::Arc;
use verter_no_typeexpr::NoTypeExpr;

/// One read of one runtime variable, keyed for "is it read after this span".
#[derive(Debug, Clone, PartialEq, Eq, NoTypeExpr)]
pub struct IndexedRead {
    /// The span of the site the read is attributed to.
    pub site_span: FrameSpan,
    /// The read's projection path.
    pub path: Arc<[SkeletonPathSegment]>,
}

/// The containment and read index of one prepared skeleton.
///
/// The three containment tables hold `(span, id)` in nesting order (start
/// ascending, the wider span first at an equal start). The read table groups
/// every site read by its runtime variable — a frame local by its canonical
/// binding, a captured cell by its identity — each group in site-span order.
///
/// An unprepared skeleton carries the empty index; only
/// [`prepare_function_body_skeleton`](super::skeleton::prepare_function_body_skeleton)
/// fills it.
#[derive(Debug, Clone, Default, PartialEq, Eq, NoTypeExpr)]
pub struct SkeletonSpanIndex {
    sites: Arc<[(FrameSpan, SkeletonExprSiteId)]>,
    writes: Arc<[(FrameSpan, u32)]>,
    bindings: Arc<[(FrameSpan, SkeletonBindingId)]>,
    reads: Arc<[IndexedRead]>,
    read_groups: Arc<FxHashMap<FlowBindingRef, (u32, u32)>>,
}

impl SkeletonSpanIndex {
    /// Index `skeleton`'s sites, writes, bindings and resolved reads.
    pub(crate) fn build(skeleton: &FunctionBodySkeleton, bindings: &FlowBindingMap) -> Self {
        let sites = nesting_sorted(
            skeleton
                .expr_sites
                .iter()
                .enumerate()
                .map(|(index, site)| (site.span, SkeletonExprSiteId::from_index(index as u32))),
        );
        let writes = nesting_sorted(
            skeleton
                .writes
                .iter()
                .enumerate()
                .map(|(index, write)| (write.span, index as u32)),
        );
        let binding_spans = nesting_sorted(
            skeleton
                .bindings
                .iter()
                .enumerate()
                .map(|(index, binding)| {
                    (binding.span, SkeletonBindingId::from_index(index as u32))
                }),
        );
        let mut keyed: Vec<(u32, IndexedRead)> = Vec::new();
        let mut keys: FxHashMap<FlowBindingRef, u32> = FxHashMap::default();
        let mut key_order: Vec<FlowBindingRef> = Vec::new();
        for site in skeleton.expr_sites.iter() {
            for read in site.reads.iter() {
                let Some(binding) = read.binding.as_ref() else {
                    continue;
                };
                let key = canonical(bindings, binding);
                let next = key_order.len() as u32;
                let group = *keys.entry(key.clone()).or_insert_with(|| {
                    key_order.push(key);
                    next
                });
                keyed.push((
                    group,
                    IndexedRead {
                        site_span: site.span,
                        path: Arc::clone(&read.path),
                    },
                ));
            }
        }
        keyed.sort_by(|(left_group, left), (right_group, right)| {
            left_group
                .cmp(right_group)
                .then(left.site_span.cmp(&right.site_span))
        });
        let mut read_groups: FxHashMap<FlowBindingRef, (u32, u32)> = FxHashMap::default();
        let mut start = 0usize;
        while start < keyed.len() {
            let group = keyed[start].0;
            let end = start + keyed[start..].partition_point(|(other, _)| *other == group);
            read_groups.insert(
                key_order[group as usize].clone(),
                (start as u32, end as u32),
            );
            start = end;
        }
        Self {
            sites,
            writes,
            bindings: binding_spans,
            reads: keyed.into_iter().map(|(_, read)| read).collect(),
            read_groups: Arc::new(read_groups),
        }
    }

    /// Whether this index was built over `skeleton`'s tables. An unprepared
    /// skeleton carries the empty index, which would answer every question
    /// with silence rather than refuse.
    #[must_use]
    pub fn covers(&self, skeleton: &FunctionBodySkeleton) -> bool {
        self.sites.len() == skeleton.expr_sites.len()
            && self.writes.len() == skeleton.writes.len()
            && self.bindings.len() == skeleton.bindings.len()
    }

    /// How many entries construction indexed (sites, writes, bindings and
    /// grouped reads): its work is one sort over each table, so this is the
    /// size the construction cost scales with.
    #[must_use]
    pub fn indexed_entries(&self) -> usize {
        self.sites.len() + self.writes.len() + self.bindings.len() + self.reads.len()
    }

    /// Every expression site `span` contains, in table (source) order.
    #[must_use]
    pub fn sites_within(&self, span: FrameSpan) -> Vec<SkeletonExprSiteId> {
        let mut found = within(&self.sites, span);
        found.sort_unstable_by_key(|site| site.index());
        found
    }

    /// The ordinal of every write `span` contains, in table (source) order.
    #[must_use]
    pub fn writes_within(&self, span: FrameSpan) -> Vec<usize> {
        let mut found = within(&self.writes, span);
        found.sort_unstable();
        found.into_iter().map(|write| write as usize).collect()
    }

    /// Every binding whose identifier `span` contains, in table order.
    #[must_use]
    pub fn bindings_within(&self, span: FrameSpan) -> Vec<SkeletonBindingId> {
        let mut found = within(&self.bindings, span);
        found.sort_unstable_by_key(|binding| binding.index());
        found
    }

    /// The reads of `binding` (a frame local by its canonical binding, a
    /// captured cell by its identity) whose site follows `span` without
    /// lying inside it — the reads that observe a value `span` leaves
    /// behind.
    pub fn reads_after<'a>(
        &'a self,
        binding: &FlowBindingRef,
        span: FrameSpan,
    ) -> impl Iterator<Item = &'a IndexedRead> + 'a {
        let group = self
            .read_groups
            .get(binding)
            .map_or(&[][..], |&(start, end)| {
                &self.reads[start as usize..end as usize]
            });
        let first = group.partition_point(|read| read.site_span <= span);
        group[first..]
            .iter()
            .inspect(|_| record_visits(1))
            .filter(move |read| !span.contains(read.site_span))
    }
}

/// A frame local by its canonical binding; any other reference as it is.
fn canonical(bindings: &FlowBindingMap, binding: &FlowBindingRef) -> FlowBindingRef {
    match binding {
        FlowBindingRef::Local(local) => FlowBindingRef::Local(bindings.canonical_local(*local)),
        captured @ FlowBindingRef::Captured(_) => captured.clone(),
    }
}

fn nesting_sorted<T>(entries: impl Iterator<Item = (FrameSpan, T)>) -> Arc<[(FrameSpan, T)]> {
    let mut entries: Vec<(FrameSpan, T)> = entries.collect();
    entries.sort_by(|(left, _), (right, _)| left.nesting_cmp(*right));
    Arc::from(entries.into_boxed_slice())
}

/// The ids whose span `span` contains: the run of entries starting inside
/// `span`, minus any that end past it.
fn within<T: Copy>(entries: &[(FrameSpan, T)], span: FrameSpan) -> Vec<T> {
    let first = entries.partition_point(|(candidate, _)| candidate.starts_before(span));
    let mut found = Vec::new();
    let mut visited = 0usize;
    for &(candidate, id) in &entries[first..] {
        if candidate.starts_after_end_of(span) {
            break;
        }
        visited += 1;
        if span.contains(candidate) {
            found.push(id);
        }
    }
    record_visits(visited);
    found
}

#[cfg(any(test, feature = "test-support", feature = "semantic-observe"))]
std::thread_local! {
    static SPAN_INDEX_VISITS: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// Entries the containment and read queries have inspected on this thread.
/// Measurement only: production reads none of it.
#[cfg(any(test, feature = "test-support", feature = "semantic-observe"))]
#[must_use]
pub fn span_index_visits() -> u64 {
    SPAN_INDEX_VISITS.with(std::cell::Cell::get)
}

#[inline]
fn record_visits(visited: usize) {
    #[cfg(any(test, feature = "test-support", feature = "semantic-observe"))]
    SPAN_INDEX_VISITS.with(|visits| visits.set(visits.get() + visited as u64));
    #[cfg(not(any(test, feature = "test-support", feature = "semantic-observe")))]
    let _ = visited;
}

#[cfg(test)]
#[path = "span_index_tests.rs"]
mod tests;
