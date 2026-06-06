//! Path-precise projection demand substrate.
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
//!   architectural invariant is that production
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
//! ## Scope of use
//!
//! The substrate threads `ProjectionCursor` through
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
///   triggers a `debug_assert!` panic in both
///   [`KeyFilter::admits`] and [`ProjectionCursor::descend`] (Block
///   6.i F6 — the impls panic in debug builds and conservative-
///   reject in release builds so a stale filter cannot silently
///   admit every key).
///
/// `UnknownDeferred` is reserved for a deferred-projection
/// resolution pattern in a follow-up commit; production publication
/// code must NEVER reach this variant at a hop it intends to walk.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) enum KeyFilter {
    All,
    Include(Arc<[Arc<str>]>),
    Exclude(Arc<[Arc<str>]>),
    UnknownDeferred,
}

impl KeyFilter {
    /// Whether a candidate key passes this filter.
    ///
    /// `UnknownDeferred` at the publication boundary
    /// is a Rule-5 violation site. The threaded helpers MUST resolve
    /// the filter to `All`/`Include`/`Exclude` before walking. This
    /// method panics via `debug_assert!` when `UnknownDeferred` is
    /// reached (the doc-comment on the variant promises this); in
    /// production builds the variant is conservative-rejected
    /// (returns `false`) so a stale filter does NOT silently admit
    /// every key — the prior behaviour traded a debug panic for
    /// over-admission, which is the worse default.
    pub(crate) fn admits(&self, key: &str) -> bool {
        match self {
            KeyFilter::All => true,
            KeyFilter::Include(set) => set.iter().any(|k| k.as_ref() == key),
            KeyFilter::Exclude(set) => set.iter().all(|k| k.as_ref() != key),
            KeyFilter::UnknownDeferred => {
                debug_assert!(
                    false,
                    "KeyFilter::UnknownDeferred reached at publication \
                     boundary in KeyFilter::admits — Rule-5 violation \
                     site. The caller MUST resolve the filter to \
                     All/Include/Exclude BEFORE walking. See \
                     projection_demand.rs doc-comment for the STOP \
                     contract."
                );
                false
            }
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
    /// "Whole surface in `Expanded` mode" — the pre-path-precision
    /// default. Used by call sites that have not yet adopted
    /// path-precision.
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

/// Lazy-initialised whole-surface sentinel node returned by
/// [`ProjectionCursor::descend`] when an Include/Exclude key passes
/// the filter but has no explicit child entry in the projection trie.
///
/// The narrowing at the parent hop has ALREADY done its work; the
/// descended cursor must NOT keep applying the parent filter at the
/// child hop (that would block legitimate sibling keys at the child
/// level). Returning a borrow into this static `KeyFilter::All` /
/// no-children node gives the descending caller a fresh whole-surface
/// cursor at the child level — equivalent to "the filter applied at
/// this hop only; deeper structure is unrestricted".
fn whole_surface_descend_node() -> &'static ProjectionNode {
    static NODE: std::sync::OnceLock<ProjectionNode> = std::sync::OnceLock::new();
    NODE.get_or_init(ProjectionNode::whole_surface_expanded)
}

impl<'a> ProjectionCursor<'a> {
    /// Descend into a child by segment. Returns `None` when the
    /// segment is NOT in the cursor's allowed children.
    ///
    /// Narrowing-aware contract:
    ///
    /// 1. If `children[segment]` is explicit → descend into that
    ///    refined child node (true trie navigation; supports deep
    ///    path-precision such as `Foo['a']['b']`).
    /// 2. Else if the parent cursor is GENUINELY whole-surface
    ///    (empty `children` + `KeyFilter::All`) → self-pin (the
    ///    pre-path-precision backward-compat mode for callers that
    ///    have not yet adopted narrowed projections).
    /// 3. Else if the parent's key filter ADMITS the segment but
    ///    has no explicit child entry → descend into a fresh
    ///    whole-surface child cursor (the parent's narrowing
    ///    applied at THIS hop only; deeper structure is
    ///    unrestricted).
    /// 4. Else (filter rejects the segment, or explicit children
    ///    exist but this one isn't enumerated) → return `None`.
    pub(crate) fn descend(&self, segment: &PathSegment) -> Option<ProjectionCursor<'a>> {
        // (1) Explicit child wins.
        if let Some(child) = self.node.children.get(segment) {
            return Some(ProjectionCursor {
                node: child,
                surface: self.surface,
                remaining: &[],
            });
        }

        // F2: When `children` is non-empty AND `key_filter` is `All`,
        // the projection explicitly enumerated children — an
        // un-enumerated segment is OUT OF SCOPE. Return `None` so
        // downstream walkers do not traverse unrequested siblings.
        let children_empty = self.node.children.is_empty();
        if !children_empty && matches!(self.node.key_filter, KeyFilter::All) {
            return None;
        }

        // F6: UnknownDeferred at the publication boundary is a
        // Rule-5 violation site. The threaded helpers must resolve
        // the filter BEFORE walking; reaching this arm means the
        // resolution was skipped. Panic in debug builds so the
        // missing pre-resolution surfaces loudly; in production
        // return `None` (conservative-reject) so we don't admit
        // a stale filter's silent member drops.
        if matches!(self.node.key_filter, KeyFilter::UnknownDeferred) {
            debug_assert!(
                false,
                "KeyFilter::UnknownDeferred reached at publication \
                 boundary in ProjectionCursor::descend — Rule-5 \
                 violation site. The caller MUST resolve the filter \
                 to All/Include/Exclude BEFORE walking. See \
                 projection_demand.rs doc-comment for the STOP \
                 contract."
            );
            return None;
        }

        // (2)/(3) Apply the filter at THIS hop, then descend into the
        // whole-surface child sentinel for Include/Exclude-admitted
        // keys (F3: deeper structure is unrestricted; parent filter
        // does not re-apply at the child level).
        let admits = match (&self.node.key_filter, segment) {
            (KeyFilter::All, _) => true,
            (KeyFilter::Include(set), PathSegment::Member(name)) => {
                set.iter().any(|k| k.as_ref() == name.as_ref())
            }
            (KeyFilter::Exclude(set), PathSegment::Member(name)) => {
                set.iter().all(|k| k.as_ref() != name.as_ref())
            }
            // Non-Member segments (Index): filter applies only to
            // member names; pass through so IndexedAccess descent
            // works.
            (KeyFilter::Include(_) | KeyFilter::Exclude(_), _) => true,
            (KeyFilter::UnknownDeferred, _) => unreachable!(
                "UnknownDeferred handled in earlier arm before \
                 segment-admits computation"
            ),
        };

        if !admits {
            return None;
        }

        // (2) Genuine whole-surface backward-compat mode: empty
        // children + KeyFilter::All ⇒ self-pin so the descending
        // caller keeps the whole-surface filter on subsequent hops.
        if children_empty && matches!(self.node.key_filter, KeyFilter::All) {
            return Some(*self);
        }

        // (3) Include/Exclude-narrowed parent with no explicit
        // child for `segment`: descend into the whole-surface
        // sentinel so deeper structure is unrestricted. The
        // narrowing at the parent hop has already done its work.
        Some(ProjectionCursor {
            node: whole_surface_descend_node(),
            surface: self.surface,
            remaining: &[],
        })
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
    /// pre-path-precision whole-surface mode.
    pub(crate) fn is_whole_surface(&self) -> bool {
        matches!(self.node.key_filter, KeyFilter::All) && self.node.children.is_empty()
    }

    /// Descend into a published macro member by name.
    ///
    /// The per-member publication primitive. A
    /// macro projector publishes EVERY top-level member name
    /// (`whole_surface` selects the member NAMES). For each admitted
    /// member it then calls this method to obtain the cursor to
    /// publish that member's type body AT.
    ///
    /// Returns:
    ///
    /// 1. `None` when the member key is NOT admitted by this hop's
    ///    `KeyFilter` — the caller skips the member entirely (it is
    ///    out of the published surface).
    /// 2. `Some(explicit_child)` when the projection trie carries an
    ///    explicit child node for the member — the consumer walks a
    ///    path INTO this member (`Foo['a']['b']`), so the child
    ///    cursor carries that deep demand.
    /// 3. `Some(terminal_carrier)` when the member is admitted but
    ///    has NO explicit child — the shallow-by-default case. The
    ///    returned cursor is a TERMINAL CARRIER node (NOT `self`):
    ///    its `terminal_mode` is `ProjectionMode::Navigate` so the
    ///    projector publishes the member's type as a carrier
    ///    (`Ref { name, type_arguments }`) without expanding its
    ///    body.
    ///
    /// The carrier-cursor distinction (vs `descend`'s self-pin) is
    /// the Rule-5 fix: `descend` self-pins a whole-surface cursor so
    /// the next hop keeps walking; `descend_published_member` stops
    /// at a `Navigate`-mode terminal so a macro member's type body is
    /// NOT breadth-enumerated.
    pub(crate) fn descend_published_member(&self, key: &str) -> Option<ProjectionCursor<'a>> {
        // (1) Explicit child wins — the consumer walked a deep path
        // into this member; carry that demand verbatim.
        if let Some(child) = self.node.children.get(&PathSegment::Member(Arc::from(key))) {
            return Some(ProjectionCursor {
                node: child,
                surface: self.surface,
                remaining: &[],
            });
        }

        // F2/AX: explicit children enumerated + `KeyFilter::All` ⇒ a
        // member with no explicit child is OUT OF SCOPE.
        if !self.node.children.is_empty() && matches!(self.node.key_filter, KeyFilter::All) {
            return None;
        }

        // F6: UnknownDeferred at the publication boundary is a Rule-5
        // violation site — the caller must resolve the filter first.
        if matches!(self.node.key_filter, KeyFilter::UnknownDeferred) {
            debug_assert!(
                false,
                "KeyFilter::UnknownDeferred reached at publication \
                 boundary in ProjectionCursor::descend_published_member \
                 — Rule-5 violation site."
            );
            return None;
        }

        // (1b) Reject keys the filter excludes.
        if !self.node.key_filter.admits(key) {
            return None;
        }

        // (3) Admitted, no explicit child: publish the member's type
        // as a TERMINAL CARRIER. `Navigate` keeps generic refs as
        // refs — the macro publishes the member NAME, not its
        // expanded type body.
        Some(ProjectionCursor {
            node: terminal_carrier_node(),
            surface: self.surface,
            remaining: &[],
        })
    }

    /// The projection mode the caller should materialise a published
    /// macro member's type at.
    ///
    /// When the cursor is a terminal carrier
    /// (the `descend_published_member` no-explicit-child case) this
    /// returns `ProjectionMode::Navigate`: the member's type is
    /// published as a CARRIER (`Ref { name, type_arguments }`)
    /// without expanding the underlying object body. When the cursor
    /// carries an explicit child node with a deep `terminal_mode`
    /// (the consumer walked `Foo['a']['b']` into this member), that
    /// mode is honoured so the explicit path reduces path-precisely.
    ///
    /// `Navigate` (NOT `Shallow`) is the deliberate stop: `Shallow`
    /// over a generic instantiation still synthesises the one-level
    /// object surface, which would re-admit the carrier type's own
    /// members into the published surface. `Navigate` keeps the ref
    /// a carrier.
    pub(crate) fn terminal_publication_mode(&self) -> ProjectionMode {
        self.node.terminal_mode.unwrap_or(ProjectionMode::Navigate)
    }
}

/// Lazy-initialised TERMINAL CARRIER node returned by
/// [`ProjectionCursor::descend_published_member`] for an admitted
/// macro member that has no explicit child in the projection trie.
///
/// `terminal_mode = Some(Navigate)`: the macro
/// publishes the member's type AS A CARRIER (`Ref { name,
/// type_arguments }`) rather than expanding its body. This is the
/// shallow-by-default publication boundary — the leak fix. Distinct
/// from [`whole_surface_descend_node`] (which is `Expanded`) so a
/// per-member publication does NOT breadth-enumerate the member
/// type's own surface.
fn terminal_carrier_node() -> &'static ProjectionNode {
    static NODE: std::sync::OnceLock<ProjectionNode> = std::sync::OnceLock::new();
    NODE.get_or_init(|| ProjectionNode {
        terminal_mode: Some(ProjectionMode::Navigate),
        children: FxHashMap::default(),
        key_filter: KeyFilter::All,
    })
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

    // -----------------------------------------------------------------
    // Cursor with explicit children + KeyFilter::All
    // rejects unspecified segments. Whole-surface self-pin applies ONLY
    // when `children` is empty.
    // -----------------------------------------------------------------
    #[test]
    fn cursor_explicit_children_reject_unspecified_segments_f2() {
        // Build { foo → terminal }. Root has explicit children +
        // KeyFilter::All. Descending into a non-enumerated key MUST
        // return None — the projection explicitly enumerated children
        // so unspecified ones are out of scope.
        let foo_node = ProjectionNode::whole_surface_expanded();
        let mut root = ProjectionNode::whole_surface_expanded();
        root.children
            .insert(PathSegment::Member(Arc::from("foo")), foo_node);
        let proj = SurfaceProjection {
            surface: PublishedSurfaceKind::Props,
            root,
        };
        let cursor = proj.cursor();

        // The enumerated child still descends.
        assert!(cursor
            .descend(&PathSegment::Member(Arc::from("foo")))
            .is_some());
        // An un-enumerated sibling MUST return None — this is the
        // F2 contract.
        assert!(
            cursor
                .descend(&PathSegment::Member(Arc::from("bar")))
                .is_none(),
            "F2: explicit children + KeyFilter::All must reject \
             unspecified siblings (was: self-pinning on Some(*self))"
        );

        // Whole-surface (empty children) still self-pins for backward
        // compat.
        let whole = SurfaceProjection::whole_surface(PublishedSurfaceKind::Props);
        let whole_cursor = whole.cursor();
        assert!(whole_cursor
            .descend(&PathSegment::Member(Arc::from("anything")))
            .is_some());
    }

    // -----------------------------------------------------------------
    // descend() is a TRUE trie cursor; descending an
    // Include/Exclude-admitted key yields a fresh whole-surface child
    // cursor (deeper structure is unrestricted; parent filter does not
    // re-apply).
    // -----------------------------------------------------------------
    #[test]
    fn cursor_descend_walks_deep_path_explicit_children_f3() {
        // Shape({a: Shape({b: Shallow})}) — descend `a` then `b` must
        // return the Shallow terminal. Descending `a` then `c` must
        // return None (c is not enumerated under a).
        let mut b_node = ProjectionNode::whole_surface_shallow();
        b_node.terminal_mode = Some(ProjectionMode::Shallow);
        let mut a_node = ProjectionNode::whole_surface_expanded();
        a_node
            .children
            .insert(PathSegment::Member(Arc::from("b")), b_node);
        let mut root = ProjectionNode::whole_surface_expanded();
        root.children
            .insert(PathSegment::Member(Arc::from("a")), a_node);
        let proj = SurfaceProjection {
            surface: PublishedSurfaceKind::Props,
            root,
        };
        let cursor = proj.cursor();

        // a → b returns the Shallow terminal.
        let a_cursor = cursor
            .descend(&PathSegment::Member(Arc::from("a")))
            .expect("explicit a child descends");
        let b_cursor = a_cursor
            .descend(&PathSegment::Member(Arc::from("b")))
            .expect("explicit b child under a descends");
        assert!(b_cursor.is_terminal());
        assert_eq!(b_cursor.terminal_mode(), Some(ProjectionMode::Shallow));

        // a → c is not enumerated under a; descend MUST return None
        // (F3: deeper structure is path-precise too).
        assert!(
            a_cursor
                .descend(&PathSegment::Member(Arc::from("c")))
                .is_none(),
            "F3: deep path-precision must reject unspecified child \
             of an enumerated parent"
        );
    }

    #[test]
    fn cursor_include_filter_descend_unrestricts_child_f3() {
        // Include('a') filter at root, no explicit children. Descending
        // 'a' MUST return a cursor whose filter is `All` at the child
        // level — the parent narrowing applied at THIS hop only.
        let mut root = ProjectionNode::whole_surface_expanded();
        root.key_filter = KeyFilter::Include(Arc::from([Arc::from("a")]));
        let proj = SurfaceProjection {
            surface: PublishedSurfaceKind::Props,
            root,
        };
        let cursor = proj.cursor();

        // 'a' admits at this hop.
        let a_cursor = cursor
            .descend(&PathSegment::Member(Arc::from("a")))
            .expect("Include-admitted key must descend");

        // F3: the child cursor's filter is `All` (the parent narrowing
        // does not re-apply at child level). The child admits any
        // sibling at the next level.
        assert!(
            a_cursor.admits_key("anything"),
            "F3: descended cursor under Include('a') must be \
             whole-surface at the next level (parent filter \
             applied at one hop only)"
        );

        // 'b' is rejected at the parent — descend returns None.
        assert!(cursor
            .descend(&PathSegment::Member(Arc::from("b")))
            .is_none());
    }

    // -----------------------------------------------------------------
    // `KeyFilter::UnknownDeferred` admission contract.
    // The variant's doc specifies a `debug_assert!` panic on the
    // publication boundary: the impl panics in debug builds and returns
    // `false` (refuse admission) in release, rather than conservatively
    // admitting with `true`.
    // -----------------------------------------------------------------
    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "Rule-5 violation site")]
    fn key_filter_admits_panics_on_unknown_deferred() {
        let filter = KeyFilter::UnknownDeferred;
        let _ = filter.admits("anything");
    }

    // -----------------------------------------------------------------
    // `descend_published_member` returns a
    // TERMINAL CARRIER cursor (Navigate mode) for an admitted member
    // with no explicit child. This is the Rule-5 publication-boundary
    // stop: a macro member's type body is NOT breadth-enumerated.
    // -----------------------------------------------------------------
    #[test]
    fn descend_published_member_yields_navigate_carrier_for_whole_surface() {
        let proj = SurfaceProjection::whole_surface(PublishedSurfaceKind::Props);
        let cursor = proj.cursor();
        // The root is a whole-surface Expanded node.
        assert_eq!(cursor.terminal_publication_mode(), ProjectionMode::Expanded);
        // Descending a published member yields a TERMINAL CARRIER
        // cursor whose publication mode is Navigate — NOT Expanded.
        let member = cursor
            .descend_published_member("searchTool")
            .expect("whole-surface admits every member");
        assert_eq!(
            member.terminal_publication_mode(),
            ProjectionMode::Navigate,
            "AX: a published macro member with no explicit child must \
             publish at Navigate (carrier), not Expanded"
        );
        assert!(member.is_terminal());
    }

    #[test]
    fn descend_published_member_honors_explicit_child_mode() {
        // Build { searchTool → { Expanded terminal } } — the consumer
        // explicitly walked into searchTool.
        let mut child = ProjectionNode::whole_surface_expanded();
        child.terminal_mode = Some(ProjectionMode::Expanded);
        let mut root = ProjectionNode::whole_surface_expanded();
        root.children
            .insert(PathSegment::Member(Arc::from("searchTool")), child);
        let proj = SurfaceProjection {
            surface: PublishedSurfaceKind::Props,
            root,
        };
        let cursor = proj.cursor();
        // The explicit child carries deep demand → Expanded honoured.
        let member = cursor
            .descend_published_member("searchTool")
            .expect("explicit child descends");
        assert_eq!(member.terminal_publication_mode(), ProjectionMode::Expanded);
        // A sibling with no explicit child is OUT OF SCOPE under an
        // explicit-children + KeyFilter::All root.
        assert!(
            cursor.descend_published_member("other").is_none(),
            "AX: explicit children + KeyFilter::All must reject \
             un-enumerated members"
        );
    }

    #[test]
    fn descend_published_member_rejects_excluded_keys() {
        let mut node = ProjectionNode::whole_surface_expanded();
        node.key_filter = KeyFilter::Exclude(Arc::from([Arc::from("hidden")]));
        let proj = SurfaceProjection {
            surface: PublishedSurfaceKind::Props,
            root: node,
        };
        let cursor = proj.cursor();
        assert!(cursor.descend_published_member("hidden").is_none());
        assert!(cursor.descend_published_member("visible").is_some());
    }

    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "Rule-5 violation site")]
    fn cursor_descend_panics_on_unknown_deferred_f6() {
        let mut node = ProjectionNode::whole_surface_expanded();
        node.key_filter = KeyFilter::UnknownDeferred;
        let proj = SurfaceProjection {
            surface: PublishedSurfaceKind::Props,
            root: node,
        };
        let cursor = proj.cursor();
        let _ = cursor.descend(&PathSegment::Member(Arc::from("any")));
    }
}
