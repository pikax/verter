//! Path-precise projection demand substrate (Block 6.i).
//!
//! Encodes the published-surface demand at every projection boundary
//! so projectors / registry walker / fallthrough / cache keys can
//! gate descent on the published path the consumer actually walks.
//!
//! The substrate is built on the EXISTING `PathSegment` type from
//! [`crate::semantic_query`] (which already carries
//! `Member(Arc<str>)` and `Index(IndexKey)`); this module adds the
//! caller-side spec (`SurfaceProjection`, `ProjectionNode`,
//! `KeyFilter`, `PublishedSurfaceKind`) and the threading
//! mechanism (`ProjectionCursor<'a>`).
//!
//! ## Architectural framing
//!
//! - `SurfaceProjection` is the per-publication-boundary spec — a
//!   root `ProjectionNode` carrying per-child constraints plus the
//!   `PublishedSurfaceKind` discriminant for the caller surface
//!   (`Props` / `Emits` / `Slots` / `Exposed` / `Model` / `Registry`).
//! - `ProjectionNode` is a trie of `(PathSegment → child node)` with
//!   a `KeyFilter` at each hop (Pick → `Include`; Omit → `Exclude`;
//!   bare carrier → `All`).
//! - `KeyFilter::UnknownDeferred` is the explicit STOP marker for
//!   publication boundaries that still need pre-resolution — the
//!   architectural invariant under Block 6.i is that production
//!   publication code never observes `UnknownDeferred` at a hop it
//!   intends to walk (cf. STOP trigger #2: no eager fallback path).
//! - `ProjectionCursor<'a>` is the call-scoped threading mechanism:
//!   `descend(&segment) → Option<ProjectionCursor>` returns `None`
//!   when the segment is NOT in the cursor's allowed children, at
//!   which point the caller publishes the bare `Ref` and stops
//!   walking. Cursors are non-owning views into a
//!   `SurfaceProjection` so the recursive descent doesn't fan out
//!   allocation.
//!
//! ## Scope of use (Block 6.i)
//!
//! Commit A threads `ProjectionCursor` through
//! `produce_one_macro_object_shape`, its solver/projection helpers,
//! and `collect_component_meta_registry_refs`. Top-level callers
//! pass `SurfaceProjection::whole_surface(kind)` (which has
//! `KeyFilter::All` at the root) so behaviour is preserved for
//! sites that have not yet adopted path-precision. Subsequent
//! commits (B / C / D / E / F) progressively replace
//! `whole_surface` calls with narrowed projections.

#![allow(dead_code)] // Substrate; full surface adopted across Commits A–F.

use std::sync::Arc;

use crate::semantic_query::PathSegment;
use crate::types::ProjectionMode;
use rustc_hash::FxHashMap;

/// Which published surface a [`SurfaceProjection`] addresses.
///
/// Used by the registry walker to discriminate caller intent (the
/// registry-side walker enqueues differently than a per-macro
/// shape solver) and by the cache key to keep slot identity
/// disjoint across surfaces.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) enum PublishedSurfaceKind {
    Props,
    Emits,
    Slots,
    Exposed,
    Model,
    /// The component-meta type registry walker — sees imported
    /// types reachable from the published surface.
    Registry,
    /// A non-publication call site that still requires path
    /// precision (e.g. internal recursive projection). The
    /// `caller` string is the call-site identifier for the audit
    /// observer.
    Internal {
        caller: &'static str,
    },
}

/// Filter on a node's keys.
///
/// - `All`: every key participates (bare carrier / open object).
/// - `Include(set)`: only the listed keys (e.g., from `Pick<T, K>`).
/// - `Exclude(set)`: every key EXCEPT the listed (e.g., `Omit<T, K>`).
/// - `UnknownDeferred`: the demand is not resolved at this hop; the
///   caller MUST resolve before walking. Reaching `UnknownDeferred`
///   at the publication boundary is a Rule-5 violation site and
///   triggers a `debug_assert!` panic in the threaded helpers.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) enum KeyFilter {
    All,
    Include(Arc<[Arc<str>]>),
    Exclude(Arc<[Arc<str>]>),
    UnknownDeferred,
}

impl KeyFilter {
    /// Whether a candidate key passes this filter.
    pub(crate) fn admits(&self, key: &str) -> bool {
        match self {
            KeyFilter::All => true,
            KeyFilter::Include(set) => set.iter().any(|k| k.as_ref() == key),
            KeyFilter::Exclude(set) => set.iter().all(|k| k.as_ref() != key),
            // UnknownDeferred — at the publication boundary the
            // caller MUST resolve the filter before walking. The
            // threaded code paths assert this; treat as
            // conservative-admit here so a stale filter does not
            // cause silent member drops.
            KeyFilter::UnknownDeferred => true,
        }
    }
}

/// One node in a [`SurfaceProjection`] — encodes per-child
/// constraints plus the (terminal-only) mode to dispatch at.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ProjectionNode {
    /// Mode at THIS node when it is the terminal hop (`Some`);
    /// `None` means the hop is intermediate (route in `Navigate`).
    pub(crate) terminal_mode: Option<ProjectionMode>,
    /// Per-child constraints. Empty = no children explicitly walked
    /// (shallow at this hop's siblings). Stored as `FxHashMap` for
    /// O(1) `descend` lookup. `Hash` is implemented manually below
    /// by hashing a sorted view of the entries so the cache key
    /// stays deterministic.
    pub(crate) children: FxHashMap<PathSegment, ProjectionNode>,
    /// Filter on this node's keys.
    pub(crate) key_filter: KeyFilter,
}

impl std::hash::Hash for ProjectionNode {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        // Deterministic hash: sort entries by Debug-rendered key
        // form so two structurally-equal `ProjectionNode` values
        // hash identically regardless of map insertion order.
        self.terminal_mode.hash(state);
        let mut entries: Vec<_> = self.children.iter().collect();
        entries.sort_by(|(a, _), (b, _)| format!("{a:?}").cmp(&format!("{b:?}")));
        entries.len().hash(state);
        for (key, value) in entries {
            key.hash(state);
            value.hash(state);
        }
        self.key_filter.hash(state);
    }
}

impl ProjectionNode {
    /// A `ProjectionNode` representing "walk this hop's whole
    /// surface in `Expanded` mode": `terminal_mode = Some(Expanded)`,
    /// no children, `KeyFilter::All`.
    pub(crate) fn whole_surface_expanded() -> Self {
        Self {
            terminal_mode: Some(ProjectionMode::Expanded),
            children: FxHashMap::default(),
            key_filter: KeyFilter::All,
        }
    }

    /// A `ProjectionNode` representing "walk this hop's whole
    /// surface in `Shallow` mode".
    #[cfg(test)]
    pub(crate) fn whole_surface_shallow() -> Self {
        Self {
            terminal_mode: Some(ProjectionMode::Shallow),
            children: FxHashMap::default(),
            key_filter: KeyFilter::All,
        }
    }
}

/// Per-publication-boundary projection spec.
///
/// Carries the [`PublishedSurfaceKind`] and the root
/// [`ProjectionNode`]. Top-level publication call sites construct
/// this once at the boundary and thread a [`ProjectionCursor`]
/// (borrowed view) into the resolver helpers.
#[derive(Clone, Debug)]
pub(crate) struct SurfaceProjection {
    pub(crate) surface: PublishedSurfaceKind,
    pub(crate) root: ProjectionNode,
}

impl SurfaceProjection {
    /// "Whole surface in `Expanded` mode" — the substrate's
    /// pre-Block-6.i default. Used by call sites that have not yet
    /// adopted path-precision.
    pub(crate) fn whole_surface(kind: PublishedSurfaceKind) -> Self {
        Self {
            surface: kind,
            root: ProjectionNode::whole_surface_expanded(),
        }
    }

    /// Borrow a [`ProjectionCursor`] at the root.
    pub(crate) fn cursor(&self) -> ProjectionCursor<'_> {
        ProjectionCursor {
            node: &self.root,
            surface: &self.surface,
            remaining: &[],
        }
    }
}

/// Call-scoped, non-owning view into a [`SurfaceProjection`] node.
///
/// Threaded through resolver helpers (`produce_one_macro_object_shape`,
/// `collect_component_meta_registry_refs`, and their callees) so
/// each helper can decide whether to descend into a child or stop
/// walking. `descend` returns `None` when the segment is NOT in
/// the cursor's allowed children → the caller publishes the bare
/// `Ref` and skips deeper walking.
#[derive(Clone, Copy)]
pub(crate) struct ProjectionCursor<'a> {
    pub(crate) node: &'a ProjectionNode,
    pub(crate) surface: &'a PublishedSurfaceKind,
    /// Remaining path segments (when the cursor was constructed by
    /// `with_remaining_path`). Empty when the cursor is at a node
    /// boundary.
    pub(crate) remaining: &'a [PathSegment],
}

impl<'a> ProjectionCursor<'a> {
    /// Descend into a child by segment. Returns `None` when the
    /// segment is NOT in the cursor's allowed children. When
    /// `key_filter` is `All` and `children` is empty, descent
    /// continues with the same node (every key admitted) — this is
    /// the "whole surface" backward-compat mode used by all
    /// pre-Block-6.i callers.
    pub(crate) fn descend(&self, segment: &PathSegment) -> Option<ProjectionCursor<'a>> {
        if let Some(child) = self.node.children.get(segment) {
            return Some(ProjectionCursor {
                node: child,
                surface: self.surface,
                remaining: &[],
            });
        }
        // Default whole-surface mode: no explicit children but
        // KeyFilter::All admits all descents. Self-pin so the
        // descending caller can keep walking.
        match &self.node.key_filter {
            KeyFilter::All => Some(*self),
            KeyFilter::Include(set) => match segment {
                PathSegment::Member(name) => {
                    if set.iter().any(|k| k.as_ref() == name.as_ref()) {
                        Some(*self)
                    } else {
                        None
                    }
                }
                _ => Some(*self),
            },
            KeyFilter::Exclude(set) => match segment {
                PathSegment::Member(name) => {
                    if set.iter().all(|k| k.as_ref() != name.as_ref()) {
                        Some(*self)
                    } else {
                        None
                    }
                }
                _ => Some(*self),
            },
            KeyFilter::UnknownDeferred => Some(*self),
        }
    }

    /// `true` when the cursor is at its terminal hop (no further
    /// path to walk; the caller dispatches at `terminal_mode`).
    pub(crate) fn is_terminal(&self) -> bool {
        self.remaining.is_empty() && self.node.children.is_empty()
    }

    /// Terminal-hop mode for the cursor (`None` if intermediate).
    pub(crate) fn terminal_mode(&self) -> Option<ProjectionMode> {
        self.node.terminal_mode
    }

    /// Whether the cursor admits the given key at THIS hop. Used by
    /// the registry walker's Object arm to gate per-member descent.
    pub(crate) fn admits_key(&self, key: &str) -> bool {
        self.node.key_filter.admits(key)
    }

    /// `true` when this cursor admits ALL keys (no narrowing). The
    /// pre-Block-6.i whole-surface mode.
    pub(crate) fn is_whole_surface(&self) -> bool {
        matches!(self.node.key_filter, KeyFilter::All) && self.node.children.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn cursor_admits_all_when_whole_surface() {
        let proj = SurfaceProjection::whole_surface(PublishedSurfaceKind::Props);
        let cursor = proj.cursor();
        assert!(cursor.admits_key("anything"));
        assert!(cursor.is_whole_surface());
        // Descending any segment self-pins (no narrowing).
        let descended = cursor.descend(&PathSegment::Member(Arc::from("foo")));
        assert!(descended.is_some(), "whole-surface descend must self-pin");
        assert!(descended.unwrap().is_whole_surface());
    }

    #[test]
    fn cursor_include_filter_rejects_unlisted() {
        let mut node = ProjectionNode::whole_surface_expanded();
        node.key_filter = KeyFilter::Include(Arc::from([Arc::from("a"), Arc::from("b")]));
        let proj = SurfaceProjection {
            surface: PublishedSurfaceKind::Props,
            root: node,
        };
        let cursor = proj.cursor();
        assert!(cursor.admits_key("a"));
        assert!(cursor.admits_key("b"));
        assert!(!cursor.admits_key("c"));
        // Descending a non-included Member returns None.
        assert!(cursor
            .descend(&PathSegment::Member(Arc::from("c")))
            .is_none());
        assert!(cursor
            .descend(&PathSegment::Member(Arc::from("a")))
            .is_some());
    }

    #[test]
    fn cursor_exclude_filter_rejects_listed() {
        let mut node = ProjectionNode::whole_surface_expanded();
        node.key_filter = KeyFilter::Exclude(Arc::from([Arc::from("hidden")]));
        let proj = SurfaceProjection {
            surface: PublishedSurfaceKind::Props,
            root: node,
        };
        let cursor = proj.cursor();
        assert!(cursor.admits_key("visible"));
        assert!(!cursor.admits_key("hidden"));
        assert!(cursor
            .descend(&PathSegment::Member(Arc::from("hidden")))
            .is_none());
        assert!(cursor
            .descend(&PathSegment::Member(Arc::from("visible")))
            .is_some());
    }

    #[test]
    fn cursor_terminal_mode_threads_through() {
        let mut root = ProjectionNode::whole_surface_shallow();
        root.terminal_mode = Some(ProjectionMode::Shallow);
        let proj = SurfaceProjection {
            surface: PublishedSurfaceKind::Props,
            root,
        };
        let cursor = proj.cursor();
        assert_eq!(cursor.terminal_mode(), Some(ProjectionMode::Shallow));
        assert!(cursor.is_terminal());
    }

    #[test]
    fn cursor_descend_into_explicit_child() {
        // Build a projection with explicit children: { foo → { bar → terminal } }
        let mut bar_node = ProjectionNode::whole_surface_expanded();
        bar_node.terminal_mode = Some(ProjectionMode::Expanded);
        let mut foo_node = ProjectionNode::whole_surface_expanded();
        foo_node
            .children
            .insert(PathSegment::Member(Arc::from("bar")), bar_node);
        let mut root = ProjectionNode::whole_surface_expanded();
        root.children
            .insert(PathSegment::Member(Arc::from("foo")), foo_node);
        let proj = SurfaceProjection {
            surface: PublishedSurfaceKind::Props,
            root,
        };
        let cursor = proj.cursor();
        let foo_cursor = cursor
            .descend(&PathSegment::Member(Arc::from("foo")))
            .expect("explicit foo child must descend");
        let bar_cursor = foo_cursor
            .descend(&PathSegment::Member(Arc::from("bar")))
            .expect("explicit bar child must descend");
        assert!(bar_cursor.is_terminal());
        assert_eq!(bar_cursor.terminal_mode(), Some(ProjectionMode::Expanded));
    }
}
