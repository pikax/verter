//! Template lexical scopes: persistent frames plus per-node scope handles.
//!
//! A template introduces names in exactly two places — a `v-for` alias list and
//! a `v-slot` parameter list. Each such list becomes ONE frame holding only the
//! names that element declares, linked to the frame it is nested in. Every node
//! then carries a [`LexicalScopeId`] handle to the innermost frame visible to
//! it, so the effective scope of any expression is "this frame and its parents"
//! without copying an inherited name into a nested scope.
//!
//! The forward pass that parses template expressions resolves names through an
//! [`ActiveScope`]: one multiset of the names visible at the node being parsed,
//! updated incrementally as the pass moves between frames (it leaves the frames
//! the previous node was in and enters the frames the next node is in, touching
//! only the frames that differ). Lookups are O(1) in the depth of nesting.
//!
//! Receiving guidance for consumers: query names through a handle
//! ([`LexicalScopes::declares`], [`LexicalScopes::declares_completion_of`]) or
//! read a node's handle from [`OxcParsedAst`](super::types::OxcParsedAst). Do
//! not flatten a handle's inherited names into a per-element or per-expression
//! list, and do not rescan ancestors to rediscover a node's scope — the handle
//! already names it.

use std::collections::BTreeMap;
use std::ops::Range;

use crate::utils::oxc::bindings::EnclosingScope;

/// Handle to one frame of template lexical scope.
///
/// [`LexicalScopeId::ROOT`] is the empty scope outside every `v-for` / `v-slot`.
/// A handle is only meaningful against the [`LexicalScopes`] that minted it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LexicalScopeId(u32);

impl LexicalScopeId {
    /// The scope outside every `v-for` / `v-slot`: it declares nothing.
    pub const ROOT: Self = Self(0);

    #[inline]
    fn index(self) -> usize {
        self.0 as usize
    }
}

/// One frame: the names a single `v-for` or `v-slot` declares, in source order.
#[derive(Debug)]
struct Frame {
    parent: LexicalScopeId,
    depth: u32,
    names: Range<u32>,
}

/// Persistent template lexical scopes for one template.
///
/// Frames are append-only. A frame stores only its own names, so the total
/// stored names equal the number of names the template declares, however
/// deeply its scopes nest.
#[derive(Debug)]
pub struct LexicalScopes<'alloc> {
    frames: Vec<Frame>,
    names: Vec<&'alloc str>,
}

impl Default for LexicalScopes<'_> {
    fn default() -> Self {
        Self::new()
    }
}

impl<'alloc> LexicalScopes<'alloc> {
    /// Scopes holding only [`LexicalScopeId::ROOT`].
    pub fn new() -> Self {
        Self {
            frames: vec![Frame {
                parent: LexicalScopeId::ROOT,
                depth: 0,
                names: 0..0,
            }],
            names: Vec::new(),
        }
    }

    /// Open a frame nested in `parent` that declares `names`.
    ///
    /// A list that declares nothing opens no frame: `parent` is returned, so a
    /// handle other than `parent` always means "declares something new".
    pub fn push(
        &mut self,
        parent: LexicalScopeId,
        names: impl IntoIterator<Item = &'alloc str>,
    ) -> LexicalScopeId {
        let start = self.names.len() as u32;
        self.names.extend(names);
        let end = self.names.len() as u32;
        if start == end {
            return parent;
        }
        let id = LexicalScopeId(self.frames.len() as u32);
        let depth = self.frames[parent.index()].depth + 1;
        self.frames.push(Frame {
            parent,
            depth,
            names: start..end,
        });
        id
    }

    /// The names `scope`'s own frame declares (not its parents'), in source
    /// order. Empty for [`LexicalScopeId::ROOT`].
    pub fn own_names(&self, scope: LexicalScopeId) -> &[&'alloc str] {
        let range = &self.frames[scope.index()].names;
        &self.names[range.start as usize..range.end as usize]
    }

    /// The frame `scope` is nested in. `ROOT` is its own parent.
    pub fn parent(&self, scope: LexicalScopeId) -> LexicalScopeId {
        self.frames[scope.index()].parent
    }

    /// Whether `scope` or any frame it is nested in declares exactly `name`.
    ///
    /// Walks frames, not nodes: the cost is the number of names visible at
    /// `scope`. For lookups repeated across a whole pass use [`ActiveScope`].
    pub fn declares(&self, scope: LexicalScopeId, name: &str) -> bool {
        self.visible_frames(scope)
            .any(|frame| self.own_names(frame).contains(&name))
    }

    /// Whether a name visible at `scope` starts with `partial` (an identifier
    /// still being typed, in IDE completion mode).
    pub fn declares_completion_of(&self, scope: LexicalScopeId, partial: &str) -> bool {
        self.visible_frames(scope).any(|frame| {
            self.own_names(frame)
                .iter()
                .any(|name| name.starts_with(partial))
        })
    }

    /// Frames visible at `scope`, innermost first, excluding `ROOT`.
    fn visible_frames(&self, scope: LexicalScopeId) -> impl Iterator<Item = LexicalScopeId> + '_ {
        std::iter::successors(
            (scope != LexicalScopeId::ROOT).then_some(scope),
            move |&frame| {
                let parent = self.parent(frame);
                (parent != LexicalScopeId::ROOT).then_some(parent)
            },
        )
    }

    /// Number of frames, including `ROOT`.
    pub fn frame_count(&self) -> usize {
        self.frames.len()
    }

    /// Total names stored across all frames — each declared name once.
    pub fn stored_name_count(&self) -> usize {
        self.names.len()
    }

    #[inline]
    fn depth(&self, scope: LexicalScopeId) -> u32 {
        self.frames[scope.index()].depth
    }
}

/// The names visible at the node the forward pass is parsing.
///
/// A multiset, so a name declared by two nested frames stays visible after the
/// inner one is left. Moving to another scope leaves and enters only the frames
/// between the two scopes and their common ancestor.
#[derive(Debug, Default)]
pub(crate) struct ActiveScope<'alloc> {
    current: Option<LexicalScopeId>,
    visible: BTreeMap<&'alloc str, u32>,
}

impl<'alloc> ActiveScope<'alloc> {
    /// Whether no template-scope name is visible.
    #[inline]
    pub(crate) fn is_empty(&self) -> bool {
        self.visible.is_empty()
    }

    /// Make `target` the active scope.
    pub(crate) fn enter(&mut self, scopes: &LexicalScopes<'alloc>, target: LexicalScopeId) {
        let mut from = self.current.unwrap_or(LexicalScopeId::ROOT);
        self.current = Some(target);
        let mut to = target;
        while from != to {
            if scopes.depth(from) >= scopes.depth(to) {
                for &name in scopes.own_names(from) {
                    if let Some(count) = self.visible.get_mut(name) {
                        *count -= 1;
                        if *count == 0 {
                            self.visible.remove(name);
                        }
                    }
                }
                record_scope_work(scopes.own_names(from).len() + 1);
                from = scopes.parent(from);
            } else {
                for &name in scopes.own_names(to) {
                    *self.visible.entry(name).or_insert(0) += 1;
                }
                record_scope_work(scopes.own_names(to).len() + 1);
                to = scopes.parent(to);
            }
        }
    }
}

impl EnclosingScope for ActiveScope<'_> {
    #[inline]
    fn declares(&self, name: &str) -> bool {
        self.visible.contains_key(name)
    }

    fn declares_completion_of(&self, partial: &str) -> bool {
        // Names starting with `partial` sort contiguously from `partial` itself.
        self.visible
            .range(partial..)
            .next()
            .is_some_and(|(name, _)| name.starts_with(partial))
    }
}

#[cfg(any(test, feature = "semantic-observe"))]
thread_local! {
    /// Frames entered or left plus names added or removed by [`ActiveScope::enter`].
    static SCOPE_WORK: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(any(test, feature = "semantic-observe"))]
#[inline]
fn record_scope_work(units: usize) {
    SCOPE_WORK.with(|work| work.set(work.get() + units));
}

#[cfg(not(any(test, feature = "semantic-observe")))]
#[inline(always)]
fn record_scope_work(_units: usize) {}

/// Read and reset the per-thread scope-switch work counter.
#[cfg(any(test, feature = "semantic-observe"))]
#[cfg_attr(
    not(any(test, feature = "bench")),
    allow(
        dead_code,
        reason = "measurement harnesses reach it only where `template` is public"
    )
)]
pub fn take_scope_work() -> usize {
    SCOPE_WORK.with(|work| work.replace(0))
}
