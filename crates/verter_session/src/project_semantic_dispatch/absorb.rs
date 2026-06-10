//! §22 type-lattice absorption — the reducers' FIRST fast-reject.
//!
//! Each `absorb_*` helper is a SEPARABLE entry hook a reducer calls before
//! any structural work (`docs/arch/u2-query-value-domain-design.md` §22.2):
//! when an operand is one of the lattice extremes (`any` / `never` /
//! `unknown` / `error`), the absorption rule short-circuits the whole
//! operator to its absorbed result, removing the structural recursion. The
//! helpers are INTENTIONALLY isolated from the reducer bodies — each reducer
//! adds exactly one `if let Some(out) = self.absorb_*(...) { return out; }`
//! line at entry, so the §22 behavior never tangles with the per-arm reducer
//! logic the key-surface spine also touches.
//!
//! ## Discipline (all helpers)
//!
//! - **Cheap `node_data` peek ONLY.** No `execute`, no `evaluate_deferred`,
//!   no resolver work. The peek follows transparent [`Alias`](crate::semantic_query::SemanticNodeData::Alias)
//!   redirects (a pure arena hop) up to [`ALIAS_PEEK_HOPS`] but never reduces
//!   an operator or resolves a declaration.
//! - **`any` / `never` / `unknown` results are `Clean`** (legitimately
//!   cacheable). **`error` rides [`Opaque(QueryError)`](crate::semantic_query::SemanticNodeData::Opaque)**:
//!   an `error` operand DOMINATES every other absorber, so the absorbed
//!   result is the `error` CARRIER itself — its node identity + `QueryError`
//!   payload survive, so relation/display still see the error type instead of
//!   a `Clean` `any`/`never`/`unknown` that erased the error. This is
//!   *carrier-dominating*, NOT taint-propagating: the absorbed
//!   [`QueryBuildOutput`] is built via [`QueryBuildOutput::from`], whose
//!   `taint` defaults to [`Clean`](crate::semantic_query::ResultTaint::Clean) —
//!   absorption does NOT join any operand's §18 taint onto the output. That is
//!   sound today because no producer emits non-`Clean` taint, so every
//!   absorbed type error is deterministic (`unknown[K]`, `keyof error`, …) and
//!   legitimately cacheable. See [`absorbed_output`](Self::absorbed_output) for
//!   the §18.4 follow-up that joins the dominating operand's taint once taint
//!   producers land.
//! - **`Opaque(QueryError::DeclPlaceholder)` is NOT the error type** — it is
//!   an expandable declaration carrier, so it is excluded from the `error`
//!   classification (mirrors the relation engine's identity-carrier unwrap).

use std::sync::Arc;

use crate::project_semantic_dispatch::walk::QueryBuildOutput;
use crate::semantic_query::{
    IndexKey, PathSegment, PrimitiveKind, QueryError, QueryResult, SemanticNodeData, SemanticNodeId,
};

use super::ProjectSemanticDispatch;

/// Maximum transparent [`Alias`](SemanticNodeData::Alias) hops the fast-reject
/// peek follows before giving up. Absorption is an optimisation + a
/// correctness fix for DIRECT lattice-extreme operands; a longer alias chain
/// falls through to the structural reducer, which resolves it via the normal
/// machinery. The bound keeps the peek O(1) and immune to alias cycles.
const ALIAS_PEEK_HOPS: usize = 8;

/// A lattice-extreme operand the §22 absorption table reacts to.
/// `pub(super)` so the shared conditional branch-selection oracle
/// (`build.rs::conditional_branch_selection`) can route `any` / `error`
/// checks — which semantically use BOTH branches / dominate — to
/// `Deferred` instead of letting the relation table select a branch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SpecialKind {
    Any,
    Never,
    Unknown,
    /// The error type (`Opaque(QueryError)`, excluding `DeclPlaceholder`).
    Error,
}

impl ProjectSemanticDispatch<'_> {
    /// Classify `id` as a lattice extreme, following transparent `Alias`
    /// redirects (bounded by [`ALIAS_PEEK_HOPS`]). Returns the kind AND the
    /// resolved node id (so an `error` operand can be reused verbatim as the
    /// absorbed result, preserving its `QueryError` payload + node identity —
    /// the error CARRIER).
    pub(super) fn peek_special(&self, id: SemanticNodeId) -> Option<(SpecialKind, SemanticNodeId)> {
        let mut cur = id;
        // At most ALIAS_PEEK_HOPS hops; a longer chain / alias cycle → None.
        // bounded-loop: ALIAS_PEEK_HOPS transparent Alias redirects.
        for _ in 0..ALIAS_PEEK_HOPS {
            let data = self.graph().node_data(cur)?;
            match &*data {
                SemanticNodeData::Alias(inner) => {
                    let next = *inner;
                    drop(data);
                    cur = next;
                    continue;
                }
                SemanticNodeData::Primitive(PrimitiveKind::Any) => {
                    return Some((SpecialKind::Any, cur))
                }
                SemanticNodeData::Primitive(PrimitiveKind::Never) => {
                    return Some((SpecialKind::Never, cur))
                }
                SemanticNodeData::Primitive(PrimitiveKind::Unknown) => {
                    return Some((SpecialKind::Unknown, cur))
                }
                SemanticNodeData::Opaque(err) if err.is_error_type() => {
                    return Some((SpecialKind::Error, cur))
                }
                _ => return None,
            }
        }
        None
    }

    /// Intern a bare primitive node.
    fn primitive_node(&self, kind: PrimitiveKind) -> SemanticNodeId {
        self.graph().intern_node(SemanticNodeData::Primitive(kind))
    }

    /// `string | number | symbol` — the `keyof any` / `keyof never` keyspace.
    fn string_number_symbol(&self) -> SemanticNodeId {
        let s = self.primitive_node(PrimitiveKind::String);
        let n = self.primitive_node(PrimitiveKind::Number);
        let sym = self.primitive_node(PrimitiveKind::Symbol);
        self.intern_normalized_union_or_intersection(&[s, n, sym], /* is_union */ true)
    }

    /// `{}` — the empty object surface (`keyof unknown = never` mapped, mapped
    /// over `never`).
    fn empty_object(&self) -> SemanticNodeId {
        self.graph()
            .intern_node(SemanticNodeData::Object(super::walk::empty_surface_view()))
    }

    /// Wrap an absorbed result node into a `QueryBuildOutput` rooted on the
    /// operand(s) the absorption read. The absorbed value is a function of
    /// the operands' file content, so the published memo entry must miss when
    /// any operand's file is edited.
    fn absorbed_output(
        &self,
        node: SemanticNodeId,
        roots_from: impl IntoIterator<Item = SemanticNodeId>,
    ) -> QueryBuildOutput {
        let observed = self.observed_self_roots_from_nodes(roots_from);
        // TODO(follow-up, §18.4): once taint producers land and per-operand
        // taint is available, join the dominating error operand's taint into
        // the absorbed output here so an INPUT-DEGRADED error becomes
        // ReturnOnly-prone rather than Clean. Deterministic type errors
        // (unknown[K], keyof error) stay Clean/cacheable.
        QueryBuildOutput::from((
            QueryResult::Value(node),
            self.project_generation_signature(),
        ))
        .with_observed_self_roots(observed)
    }

    // ── Union ───────────────────────────────────────────────────────────
    /// §22.2 union absorption: `X | any = any`, `X | unknown = unknown`,
    /// `X | never = X` (drop every `never` arm; all-`never` ⇒ `never`),
    /// `X | error = error` (carrier-dominating).
    pub(crate) fn absorb_union(&self, members: &[SemanticNodeId]) -> Option<QueryBuildOutput> {
        let mut has_any = false;
        let mut has_unknown = false;
        let mut error_node: Option<SemanticNodeId> = None;
        let mut has_never = false;
        for &m in members {
            match self.peek_special(m) {
                Some((SpecialKind::Any, _)) => has_any = true,
                Some((SpecialKind::Unknown, _)) => has_unknown = true,
                Some((SpecialKind::Error, id)) => {
                    if error_node.is_none() {
                        error_node = Some(id);
                    }
                }
                Some((SpecialKind::Never, _)) => has_never = true,
                None => {}
            }
        }
        // Error dominates so the error CARRIER is never hidden behind a Clean
        // `any`/`unknown` (relation/display keep seeing the error type).
        if let Some(err) = error_node {
            return Some(self.absorbed_output(err, members.iter().copied()));
        }
        if has_any {
            return Some(self.absorbed_output(
                self.primitive_node(PrimitiveKind::Any),
                members.iter().copied(),
            ));
        }
        if has_unknown {
            return Some(self.absorbed_output(
                self.primitive_node(PrimitiveKind::Unknown),
                members.iter().copied(),
            ));
        }
        if has_never {
            // `X | never = X`: drop the `never` arms and re-normalise the
            // remainder. An all-`never` union folds to `never`
            // (`intern_*` interns `Never` for an empty member set).
            let kept: Vec<SemanticNodeId> = members
                .iter()
                .copied()
                .filter(|&m| !matches!(self.peek_special(m), Some((SpecialKind::Never, _))))
                .collect();
            let node =
                self.intern_normalized_union_or_intersection(&kept, /* is_union */ true);
            return Some(self.absorbed_output(node, members.iter().copied()));
        }
        None
    }

    // ── Intersection ──────────────────────────────────────────────────────
    /// §22.2 intersection absorption: `X & never = never`, `X & any = any`,
    /// `X & unknown = X` (drop every `unknown` arm; all-`unknown` ⇒ `unknown`),
    /// `X & error = error` (carrier-dominating).
    pub(crate) fn absorb_intersection(
        &self,
        members: &[SemanticNodeId],
    ) -> Option<QueryBuildOutput> {
        let mut has_any = false;
        let mut has_never = false;
        let mut has_unknown = false;
        let mut error_node: Option<SemanticNodeId> = None;
        for &m in members {
            match self.peek_special(m) {
                Some((SpecialKind::Any, _)) => has_any = true,
                Some((SpecialKind::Never, _)) => has_never = true,
                Some((SpecialKind::Unknown, _)) => has_unknown = true,
                Some((SpecialKind::Error, id)) => {
                    if error_node.is_none() {
                        error_node = Some(id);
                    }
                }
                None => {}
            }
        }
        // Error dominates so the error CARRIER is never hidden.
        if let Some(err) = error_node {
            return Some(self.absorbed_output(err, members.iter().copied()));
        }
        if has_never {
            return Some(self.absorbed_output(
                self.primitive_node(PrimitiveKind::Never),
                members.iter().copied(),
            ));
        }
        if has_any {
            return Some(self.absorbed_output(
                self.primitive_node(PrimitiveKind::Any),
                members.iter().copied(),
            ));
        }
        if has_unknown {
            // `X & unknown = X`: drop the `unknown` arms. An all-`unknown`
            // intersection is `unknown`.
            let kept: Vec<SemanticNodeId> = members
                .iter()
                .copied()
                .filter(|&m| !matches!(self.peek_special(m), Some((SpecialKind::Unknown, _))))
                .collect();
            if kept.is_empty() {
                return Some(self.absorbed_output(
                    self.primitive_node(PrimitiveKind::Unknown),
                    members.iter().copied(),
                ));
            }
            let node =
                self.intern_normalized_union_or_intersection(&kept, /* is_union */ false);
            return Some(self.absorbed_output(node, members.iter().copied()));
        }
        None
    }

    // ── keyof ───────────────────────────────────────────────────────────
    /// §22.2 `keyof` absorption: `keyof any = string | number | symbol`,
    /// `keyof never = string | number | symbol` (TS quirk),
    /// `keyof unknown = never`, `keyof error = error`.
    pub(crate) fn absorb_key_of(&self, base: SemanticNodeId) -> Option<QueryBuildOutput> {
        match self.peek_special(base)? {
            (SpecialKind::Any | SpecialKind::Never, _) => {
                Some(self.absorbed_output(self.string_number_symbol(), [base]))
            }
            (SpecialKind::Unknown, _) => {
                Some(self.absorbed_output(self.primitive_node(PrimitiveKind::Never), [base]))
            }
            (SpecialKind::Error, err) => Some(self.absorbed_output(err, [base])),
        }
    }

    // ── Indexed access ────────────────────────────────────────────────────
    /// §22.2 indexed-access absorption for the `?[K]` shape:
    /// `any[K] = any`, `never[K] = never`,
    /// `unknown[K] = ` UNCONDITIONAL error for ALL `K` (`unknown` has no index
    /// signatures — an illegal index is an `Opaque(QueryError)`, NOT per-K and
    /// NOT a crash), `error[K] = error`.
    pub(crate) fn absorb_indexed_access(&self, object: SemanticNodeId) -> Option<QueryBuildOutput> {
        match self.peek_special(object)? {
            (SpecialKind::Any, _) => {
                Some(self.absorbed_output(self.primitive_node(PrimitiveKind::Any), [object]))
            }
            (SpecialKind::Never, _) => {
                Some(self.absorbed_output(self.primitive_node(PrimitiveKind::Never), [object]))
            }
            (SpecialKind::Unknown, _) => {
                let err = self.opaque(QueryError::Other(Arc::from(
                    "indexed access into `unknown` (no index signatures)",
                )));
                Some(self.absorbed_output(err, [object]))
            }
            (SpecialKind::Error, err) => Some(self.absorbed_output(err, [object])),
        }
    }

    /// Whether a `ProjectPath` is the single-segment indexed-access shape
    /// (`?[K]`). §22 indexed-access absorption applies only to this shape;
    /// member projection (`.foo`) is a distinct surface left to the walker.
    pub(crate) fn project_path_is_indexed_access(path: &[PathSegment]) -> bool {
        matches!(
            path,
            [PathSegment::Index(
                IndexKey::String(_) | IndexKey::Number(_) | IndexKey::TypeNode(_)
            )]
        )
    }

    // ── Mapped ──────────────────────────────────────────────────────────
    /// §22.2 mapped-type absorption on the mapped SOURCE:
    /// over `any` ⇒ `any`; over `never` ⇒ `{}`; over `error` ⇒ `error`; a
    /// DIRECT mapping over `unknown` (`{ [K in unknown] }`, `K` not constrained
    /// to a key set) is illegal ⇒ error. (The COMMON `{ [K in keyof unknown] }`
    /// path arrives here as source = `never` — `keyof unknown` already reduced
    /// to `never` — and folds to `{}`.)
    pub(crate) fn absorb_mapped(&self, source: SemanticNodeId) -> Option<QueryBuildOutput> {
        match self.peek_special(source)? {
            (SpecialKind::Any, _) => {
                Some(self.absorbed_output(self.primitive_node(PrimitiveKind::Any), [source]))
            }
            (SpecialKind::Never, _) => Some(self.absorbed_output(self.empty_object(), [source])),
            (SpecialKind::Unknown, _) => {
                let err = self.opaque(QueryError::Other(Arc::from(
                    "mapped type over `unknown` (not a key set)",
                )));
                Some(self.absorbed_output(err, [source]))
            }
            (SpecialKind::Error, err) => Some(self.absorbed_output(err, [source])),
        }
    }

    // ── Conditional ───────────────────────────────────────────────────────
    /// §22.2 conditional absorption on the CHECK type. Three rows, in
    /// dominance order:
    ///
    /// 1. `error extends T` ⇒ `error` (the error CARRIER dominates any/never
    ///    and both branches — stays FIRST).
    /// 2. `any extends T ? X : Y` ⇒ `X | Y` — the union of BOTH branches,
    ///    mode-INDEPENDENT (distributive and non-distributive alike). Built
    ///    via [`intern_normalized_union_or_intersection`](Self::intern_normalized_union_or_intersection)
    ///    (the `NormalizeUnion` intern) so `X | X` folds to `X` with canonical
    ///    dedup/order — NOT a raw `Union`. The relation engine would instead
    ///    pick the TRUE branch for an `any` check, so this row MUST live here.
    ///    SKIPPED when `extends` is an `infer` pattern: the true branch would
    ///    bind the infer, so unioning both branches verbatim would leak an
    ///    unbound [`Infer`](SemanticNodeData::Infer) — those fall through to
    ///    the infer-binding path in `build_conditional`.
    /// 3. DISTRIBUTIVE naked-`never` check (`distributive == true`) ⇒ `never`
    ///    (the empty distribution). GATED on `distributive`: a
    ///    non-distributive `never extends T ? X : Y` is the TRUE branch `X`
    ///    (never is assignable to everything) and is decided by the relation
    ///    path in `build_conditional` — collapsing it to `never` here would be
    ///    UNSOUND.
    ///
    /// Everything else (distributive `Union` distribution, the `infer`-binding
    /// paths, ordinary relation selection) is handled below the fast-reject in
    /// `build_conditional`.
    pub(crate) fn absorb_conditional(
        &self,
        check: SemanticNodeId,
        extends: SemanticNodeId,
        true_branch: SemanticNodeId,
        false_branch: SemanticNodeId,
        distributive: bool,
    ) -> Option<QueryBuildOutput> {
        let roots = [check, extends, true_branch, false_branch];
        match self.peek_special(check)? {
            // (1) error dominates any/never and both branches.
            (SpecialKind::Error, err) => Some(self.absorbed_output(err, roots)),
            // (2) `any extends T ? X : Y` ⇒ `X | Y`, unless an infer binding
            //     would be involved (then fall through to the infer path).
            (SpecialKind::Any, _) if !self.extends_is_infer_pattern(extends) => {
                let union = self
                    .intern_normalized_union_or_intersection(&[true_branch, false_branch], true);
                Some(self.absorbed_output(union, roots))
            }
            // (3) distributive naked-`never` ⇒ `never` (empty distribution).
            (SpecialKind::Never, _) if distributive => {
                Some(self.absorbed_output(self.primitive_node(PrimitiveKind::Never), roots))
            }
            _ => None,
        }
    }

    /// Whether `extends` carries an `infer` placeholder ANYWHERE in its node
    /// subtree (the positions the conditional infer-binding path could bind).
    /// Used to keep the §22 `any`-row from unioning both branches when the true
    /// branch would otherwise bind an `infer` (unioning verbatim leaks an
    /// unbound [`Infer`](SemanticNodeData::Infer)).
    ///
    /// The scan is EXHAUSTIVE over [`SemanticNodeData`]: it recurses into every
    /// child [`SemanticNodeId`] of every composite carrier — including a
    /// generic application's args ([`InstantiationRef`](SemanticNodeData::InstantiationRef),
    /// e.g. `Wrapper<infer U>`), an [`Object`](SemanticNodeData::Object)
    /// surface's member / signature / keyspace node ids, a
    /// [`Mapped`](SemanticNodeData::Mapped) source + mapper node ids, a
    /// [`TypeParam`](SemanticNodeData::TypeParam) constraint/default, a
    /// [`MergedDecl`](SemanticNodeData::MergedDecl)'s contributors, and a
    /// [`Function`](SemanticNodeData::Function)'s param / return / type-param
    /// node ids. The match has NO catch-all: leaf carriers
    /// ([`Primitive`](SemanticNodeData::Primitive), literals,
    /// [`Opaque`](SemanticNodeData::Opaque),
    /// [`TypeOf`](SemanticNodeData::TypeOf),
    /// [`VueMacroElements`](SemanticNodeData::VueMacroElements),
    /// [`DeclRef`](SemanticNodeData::DeclRef)) hold no nested infer-bearing node
    /// id and stop the walk explicitly, so a future variant addition forces a
    /// compile error here rather than silently regressing the guard.
    ///
    /// Cheap `node_data` peeks only (no resolver/execute work), bounded by
    /// `SCAN_BUDGET` for cycle safety. Budget exhaustion is conservative — it
    /// returns `true` ("infer may be present") so the caller falls through to
    /// the infer-binding path rather than risking a leak.
    fn extends_is_infer_pattern(&self, extends: SemanticNodeId) -> bool {
        // bounded-loop: at most SCAN_BUDGET node_data peeks; budget exhaustion
        // is treated conservatively as "infer may be present".
        const SCAN_BUDGET: usize = 64;
        let graph = self.graph();
        let mut stack = vec![extends];
        let mut budget = SCAN_BUDGET;
        while let Some(id) = stack.pop() {
            if budget == 0 {
                return true;
            }
            budget -= 1;
            let Some(data) = graph.node_data(id) else {
                continue;
            };
            match &*data {
                // The target: an `infer X` placeholder anywhere in the subtree.
                SemanticNodeData::Infer { .. } => return true,

                // ── Composite carriers: recurse into EVERY child node id. ──
                SemanticNodeData::Alias(inner) => stack.push(*inner),
                SemanticNodeData::Union(members) | SemanticNodeData::Intersection(members) => {
                    stack.extend(members.iter().copied());
                }
                SemanticNodeData::MergedDecl { contributors } => {
                    stack.extend(contributors.iter().copied());
                }
                SemanticNodeData::Object(surface) => {
                    stack.extend(surface.members.iter().map(|m| m.value));
                    stack.extend(surface.call_signatures.iter().copied());
                    stack.extend(surface.construct_signatures.iter().copied());
                    for sig in surface.index_signatures.iter() {
                        stack.push(sig.key_type);
                        stack.push(sig.value_type);
                    }
                    stack.extend(surface.keyspace);
                }
                SemanticNodeData::Array { element, .. } => stack.push(*element),
                SemanticNodeData::Tuple { elements, .. } => {
                    stack.extend(elements.iter().map(|e| e.value));
                }
                SemanticNodeData::TemplateLiteral { expressions, .. } => {
                    stack.extend(expressions.iter().copied());
                }
                SemanticNodeData::KeyOf { base } => stack.push(*base),
                SemanticNodeData::IndexedAccess { object, index } => {
                    stack.push(*object);
                    if let IndexKey::TypeNode(idx) = index {
                        stack.push(*idx);
                    }
                }
                SemanticNodeData::Mapped { source, mapper } => {
                    stack.push(*source);
                    stack.push(mapper.parameter_node);
                    stack.push(mapper.key_space);
                    stack.push(mapper.value_expr);
                    stack.extend(mapper.name_remap);
                }
                SemanticNodeData::TypeParam {
                    constraint,
                    default,
                    ..
                } => {
                    stack.extend(*constraint);
                    stack.extend(*default);
                }
                SemanticNodeData::Conditional {
                    check,
                    extends,
                    true_branch_ref,
                    false_branch_ref,
                    ..
                } => {
                    stack.push(*check);
                    stack.push(*extends);
                    stack.push(*true_branch_ref);
                    stack.push(*false_branch_ref);
                }
                SemanticNodeData::Function {
                    params,
                    return_type,
                    type_parameters,
                    ..
                } => {
                    stack.extend(params.iter().map(|p| p.ty));
                    stack.push(*return_type);
                    for tp in type_parameters.iter() {
                        stack.extend(tp.constraint);
                        stack.extend(tp.default);
                    }
                }
                SemanticNodeData::InstantiationRef { args, .. } => {
                    stack.extend(args.iter().copied());
                }

                // ── Leaf carriers: no child node id can hold a nested `infer`
                //    (TS rejects `infer` outside conditional `extends`, and
                //    these variants carry no infer-bearing node id). Explicit —
                //    no catch-all — so a new variant forces a compile error. ──
                SemanticNodeData::Primitive(_)
                | SemanticNodeData::Literal(_)
                | SemanticNodeData::Opaque(_)
                | SemanticNodeData::TypeOf { .. }
                | SemanticNodeData::VueMacroElements(_)
                | SemanticNodeData::DeclRef { .. } => {}
            }
        }
        false
    }
}
