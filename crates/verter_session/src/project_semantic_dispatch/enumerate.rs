//! `key_names_from_base_node` / `key_names_from_keyspace_node` — TS keyof
//! enumeration helpers.
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
    /// **Intersection accumulation change.**
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
                    // Keyof (A | B) = common keys across ALL arms (intersection
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
                    // Key-name enumeration consumes the body's structural
                    // shape (Object members, Union arms, etc.) — Expanded
                    // is required so the next Expand frame can read keys
                    // off the unwrapped surface, not a lazy Ref shell.
                    // Block 6.i Commit AX (codex-hybrid): key
                    // enumeration is a legitimate publication-grade
                    // demand (the keyspace is the explicit consumer
                    // surface), so the context stays `Published +
                    // Expanded`.
                    context: crate::semantic_query::ProjectionReductionContext::published(
                        crate::semantic_query::ProjectionMode::Expanded,
                    ),
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

    /// Block 6.i Round 10 Commit 3 (Chain X closure, codex Q1-X) —
    /// **non-emitting** key-domain membership predicate for path
    /// admission.
    ///
    /// Returns `Some(true)` iff `needle` is structurally proven to be a
    /// member of the keyspace `node`. Returns `Some(false)` iff `needle`
    /// is structurally proven NOT to be a member. Returns `None` when
    /// membership cannot be proven without falling back to whole-keyspace
    /// enumeration — callers MUST then either fall through to the
    /// primitive-keyspace tier or accept the unresolved carrier (NOT
    /// enumerate).
    ///
    /// ## Why
    ///
    /// The pre-Round-10 path-admission tier at
    /// `walk.rs:955-959` called [`Self::key_names_from_keyspace_node`]
    /// to test "does this Mapped's key space contain `needle`?". That
    /// helper internally calls
    /// [`Self::evaluate_deferred_semantic_node`] on the keyspace node
    /// AND [`Self::key_names_from_base_node`] on a `KeyOf { base }` arm
    /// — both of which trigger `build_key_of` / `build_mapped_type`
    /// per-key emissions through `intern_keyspace_names`
    /// (`build.rs:1614`) and the publication loop (`build.rs:1876`).
    ///
    /// The diagnostic at `D:/tmp/round10-diagnostic-report.md` Chain X
    /// (31.3% / 114 of 364 captured ProjectMember emissions on the
    /// nuxt-ui corpus) confirms the leak source: the PathWalker's
    /// Mapped admission emits the **entire** keyspace just to test
    /// membership of ONE literal segment.
    ///
    /// This predicate replaces the enumeration-based admission with a
    /// structural walk that NEVER calls `evaluate_deferred_semantic_node`
    /// on a deferred shell, NEVER calls `key_names_from_base_node`, and
    /// NEVER routes through `Instantiate` / cold-build helpers. Every
    /// case it can decide is decided by walking
    /// already-resolved [`SemanticNodeData`] — `Object` → check members
    /// directly, `Literal` → compare, `Union` → recurse arms, `Never` →
    /// refute, `KeyOf { base }` → recurse into the base's structural
    /// surface ONLY when the base is itself an already-resolved Object
    /// / Intersection / Literal / Union / Never (NOT a `DeclRef`,
    /// `InstantiationRef`, `Opaque`, or other shape that would force a
    /// resolve).
    ///
    /// ## Soundness contract
    ///
    /// - `Some(true)` is a **structural proof of admission**. The
    ///   keyspace genuinely contains `needle`.
    /// - `Some(false)` is a **structural proof of refutation**. The
    ///   keyspace genuinely does NOT contain `needle`.
    /// - `None` is **inconclusive without emission**. The caller MUST
    ///   NOT treat this as refutation; it must fall through to the
    ///   primitive-keyspace tier or accept the carrier as unresolved.
    pub(super) fn keyspace_admits_literal_non_emitting(
        &self,
        node: SemanticNodeId,
        needle: &str,
    ) -> Option<bool> {
        let data = self.graph().node_data(node)?;
        match data.as_ref() {
            // A single literal: admits iff it matches.
            SemanticNodeData::Literal(LiteralValue::String(name)) => Some(name.as_str() == needle),
            SemanticNodeData::Literal(LiteralValue::Number(n)) => {
                let s = if n.fract() == 0.0 && *n >= i64::MIN as f64 && *n <= i64::MAX as f64 {
                    (*n as i64).to_string()
                } else {
                    n.to_string()
                };
                Some(s.as_str() == needle)
            }
            // Never admits nothing.
            SemanticNodeData::Primitive(PrimitiveKind::Never) => Some(false),
            // Union arms: admit iff ANY arm admits; refute iff ALL arms
            // refute. A single `None` arm makes the whole union
            // inconclusive.
            SemanticNodeData::Union(arms) => {
                let arms = Arc::clone(arms);
                drop(data);
                let mut any_admits = false;
                let mut all_refuted = true;
                for arm in arms.iter() {
                    match self.keyspace_admits_literal_non_emitting(*arm, needle) {
                        Some(true) => {
                            any_admits = true;
                            break;
                        }
                        Some(false) => {
                            // this arm refutes; keep checking
                        }
                        None => {
                            all_refuted = false;
                        }
                    }
                }
                if any_admits {
                    Some(true)
                } else if all_refuted {
                    Some(false)
                } else {
                    None
                }
            }
            // `keyof T`: structurally check T's surface ONLY if it is
            // already an enumerable shape. NEVER call
            // `evaluate_deferred_semantic_node` here — that is the
            // emission rail this predicate exists to avoid. If `base`
            // is a `DeclRef` / `InstantiationRef` / `Opaque` / any
            // deferred shape, return `None` (caller falls through).
            SemanticNodeData::KeyOf { base } => {
                let base_id = *base;
                drop(data);
                self.base_member_admission_non_emitting(base_id, needle)
            }
            // Anything else (Object, DeclRef, InstantiationRef,
            // primitives other than Never, Mapped, Conditional, …) is
            // not a keyspace shape this predicate decides — return
            // `None`. The caller falls through to the primitive-keyspace
            // tier or accepts the carrier as unresolved.
            _ => None,
        }
    }

    /// Companion to [`Self::keyspace_admits_literal_non_emitting`] for
    /// the `KeyOf { base }` arm: is `needle` a known structural member
    /// of `base`'s already-resolved surface, without enumerating the
    /// full member set?
    ///
    /// `Object` and `Intersection`-of-Objects are decidable cheaply
    /// here. `DeclRef`, `InstantiationRef`, `Opaque`, and other shapes
    /// that would force a resolve return `None` so the caller falls
    /// through. The intent is to admit the common simple cases (a
    /// `keyof T` whose `T` is a literal Object surface that the walker
    /// already lowered) without paying the emission cost of a
    /// keyspace-wide enumeration.
    fn base_member_admission_non_emitting(
        &self,
        base: SemanticNodeId,
        needle: &str,
    ) -> Option<bool> {
        let data = self.graph().node_data(base)?;
        match data.as_ref() {
            SemanticNodeData::Object(view) => {
                Some(view.members.iter().any(|m| m.name.as_ref() == needle))
            }
            SemanticNodeData::Intersection(arms) => {
                let arms = Arc::clone(arms);
                drop(data);
                let mut any_admits = false;
                let mut any_inconclusive = false;
                for arm in arms.iter() {
                    match self.base_member_admission_non_emitting(*arm, needle) {
                        Some(true) => {
                            any_admits = true;
                            break;
                        }
                        Some(false) => {
                            // this arm doesn't have the member; keep checking
                        }
                        None => {
                            any_inconclusive = true;
                        }
                    }
                }
                if any_admits {
                    Some(true)
                } else if any_inconclusive {
                    None
                } else {
                    Some(false)
                }
            }
            SemanticNodeData::Union(arms) => {
                // `keyof (A | B)` is the INTERSECTION of A's and B's
                // keysets. The needle admits iff EVERY arm admits.
                let arms = Arc::clone(arms);
                drop(data);
                let mut all_admit = true;
                let mut any_inconclusive = false;
                for arm in arms.iter() {
                    match self.base_member_admission_non_emitting(*arm, needle) {
                        Some(true) => {
                            // this arm admits; keep checking
                        }
                        Some(false) => {
                            all_admit = false;
                            break;
                        }
                        None => {
                            any_inconclusive = true;
                        }
                    }
                }
                if !all_admit {
                    Some(false)
                } else if any_inconclusive {
                    None
                } else {
                    Some(true)
                }
            }
            SemanticNodeData::Primitive(PrimitiveKind::Never) => {
                // `keyof never` has every-key-vacuously membership;
                // soundly returning None here lets the caller decide
                // (the existing primitive tier handles the never case
                // by returning false-or-fallthrough).
                None
            }
            // Anything else (DeclRef, InstantiationRef, Opaque, Mapped,
            // Conditional, IndexedAccess, primitives other than Never,
            // …) is not decidable without a resolve. Return None so the
            // caller falls through.
            _ => None,
        }
    }

    /// Path-precise admission for Mapped narrowing when the mapper's
    /// `key_space` is a *non-enumerable* primitive (e.g. `string`,
    /// `number`) — the case `key_names_from_keyspace_node` returns
    /// `None` for.
    ///
    /// `Record<string, V>['foo']` and `{ [K in string]: V }['foo']`
    /// have `key_space = Primitive(String)`: the key domain is the
    /// entire `string` universe, but the enumerator cannot list its
    /// inhabitants. Narrowing here is sound because *every* string
    /// literal is admitted by the `string` key domain — substituting
    /// `K = "foo"` into the value expression evaluates to the same
    /// type the coarse Mapped path would assign to that key.
    ///
    /// Returns `true` when the (key_space, segment-domain) pair admits
    /// the literal:
    ///   - `Primitive(String)` admits a string-domain segment
    ///     (`Member`, `Index::String`, or a `TypeNode` normalised to a
    ///     string literal).
    ///   - `Primitive(Number)` admits a number-domain segment
    ///     (`Index::Number`, or a `TypeNode` normalised to a number
    ///     literal).
    ///   - `Primitive(Any)` / `Primitive(Unknown)` admit any literal —
    ///     fully permissive key domain.
    ///
    /// Returns `false` for any other key_space shape (the caller falls
    /// back to the coarse whole-surface Mapped path).
    ///
    /// `segment_is_string_domain` is `true` when the segment originated
    /// from a `Member(name)` / `Index::String` / `Index::TypeNode`
    /// normalised to a string literal; `false` when it originated from
    /// `Index::Number` / a `TypeNode` normalised to a number literal.
    /// (Pure ambiguity — the `TypeNode` still unresolved — is filtered
    /// upstream: `literal_name` would be `None` and the caller never
    /// invokes this helper.)
    pub(super) fn primitive_keyspace_admits_segment(
        &self,
        key_space: SemanticNodeId,
        segment_is_string_domain: bool,
    ) -> bool {
        let resolved = self.evaluate_deferred_semantic_node(key_space);
        let Some(data) = self.graph().node_data(resolved) else {
            return false;
        };
        match data.as_ref() {
            SemanticNodeData::Primitive(PrimitiveKind::String) => segment_is_string_domain,
            SemanticNodeData::Primitive(PrimitiveKind::Number) => !segment_is_string_domain,
            SemanticNodeData::Primitive(PrimitiveKind::Any | PrimitiveKind::Unknown) => true,
            // Union of primitives (e.g. `string | number`) admits if any
            // arm admits the segment domain. Reuses the same recursive
            // check, mirroring `key_names_from_keyspace_node`'s union
            // handling but for primitive admission.
            SemanticNodeData::Union(members) => members.iter().any(|member| {
                self.primitive_keyspace_admits_segment(*member, segment_is_string_domain)
            }),
            _ => false,
        }
    }
}
