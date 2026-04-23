//! `key_names_from_base_node` / `key_names_from_keyspace_node` — TS keyof
//! enumeration helpers (plan §3 Change Split).
//!
//! Shared builders walk the base node's [`SemanticNodeData`] shape and
//! return the concrete member names when enumeration succeeds, or `None`
//! when the base is still open (deferred shell). Unresolvable cases
//! surface to the caller which produces a canonical `Mapped` / `KeyOf`
//! deferred shell.

use std::sync::Arc;

use rustc_hash::FxHashSet;

use super::ProjectSemanticDispatch;
use crate::semantic_query::{
    LiteralValue, PrimitiveKind, SemanticNodeData, SemanticNodeId, SemanticQueryApi,
};

/// Worklist frame for the iterative `key_names_from_base_node`
/// driver (Path C C10). `Expand` advances one node; `Combine*`
/// reduce the top N prior results (one per arm) into the compound's
/// key enumeration.
enum KeyNamesFrame {
    Expand(SemanticNodeId),
    CombineIntersection { arm_count: usize },
    CombineUnion { arm_count: usize },
}

impl<'a> ProjectSemanticDispatch<'a> {
    /// Iterative `keyof` enumeration (Path C C10). Replaces the recursive
    /// per-arm descent with a heap-backed worklist so deeply-nested
    /// Intersection / Union arm chains no longer grow the Rust call
    /// stack.
    ///
    /// **Intersection accumulation change (plan §2 Stage 5 Pass C10).**
    /// The Intersection arm's pre-C10 all-or-nothing `?` operator
    /// propagated `None` up whenever any arm was unresolvable, even when
    /// other arms had enumerable keys. Post-C10 the Intersection arm
    /// accumulates the union of keys across every **enumerable** arm
    /// and returns `None` only when every arm is unresolvable —
    /// addresses the pre-§14 Gemini F3 report where `keyof (A & B)` lost
    /// enumerable keys from A when B was unresolvable.
    pub(super) fn key_names_from_base_node(&self, base: SemanticNodeId) -> Option<Vec<Arc<str>>> {
        let mut work: Vec<KeyNamesFrame> = Vec::new();
        let mut results: Vec<Option<Vec<Arc<str>>>> = Vec::new();
        work.push(KeyNamesFrame::Expand(base));

        while let Some(frame) = work.pop() {
            match frame {
                KeyNamesFrame::Expand(id) => {
                    self.key_names_step(id, &mut work, &mut results);
                }
                KeyNamesFrame::CombineIntersection { arm_count } => {
                    // Accumulate enumerable arms; ignore unresolvable ones.
                    // Only return None when EVERY arm is unresolvable.
                    let start = results.len().saturating_sub(arm_count);
                    let arm_results: Vec<_> = results.drain(start..).collect();
                    let mut names: Vec<Arc<str>> = Vec::new();
                    let mut seen: FxHashSet<Arc<str>> = FxHashSet::default();
                    let mut any_enumerable = false;
                    for arm_names in arm_results.into_iter().flatten() {
                        any_enumerable = true;
                        for name in arm_names {
                            if seen.insert(Arc::clone(&name)) {
                                names.push(name);
                            }
                        }
                    }
                    results.push(if any_enumerable { Some(names) } else { None });
                }
                KeyNamesFrame::CombineUnion { arm_count } => {
                    // keyof (A | B) = common keys across ALL arms (intersection
                    // of enumerated sets). Unresolvable arm → whole union None.
                    let start = results.len().saturating_sub(arm_count);
                    let arm_results: Vec<_> = results.drain(start..).collect();
                    let mut common: Option<FxHashSet<Arc<str>>> = None;
                    let mut unresolvable = false;
                    for arm in arm_results {
                        match arm {
                            Some(arm_names) => {
                                let arm_set: FxHashSet<Arc<str>> = arm_names.into_iter().collect();
                                common = Some(match common {
                                    Some(current) => current
                                        .intersection(&arm_set)
                                        .cloned()
                                        .collect::<FxHashSet<_>>(),
                                    None => arm_set,
                                });
                            }
                            None => {
                                unresolvable = true;
                                // Cannot early-break — must drain remaining
                                // results from the stack to keep `results`
                                // aligned for the next combine.
                            }
                        }
                    }
                    let combined = if unresolvable {
                        None
                    } else {
                        let mut names: Vec<Arc<str>> =
                            common.unwrap_or_default().into_iter().collect();
                        names.sort_unstable_by(|left, right| left.as_ref().cmp(right.as_ref()));
                        Some(names)
                    };
                    results.push(combined);
                }
            }
        }

        results.pop().unwrap_or(None)
    }

    /// Expand one node worth of key-name enumeration. Pushes either a
    /// direct result (`Some(names)` / `None`) onto `results`, or child
    /// expansions + a combine frame onto `work`.
    fn key_names_step(
        &self,
        base: SemanticNodeId,
        work: &mut Vec<KeyNamesFrame>,
        results: &mut Vec<Option<Vec<Arc<str>>>>,
    ) {
        let resolved = self.evaluate_deferred_semantic_node(base);
        let data = match self.graph().node_data(resolved) {
            Some(d) => d,
            None => {
                results.push(None);
                return;
            }
        };
        match data.as_ref() {
            SemanticNodeData::Object(surface) => {
                let names = surface
                    .members
                    .iter()
                    .map(|member| Arc::clone(&member.name))
                    .collect();
                results.push(Some(names));
            }
            SemanticNodeData::Intersection(arms) => {
                let arms = Arc::clone(arms);
                drop(data);
                let n = arms.len();
                if n == 0 {
                    results.push(Some(Vec::new()));
                    return;
                }
                work.push(KeyNamesFrame::CombineIntersection { arm_count: n });
                for arm in arms.iter().rev() {
                    work.push(KeyNamesFrame::Expand(*arm));
                }
            }
            SemanticNodeData::Union(arms) => {
                let arms = Arc::clone(arms);
                drop(data);
                let n = arms.len();
                if n == 0 {
                    results.push(Some(Vec::new()));
                    return;
                }
                work.push(KeyNamesFrame::CombineUnion { arm_count: n });
                for arm in arms.iter().rev() {
                    work.push(KeyNamesFrame::Expand(*arm));
                }
            }
            SemanticNodeData::Primitive(PrimitiveKind::Never) => {
                results.push(Some(Vec::new()));
            }
            // C16: DeclPlaceholder — expand via Instantiate before
            // enumerating keys.
            SemanticNodeData::Opaque(crate::semantic_query::QueryError::DeclPlaceholder {
                canonical_id,
                name,
                whole_hash,
            }) => {
                let identity = crate::semantic_query::DeclIdentity {
                    canonical_id: Arc::clone(canonical_id),
                    whole_hash: *whole_hash,
                    decl_name: Arc::clone(name),
                };
                drop(data);
                match self.execute(crate::semantic_query::SemanticQueryKey::Instantiate {
                    base: identity,
                    args: Arc::from(Vec::<SemanticNodeId>::new().into_boxed_slice()),
                }) {
                    crate::semantic_query::QueryResult::Value(instantiated)
                        if instantiated != resolved =>
                    {
                        work.push(KeyNamesFrame::Expand(instantiated));
                    }
                    _ => {
                        results.push(None);
                    }
                }
            }
            // Unresolvable shapes fall through to None — catch-all
            // matches deferred shells, primitives other than Never,
            // Literals, TypeParams, etc.
            _ => {
                results.push(None);
            }
        }
    }

    pub(super) fn key_names_from_keyspace_node(
        &self,
        node: SemanticNodeId,
    ) -> Option<Vec<Arc<str>>> {
        let resolved = self.evaluate_deferred_semantic_node(node);
        let data = self.graph().node_data(resolved)?;
        match data.as_ref() {
            SemanticNodeData::Literal(LiteralValue::String(name)) => {
                Some(vec![Arc::from(name.as_str())])
            }
            SemanticNodeData::Union(members) => {
                let mut names = Vec::new();
                let mut seen = FxHashSet::default();
                for member in members.iter() {
                    for name in self.key_names_from_keyspace_node(*member)? {
                        if seen.insert(Arc::clone(&name)) {
                            names.push(name);
                        }
                    }
                }
                Some(names)
            }
            SemanticNodeData::Primitive(PrimitiveKind::Never) => Some(Vec::new()),
            SemanticNodeData::KeyOf { base } => self.key_names_from_base_node(*base),
            _ => self.key_names_from_base_node(resolved),
        }
    }
}
