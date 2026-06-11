//! Canonical display policy.
//!
//! Display is a PROJECTION over the typed [`SemanticQueryValue`] /
//! [`SemanticNodeData`] graph — NEVER a stored or re-parsed string. The single
//! [`display`] entry point walks an already-computed value plus a display-only
//! [`DisplayNeeds`] bitset and emits a [`DisplayString`]. This ties two
//! architecture rules together:
//!
//! - the **typed-IR-only** rule: a display string is never re-parsed by a
//!   resolver — display only ever *reads* the typed graph, never dispatches a
//!   query, re-parses, or re-resolves.
//! - the **CodeTransform** rule: a typed result is never post-hoc string-spliced
//!   — every rendered token is computed structurally from node data + `needs`.
//!
//! `display_needs` is DISPLAY-ONLY: it never enters a typed-value family key
//! (it is masked to `⊥` unconditionally by
//! [`apply_mask`](super::demand::apply_mask)), so two queries differing only in
//! their display needs share one cached typed value and differ ONLY in the
//! string [`display`] projects from it. See
//! `docs/arch/u2-query-value-domain-design.md` §14.
//!
//! ### Note on [`DisplayFacet::ExpandAliases`]
//!
//! Whether an alias is kept as a name or inlined as a body is decided UPSTREAM,
//! at resolution time, by `EvalPolicy.alias_preservation` (§14.3): a value
//! resolved with `alias_preservation = Keep` is a lazy named reference
//! ([`SemanticNodeData::DeclRef`] / [`SemanticNodeData::InstantiationRef`]) with
//! NO body materialised in the graph; a value resolved with `Inline` is the
//! already-inlined body node. `display` honours that decision by rendering
//! whatever the graph carries — it MUST NOT follow a lazy reference (that would
//! require a dispatch). `ExpandAliases` therefore selects a richer rendering
//! only where a body is actually reachable; on a lazy reference it is a no-op,
//! because expansion never re-resolves (§14.3). [`SemanticNodeData::Alias`] is a
//! transparent structural indirection (it carries a target, not a name) and is
//! always followed.

use super::demand::{DisplayFacet, DisplayNeeds};
use super::{
    DeclarationAnalysisValue, FunctionParam, IndexKey, LiteralValue, OptionalityMod, PrimitiveKind,
    ProgramAnalysisValue, ReadonlyMod, RelationOutcome, RelationPayload, SemanticNodeData,
    SemanticNodeId, SemanticQueryValue, SignatureRef, TypeParamDecl,
};
use crate::project_semantic_dispatch::walk::{
    MergedDeclDisplaySurface, MergedDeclMember, MergedDeclSurface, ShallowSurfaceMember,
};
use crate::semantic_query_memo::SemanticGraphStore;
use std::fmt;

/// A rendered display string projected from a [`SemanticQueryValue`]. A plain
/// newtype over [`String`]: it is the *output* of the projection, carries no
/// identity, and is safe to be fully `pub`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DisplayString(pub String);

impl DisplayString {
    /// Borrow the rendered string.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for DisplayString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<DisplayString> for String {
    fn from(value: DisplayString) -> Self {
        value.0
    }
}

/// Display-side recursion cap (`docs/arch/u2-query-value-domain-design.md` §14,
/// termination rule 3). It bounds the structural walk independently of any
/// resolver-side budget, so even a pathological graph (deep nesting that escaped
/// a cycle back-reference) terminates with a `…` truncation token.
pub(crate) const MAX_DISPLAY_DEPTH: usize = 64;

/// Beyond this many members a union is truncated when
/// [`DisplayFacet::TruncateLargeUnions`] is set.
const UNION_TRUNCATION_THRESHOLD: usize = 8;

/// The truncation / unresolved token
/// (`docs/arch/u2-query-value-domain-design.md` §14, termination rule 3).
const TRUNCATION_TOKEN: &str = "…";

/// The single §14.1 projection rule: render an already-computed
/// [`SemanticQueryValue`] under `needs`. NEVER stores, re-parses, or
/// re-resolves — it only reads `store` node data + `needs`.
pub fn display(
    store: &SemanticGraphStore,
    value: &SemanticQueryValue,
    needs: DisplayNeeds,
) -> DisplayString {
    match value {
        SemanticQueryValue::TypeNode(id) => {
            display_type_node(store, *id, needs, MAX_DISPLAY_DEPTH, &mut Vec::new())
        }
        SemanticQueryValue::OverloadSet(sigs) => {
            let rendered: Vec<String> = sigs
                .iter()
                .map(|s| display_signature(store, s, needs).0)
                .collect();
            DisplayString(rendered.join("; "))
        }
        SemanticQueryValue::Relation(payload) => display_relation(payload),
        SemanticQueryValue::DeclarationAnalysis(d) => display_declaration_analysis(store, d, needs),
        SemanticQueryValue::ProgramAnalysis(p) => display_program_analysis(store, p, needs),
        // §14.1: the reserved native-checker seam. No producer constructs it,
        // so reaching it here is a logic error. Matched explicitly — NOT via a
        // `_` wildcard — so any future live `SemanticQueryValue` arm forces a
        // compile error here instead of silently falling through.
        SemanticQueryValue::DiagnosticAnalysis(_) => unreachable!(
            "display: DiagnosticAnalysis is a non-live reserved seam — no producer constructs it"
        ),
    }
}

/// Structural walk of one graph node. EXHAUSTIVE over every
/// [`SemanticNodeData`] variant — no `_` wildcard, so a new node variant forces
/// a compile error here. `depth` decrements per level; `visited` is the
/// ancestor stack used to break cyclic / recursive types.
pub(crate) fn display_type_node(
    store: &SemanticGraphStore,
    id: SemanticNodeId,
    needs: DisplayNeeds,
    depth: usize,
    visited: &mut Vec<SemanticNodeId>,
) -> DisplayString {
    // Termination #3: depth cap.
    if depth == 0 {
        return DisplayString(TRUNCATION_TOKEN.to_string());
    }
    let data = match store.node_data(id) {
        Some(d) => d,
        // Unresolved / un-interned id — render the truncation token rather than
        // panicking (display tolerates a torn graph).
        None => return DisplayString(TRUNCATION_TOKEN.to_string()),
    };
    // Cycle break: a node already on the ancestor stack renders as a
    // back-reference (its name when available, else `…`) instead of recursing.
    if visited.contains(&id) {
        return DisplayString(back_ref_token(&data));
    }
    visited.push(id);
    let child_depth = depth - 1;
    let out = match data.as_ref() {
        // Transparent structural indirection — always followed (it carries a
        // target, not a name); raise.rs treats it identically.
        SemanticNodeData::Alias(target) => {
            display_type_node(store, *target, needs, child_depth, visited).0
        }
        SemanticNodeData::Object(surface) => {
            let mut parts: Vec<String> = Vec::new();
            for member in surface.members.iter() {
                let mut s = String::new();
                if member.readonly && needs.contains(DisplayFacet::IncludeReadonlyModifier) {
                    s.push_str("readonly ");
                }
                s.push_str(&member_name_token(&member.name));
                if member.optional {
                    s.push('?');
                }
                if member.is_method && resolves_to_function(store, member.value) {
                    // Method shorthand `name(params): ret` — the signature is
                    // rendered in type-literal (colon) position, NOT as a
                    // property holding an arrow-function type. Only valid when
                    // the value is actually a `Function`: after intersection
                    // merging `is_method` can be ORed true over an
                    // `Intersection` of overloads, which must fall through to
                    // property style (a colon) below to stay valid TS.
                    s.push_str(&render_signature_colon(
                        store,
                        member.value,
                        "",
                        needs,
                        child_depth,
                        visited,
                    ));
                } else {
                    s.push_str(": ");
                    s.push_str(
                        &display_type_node(store, member.value, needs, child_depth, visited).0,
                    );
                }
                parts.push(s);
            }
            for sig in surface.call_signatures.iter() {
                parts.push(render_signature_colon(
                    store,
                    *sig,
                    "",
                    needs,
                    child_depth,
                    visited,
                ));
            }
            for sig in surface.construct_signatures.iter() {
                parts.push(render_signature_colon(
                    store,
                    *sig,
                    "new ",
                    needs,
                    child_depth,
                    visited,
                ));
            }
            for index in surface.index_signatures.iter() {
                let mut s = String::new();
                if index.readonly && needs.contains(DisplayFacet::IncludeReadonlyModifier) {
                    s.push_str("readonly ");
                }
                s.push_str(&format!(
                    "[key: {}]: {}",
                    display_type_node(store, index.key_type, needs, child_depth, visited).0,
                    display_type_node(store, index.value_type, needs, child_depth, visited).0
                ));
                parts.push(s);
            }
            if parts.is_empty() {
                "{}".to_string()
            } else {
                format!("{{ {} }}", parts.join("; "))
            }
        }
        SemanticNodeData::Union(arms) => {
            // A union arm only parenthesises a LOOSER binder (Conditional /
            // Function). A same-kind nested union arm (`A | (B | C)`) and a
            // tighter intersection arm (`A | B & C`) both render bare.
            let rendered: Vec<String> = arms
                .iter()
                .map(|a| render_operand(store, *a, needs, child_depth, visited, Prec::Union))
                .collect();
            if needs.contains(DisplayFacet::TruncateLargeUnions)
                && rendered.len() > UNION_TRUNCATION_THRESHOLD
            {
                let head = rendered[..UNION_TRUNCATION_THRESHOLD].join(" | ");
                format!("{head} | {TRUNCATION_TOKEN}")
            } else {
                rendered.join(" | ")
            }
        }
        SemanticNodeData::Intersection(arms) => {
            // An intersection arm parenthesises looser binders (Conditional /
            // Function → Loose, and Union), but NOT a same-kind nested
            // intersection (`A & (B & C)` renders `A & B & C`).
            let rendered: Vec<String> = arms
                .iter()
                .map(|a| render_operand(store, *a, needs, child_depth, visited, Prec::Intersection))
                .collect();
            rendered.join(" & ")
        }
        // A same-name merged declaration renders a peer-merged surface from a
        // transient projection. Display stays read-only: semantic consumers that
        // need a graph node materialize through the reducer before rendering.
        SemanticNodeData::MergedDecl { contributors } => {
            let surface =
                crate::project_semantic_dispatch::walk::reduce_merged_decl_display_surface(
                    store,
                    contributors,
                );
            render_merged_decl_surface(store, &surface, needs, child_depth, visited)
        }
        SemanticNodeData::Primitive(kind) => primitive_keyword(*kind).to_string(),
        SemanticNodeData::Literal(value) => literal_token(value),
        // Error carrier riding through display — a concise token, NOT a panic.
        SemanticNodeData::Opaque(_) => "<error>".to_string(),
        SemanticNodeData::Array { element, readonly } => {
            // The leading-`readonly` postfix-base parenthesisation is handled
            // uniformly inside `render_operand` for `Prec::Postfix` operands.
            let inner = render_operand(store, *element, needs, child_depth, visited, Prec::Postfix);
            let ro = *readonly && needs.contains(DisplayFacet::IncludeReadonlyModifier);
            if ro {
                format!("readonly {inner}[]")
            } else {
                format!("{inner}[]")
            }
        }
        SemanticNodeData::Tuple { elements, readonly } => {
            let mut parts: Vec<String> = Vec::new();
            for el in elements.iter() {
                let mut s = String::new();
                if el.rest {
                    s.push_str("...");
                }
                if let Some(label) = &el.label {
                    s.push_str(label);
                    if el.optional {
                        s.push('?');
                    }
                    s.push_str(": ");
                    s.push_str(&display_type_node(store, el.value, needs, child_depth, visited).0);
                } else {
                    s.push_str(&display_type_node(store, el.value, needs, child_depth, visited).0);
                    if el.optional {
                        s.push('?');
                    }
                }
                parts.push(s);
            }
            let body = format!("[{}]", parts.join(", "));
            if *readonly && needs.contains(DisplayFacet::IncludeReadonlyModifier) {
                format!("readonly {body}")
            } else {
                body
            }
        }
        SemanticNodeData::TemplateLiteral {
            quasis,
            expressions,
        } => {
            let mut s = String::from("`");
            for (i, quasi) in quasis.iter().enumerate() {
                // Quasis are stored as RAW source text (`q.value.raw` —
                // `verter_type_expr_oxc`), already carrying source-level escapes
                // for backslash / backtick / `${`. They round-trip VERBATIM;
                // re-escaping would double-escape them.
                s.push_str(quasi);
                if let Some(expr) = expressions.get(i) {
                    s.push_str("${");
                    s.push_str(&display_type_node(store, *expr, needs, child_depth, visited).0);
                    s.push('}');
                }
            }
            s.push('`');
            s
        }
        SemanticNodeData::KeyOf { base } => {
            let inner = render_operand(store, *base, needs, child_depth, visited, Prec::Prefix);
            format!("keyof {inner}")
        }
        SemanticNodeData::IndexedAccess { object, index } => {
            let obj = render_operand(store, *object, needs, child_depth, visited, Prec::Postfix);
            let key = match index {
                IndexKey::String(s) => single_quoted(s),
                IndexKey::Number(n) => n.to_string(),
                IndexKey::TypeNode(node) => {
                    display_type_node(store, *node, needs, child_depth, visited).0
                }
            };
            format!("{obj}[{key}]")
        }
        SemanticNodeData::Mapped { source: _, mapper } => {
            let param =
                display_type_node(store, mapper.parameter_node, needs, child_depth, visited).0;
            let key_space =
                display_type_node(store, mapper.key_space, needs, child_depth, visited).0;
            let value = display_type_node(store, mapper.value_expr, needs, child_depth, visited).0;
            // Readonly / optionality / `as` name-remap are LIVE graph fields,
            // each rendered structurally (TS mapped-modifier spelling).
            let readonly = match mapper.readonly {
                ReadonlyMod::Add => "readonly ",
                ReadonlyMod::Remove => "-readonly ",
                ReadonlyMod::Keep => "",
            };
            let optionality = match mapper.optionality {
                OptionalityMod::Add => "?",
                OptionalityMod::Remove => "-?",
                OptionalityMod::Keep => "",
            };
            let remap = match mapper.name_remap {
                Some(node) => format!(
                    " as {}",
                    display_type_node(store, node, needs, child_depth, visited).0
                ),
                None => String::new(),
            };
            format!("{{ {readonly}[{param} in {key_space}{remap}]{optionality}: {value} }}")
        }
        SemanticNodeData::TypeOf { value_root, path } => {
            let mut s = format!("typeof {}", value_root.name);
            for seg in path.iter() {
                s.push('.');
                s.push_str(seg);
            }
            s
        }
        SemanticNodeData::TypeParam { display_name, .. } => display_name.to_string(),
        SemanticNodeData::Infer { name } => format!("infer {name}"),
        SemanticNodeData::Conditional {
            check,
            extends,
            true_branch_ref,
            false_branch_ref,
            distributive: _,
        } => {
            // `check` / `extends` sit before `extends` / `?`, so any compound
            // operand (Function / Conditional → Loose, Union, Intersection)
            // must parenthesise: `Prec::Prefix` wraps everything looser than a
            // prefix operator. The TRUE branch must wrap a nested Loose binder
            // (a bare nested conditional there mis-reads), so it renders at
            // `Prec::Union` (wraps Conditional / Function only). The FALSE
            // branch is the trailing, right-associative position and stays bare
            // — `A extends B ? C : D extends E ? F : G` already nests correctly.
            let check = render_operand(store, *check, needs, child_depth, visited, Prec::Prefix);
            let extends =
                render_operand(store, *extends, needs, child_depth, visited, Prec::Prefix);
            let t = render_operand(
                store,
                *true_branch_ref,
                needs,
                child_depth,
                visited,
                Prec::Union,
            );
            let f = display_type_node(store, *false_branch_ref, needs, child_depth, visited).0;
            format!("{check} extends {extends} ? {t} : {f}")
        }
        // The Vue-macro carrier holds the parser's `ResolvedElements` struct,
        // which is NOT a `SemanticNodeData` surface. Display renders a concise
        // structural summary derived from the live payload (prop / emit counts
        // plus callability) — reading its shape shallowly, never re-resolving it
        // into the graph (that would require a dispatch).
        SemanticNodeData::VueMacroElements(elements) => {
            let mut s = format!(
                "<vue-macro props={} emits={}",
                elements.props.len(),
                elements.call_signatures.len()
            );
            if elements.has_call_signature {
                s.push_str(" callable");
            }
            s.push('>');
            s
        }
        SemanticNodeData::Function {
            params,
            return_type,
            type_parameters,
            signature_span: _,
            return_type_span: _,
        } => {
            let tps = render_type_parameters(store, type_parameters, needs, child_depth, visited);
            let rendered_params = render_params(store, params, needs, child_depth, visited);
            let ret = display_type_node(store, *return_type, needs, child_depth, visited).0;
            format!("{tps}({rendered_params}) => {ret}")
        }
        // Lazy named references: render the NAME (§14.3 — a lazy ref carries no
        // materialised body, and display must not re-resolve, so `ExpandAliases`
        // is a no-op here). `QualifyNames` qualifies with the declaration origin.
        SemanticNodeData::DeclRef { identity } => {
            qualified_name(needs, &identity.canonical_id, &identity.decl_name)
        }
        SemanticNodeData::InstantiationRef { base, args } => {
            let name = qualified_name(needs, &base.canonical_id, &base.decl_name);
            if args.is_empty() {
                name
            } else {
                let rendered: Vec<String> = args
                    .iter()
                    .map(|a| display_type_node(store, *a, needs, child_depth, visited).0)
                    .collect();
                format!("{name}<{}>", rendered.join(", "))
            }
        }
    };
    visited.pop();
    DisplayString(out)
}

fn render_merged_decl_surface(
    store: &SemanticGraphStore,
    surface: &MergedDeclDisplaySurface,
    needs: DisplayNeeds,
    depth: usize,
    visited: &mut Vec<SemanticNodeId>,
) -> String {
    let own = render_merged_decl_own_surface(store, &surface.own_surface, needs, depth, visited);
    if surface.heritage_arms.is_empty() {
        return own;
    }
    let mut rendered: Vec<String> = surface
        .heritage_arms
        .iter()
        .map(|arm| render_operand(store, *arm, needs, depth, visited, Prec::Intersection))
        .collect();
    rendered.push(own);
    rendered.join(" & ")
}

fn render_merged_decl_own_surface(
    store: &SemanticGraphStore,
    surface: &MergedDeclSurface,
    needs: DisplayNeeds,
    depth: usize,
    visited: &mut Vec<SemanticNodeId>,
) -> String {
    let mut parts: Vec<String> = Vec::new();
    for member in &surface.members {
        parts.push(render_merged_decl_member(
            store, member, needs, depth, visited,
        ));
    }
    for sig in surface.call_signatures.iter() {
        parts.push(render_signature_colon(
            store, *sig, "", needs, depth, visited,
        ));
    }
    for sig in surface.construct_signatures.iter() {
        parts.push(render_signature_colon(
            store, *sig, "new ", needs, depth, visited,
        ));
    }
    for index in surface.index_signatures.iter() {
        let mut s = String::new();
        if index.readonly && needs.contains(DisplayFacet::IncludeReadonlyModifier) {
            s.push_str("readonly ");
        }
        s.push_str(&format!(
            "[key: {}]: {}",
            display_type_node(store, index.key_type, needs, depth, visited).0,
            display_type_node(store, index.value_type, needs, depth, visited).0
        ));
        parts.push(s);
    }
    if parts.is_empty() {
        "{}".to_string()
    } else {
        format!("{{ {} }}", parts.join("; "))
    }
}

/// Render one peer-merged member. This mirrors the `Object` arm of
/// [`display_type_node`] applied to the graph reducer's reduced surface: a
/// single-signature method renders as method shorthand (`name(params): ret`);
/// an accumulated overload group (`values.len() > 1`) renders as a PROPERTY
/// holding the intersection of its function types — exactly what the graph
/// reducer's interned `Intersection` overload value renders as (an `is_method`
/// member whose value is an `Intersection` falls through to property style to
/// stay valid TS). Display therefore never emits separate method signatures
/// where the reduced Object would not.
fn render_merged_decl_member(
    store: &SemanticGraphStore,
    merged: &MergedDeclMember,
    needs: DisplayNeeds,
    depth: usize,
    visited: &mut Vec<SemanticNodeId>,
) -> String {
    let member = &merged.member;
    let mut s = member_prefix(member, needs);
    match merged.values.as_slice() {
        [value] if member.is_method && resolves_to_function(store, *value) => {
            s.push_str(&render_signature_colon(
                store, *value, "", needs, depth, visited,
            ));
        }
        [value] => {
            s.push_str(": ");
            s.push_str(&display_type_node(store, *value, needs, depth, visited).0);
        }
        [] => {
            s.push_str(": ");
            s.push_str(TRUNCATION_TOKEN);
        }
        values => {
            s.push_str(": ");
            let rendered: Vec<String> = values
                .iter()
                .map(|value| {
                    render_operand(store, *value, needs, depth, visited, Prec::Intersection)
                })
                .collect();
            s.push_str(&rendered.join(" & "));
        }
    }
    s
}

fn member_prefix(member: &ShallowSurfaceMember, needs: DisplayNeeds) -> String {
    let mut s = String::new();
    if member.readonly && needs.contains(DisplayFacet::IncludeReadonlyModifier) {
        s.push_str("readonly ");
    }
    s.push_str(&member_name_token(&member.name));
    if member.optional {
        s.push('?');
    }
    s
}

/// Render one overload signature (a graph node, typically a
/// [`SemanticNodeData::Function`]).
fn display_signature(
    store: &SemanticGraphStore,
    sig: &SignatureRef,
    needs: DisplayNeeds,
) -> DisplayString {
    display_type_node(store, sig.node, needs, MAX_DISPLAY_DEPTH, &mut Vec::new())
}

/// Render a relation outcome from the payload — never recomputed (§14.2).
/// Renders the `outcome` only; the payload-side proof table never appears on a
/// display surface. The public outcome is three-valued — `Assignable` /
/// `NotAssignable` / `BudgetExceeded` — and has NO `Unknown` form (a deferred /
/// undischarged relation routes through `ReturnOnly` and never surfaces here).
fn display_relation(payload: &RelationPayload) -> DisplayString {
    let token = match &payload.outcome {
        RelationOutcome::Assignable => "true",
        RelationOutcome::NotAssignable => "false",
        RelationOutcome::BudgetExceeded(_) => "budget-exceeded",
    };
    DisplayString(token.to_string())
}

/// Render the merged declaration / augmentation surface: each contributor node
/// rendered shallow, joined as a merge (`&`). Contributors render at
/// `Prec::Intersection` — identical to the `Intersection` arm — so a looser
/// contributor (Union / Conditional / Function) parenthesises rather than
/// mis-binding under the implicit `&`.
fn display_declaration_analysis(
    store: &SemanticGraphStore,
    value: &DeclarationAnalysisValue,
    needs: DisplayNeeds,
) -> DisplayString {
    let rendered: Vec<String> = value
        .contributors
        .iter()
        .map(|c| {
            render_operand(
                store,
                *c,
                needs,
                MAX_DISPLAY_DEPTH,
                &mut Vec::new(),
                Prec::Intersection,
            )
        })
        .collect();
    DisplayString(rendered.join(" & "))
}

/// Render a narrowed / contextual program-analysis node.
fn display_program_analysis(
    store: &SemanticGraphStore,
    value: &ProgramAnalysisValue,
    needs: DisplayNeeds,
) -> DisplayString {
    display_type_node(store, value.node, needs, MAX_DISPLAY_DEPTH, &mut Vec::new())
}

/// The back-reference token emitted when a node is already on the ancestor
/// stack: its name when the node carries one, else the truncation token.
fn back_ref_token(data: &SemanticNodeData) -> String {
    match data {
        SemanticNodeData::DeclRef { identity } => identity.decl_name.to_string(),
        SemanticNodeData::InstantiationRef { base, .. } => base.decl_name.to_string(),
        SemanticNodeData::TypeParam { display_name, .. } => display_name.to_string(),
        _ => TRUNCATION_TOKEN.to_string(),
    }
}

/// A declaration name, optionally qualified with its canonical origin when
/// [`DisplayFacet::QualifyNames`] is set. Never fabricates an origin: the bare
/// name is used when qualification is not requested.
fn qualified_name(needs: DisplayNeeds, canonical_id: &str, decl_name: &str) -> String {
    if needs.contains(DisplayFacet::QualifyNames) {
        format!("{canonical_id}:{decl_name}")
    } else {
        decl_name.to_string()
    }
}

/// Canonical primitive keyword spellings — match
/// `resolver_core::surface_projector`'s leaf conventions so the two renderers
/// agree (this walks `SemanticNodeData`, not `TypeExpr`, so it cannot call it).
fn primitive_keyword(kind: PrimitiveKind) -> &'static str {
    match kind {
        PrimitiveKind::String => "string",
        PrimitiveKind::Number => "number",
        PrimitiveKind::Boolean => "boolean",
        PrimitiveKind::Symbol => "symbol",
        PrimitiveKind::BigInt => "bigint",
        PrimitiveKind::Any => "any",
        PrimitiveKind::Unknown => "unknown",
        PrimitiveKind::Void => "void",
        PrimitiveKind::Never => "never",
        PrimitiveKind::Null => "null",
        PrimitiveKind::Undefined => "undefined",
        PrimitiveKind::Object => "object",
    }
}

/// Canonical literal spellings — string literals single-quoted (and escaped);
/// number / bool / bigint verbatim (matches `surface_projector`'s leaf
/// conventions).
fn literal_token(value: &LiteralValue) -> String {
    match value {
        LiteralValue::String(s) => single_quoted(s),
        LiteralValue::Number(n) => n.to_string(),
        LiteralValue::Boolean(b) => b.to_string(),
        // `LiteralValue::BigInt` stores base-10 digits WITHOUT the `n` suffix
        // (`verter_type_expr_oxc` = `b.value.to_string()`); the suffix is what
        // makes it a bigint-literal type rather than a number literal, so append
        // it. The stored string never carries `n`, so this never double-appends.
        LiteralValue::BigInt(s) => format!("{s}n"),
    }
}

/// Render an object member name. A name that is a valid bare TS member key (a
/// valid identifier, or an all-digit numeric key — both legal unquoted) is
/// emitted as-is; any other name (e.g. `"foo-bar"`, `"has space"`) is quoted as
/// a single-quoted escaped string so the surface stays valid TS.
fn member_name_token(name: &str) -> String {
    if is_bare_member_name(name) {
        name.to_string()
    } else {
        single_quoted(name)
    }
}

/// Structural test for a name that may appear unquoted as an object member key:
/// a valid TS identifier (`[A-Za-z_$][A-Za-z0-9_$]*`) or an all-ASCII-digit
/// numeric key (`123:` is legal unquoted).
fn is_bare_member_name(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if name.bytes().all(|b| b.is_ascii_digit()) {
        return true;
    }
    if !(first.is_ascii_alphabetic() || first == '_' || first == '$') {
        return false;
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$')
}

/// Render `s` as a single-quoted TS string literal, escaping `\`, `'`, and
/// every control char (`< 0x20`) so the rendered token is valid (and round-trips
/// through a parser without splicing the surrounding type).
fn single_quoted(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('\'');
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\'' => out.push_str("\\'"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            // Any remaining non-printing control char (NUL, backspace,
            // form-feed, vertical-tab, …) escapes generically as `\u{..}` so the
            // rendered token stays a valid TS string literal.
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{{{:x}}}", c as u32)),
            other => out.push(other),
        }
    }
    out.push('\'');
    out
}

/// Render a function's type-parameter clause `<T extends C = D, …>`. Empty when
/// the function declares no type parameters. Constraint and default are LIVE
/// [`TypeParamDecl`] fields and are rendered when present; a bare parameter is
/// just its name.
fn render_type_parameters(
    store: &SemanticGraphStore,
    type_parameters: &[TypeParamDecl],
    needs: DisplayNeeds,
    depth: usize,
    visited: &mut Vec<SemanticNodeId>,
) -> String {
    if type_parameters.is_empty() {
        return String::new();
    }
    let rendered: Vec<String> = type_parameters
        .iter()
        .map(|tp| {
            let mut s = tp.name.to_string();
            if let Some(constraint) = tp.constraint {
                s.push_str(" extends ");
                s.push_str(&display_type_node(store, constraint, needs, depth, visited).0);
            }
            if let Some(default) = tp.default {
                s.push_str(" = ");
                s.push_str(&display_type_node(store, default, needs, depth, visited).0);
            }
            s
        })
        .collect();
    format!("<{}>", rendered.join(", "))
}

/// Render a parameter list body `name: T, …` (no surrounding parens). Shared by
/// the standalone-function arrow rendering and the object-position colon
/// rendering.
fn render_params(
    store: &SemanticGraphStore,
    params: &[FunctionParam],
    needs: DisplayNeeds,
    depth: usize,
    visited: &mut Vec<SemanticNodeId>,
) -> String {
    let rendered: Vec<String> = params
        .iter()
        .map(|p| {
            let mut ps = String::new();
            if p.rest {
                ps.push_str("...");
            }
            ps.push_str(p.name.as_deref().unwrap_or("arg"));
            if p.optional {
                ps.push('?');
            }
            ps.push_str(": ");
            ps.push_str(&display_type_node(store, p.ty, needs, depth, visited).0);
            ps
        })
        .collect();
    rendered.join(", ")
}

/// Render a [`SemanticNodeData::Function`] signature node in object / type-
/// literal position, where the return type is introduced by a COLON
/// (`(params): ret`) — distinct from the standalone arrow form. `prefix` is
/// `""` for a call signature, `"new "` for a construct signature, or already
/// holds the member name for a method shorthand.
fn render_signature_colon(
    store: &SemanticGraphStore,
    sig: SemanticNodeId,
    prefix: &str,
    needs: DisplayNeeds,
    depth: usize,
    visited: &mut Vec<SemanticNodeId>,
) -> String {
    // Follow the transparent `Alias` chain to the underlying node before
    // matching: a member with `value = Alias(Function)` enters method shorthand
    // (via `resolves_to_function`, which also follows the chain), so the colon
    // form must reach the same `Function` — otherwise an `Alias` falls to the
    // defensive arm and renders the arrow form, an invalid `name(p) => r` hybrid.
    let resolved = follow_alias_chain(store, sig);
    match store.node_data(resolved).as_deref() {
        Some(SemanticNodeData::Function {
            params,
            return_type,
            type_parameters,
            ..
        }) => {
            let tps = render_type_parameters(store, type_parameters, needs, depth, visited);
            let rendered_params = render_params(store, params, needs, depth, visited);
            let ret = display_type_node(store, *return_type, needs, depth, visited).0;
            format!("{prefix}{tps}({rendered_params}): {ret}")
        }
        // Defensive: a genuinely non-function signature node renders through its
        // own arm, prefixed verbatim.
        _ => format!(
            "{prefix}{}",
            display_type_node(store, sig, needs, depth, visited).0
        ),
    }
}

/// Structural precedence of a rendered type, used to decide parenthesisation by
/// NODE KIND rather than by scanning rendered text. Higher binds tighter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Prec {
    /// `a extends b ? c : d` and `(…) => r` — the loosest binders.
    Loose = 1,
    /// `a | b`
    Union = 2,
    /// `a & b`
    Intersection = 3,
    /// prefix operators: `keyof a`, `infer a`
    Prefix = 4,
    /// postfix operators: `a[]`, `a[K]`
    Postfix = 5,
    /// atomic — names, primitives, literals, `{…}`, `[…]`, `typeof`, template.
    Atom = 6,
}

/// Precedence of a single node by kind. Exhaustive — a new
/// [`SemanticNodeData`] variant forces a decision here rather than defaulting.
fn prec_of(data: &SemanticNodeData) -> Prec {
    match data {
        // A conditional is a loose binder: nested in any tighter position (array
        // element, keyof base, indexed-access object, another conditional's
        // check / extends / true branch) it must parenthesise.
        SemanticNodeData::Conditional { .. } => Prec::Loose,
        SemanticNodeData::Function { .. } => Prec::Loose,
        SemanticNodeData::Union(_) => Prec::Union,
        SemanticNodeData::Intersection(_) => Prec::Intersection,
        SemanticNodeData::KeyOf { .. } | SemanticNodeData::Infer { .. } => Prec::Prefix,
        SemanticNodeData::Array { .. } | SemanticNodeData::IndexedAccess { .. } => Prec::Postfix,
        // `Alias` is transparent (rendered as its target); its precedence is
        // resolved through the chain by `node_precedence`. Reaching it here
        // means the chain bottomed out — treat as atomic.
        // A merged declaration renders its peer-merged `Object` surface (`{…}`),
        // which is atomic — see the `display_type_node` MergedDecl arm.
        SemanticNodeData::Alias(_)
        | SemanticNodeData::Object(_)
        | SemanticNodeData::Primitive(_)
        | SemanticNodeData::Literal(_)
        | SemanticNodeData::Opaque(_)
        | SemanticNodeData::Tuple { .. }
        | SemanticNodeData::TemplateLiteral { .. }
        | SemanticNodeData::Mapped { .. }
        | SemanticNodeData::TypeOf { .. }
        | SemanticNodeData::TypeParam { .. }
        | SemanticNodeData::VueMacroElements(_)
        | SemanticNodeData::MergedDecl { .. }
        | SemanticNodeData::DeclRef { .. }
        | SemanticNodeData::InstantiationRef { .. } => Prec::Atom,
    }
}

/// Precedence of a node id, following the transparent `Alias` chain (bounded by
/// the display depth cap) to the node that actually renders.
fn node_precedence(store: &SemanticGraphStore, id: SemanticNodeId) -> Prec {
    let mut current = id;
    for _ in 0..MAX_DISPLAY_DEPTH {
        match store.node_data(current).as_deref() {
            Some(SemanticNodeData::Alias(target)) => current = *target,
            Some(data) => return prec_of(data),
            // Unresolved id renders as the atomic truncation token.
            None => return Prec::Atom,
        }
    }
    Prec::Atom
}

/// Follow the transparent `Alias` chain (bounded by the display depth cap) and
/// report whether the node ultimately renders as a [`SemanticNodeData::Function`]
/// — the only shape that may use object method-shorthand. After intersection
/// merging a member's `is_method` flag can be ORed true over an `Intersection`
/// of overloads; such a member must render property-style (with a colon).
fn resolves_to_function(store: &SemanticGraphStore, id: SemanticNodeId) -> bool {
    matches!(
        store.node_data(follow_alias_chain(store, id)).as_deref(),
        Some(SemanticNodeData::Function { .. })
    )
}

/// Follow the transparent `Alias` chain (bounded by the display depth cap) and
/// return the id of the node that actually renders. A non-`Alias` node (or an
/// unresolved id) is returned as-is.
fn follow_alias_chain(store: &SemanticGraphStore, id: SemanticNodeId) -> SemanticNodeId {
    let mut current = id;
    for _ in 0..MAX_DISPLAY_DEPTH {
        match store.node_data(current).as_deref() {
            Some(SemanticNodeData::Alias(target)) => current = *target,
            _ => return current,
        }
    }
    current
}

/// True when `id` resolves (through the transparent `Alias` chain) to an
/// `Array`/`Tuple` node carrying `readonly: true` AND the readonly modifier will
/// actually be rendered (`IncludeReadonlyModifier` active). Such a node placed in
/// postfix position (`X[]`) must parenthesise so its leading `readonly` does not
/// re-read as the OUTER array's modifier: `(readonly string[])[]`. Decided from
/// the node kind + flag + facet — node inspection, NOT rendered-text sniffing.
fn element_renders_leading_readonly(
    store: &SemanticGraphStore,
    id: SemanticNodeId,
    needs: DisplayNeeds,
) -> bool {
    if !needs.contains(DisplayFacet::IncludeReadonlyModifier) {
        return false;
    }
    matches!(
        store.node_data(follow_alias_chain(store, id)).as_deref(),
        Some(SemanticNodeData::Array { readonly: true, .. })
            | Some(SemanticNodeData::Tuple { readonly: true, .. })
    )
}

/// Render `id` and parenthesise it when its structural precedence binds looser
/// than `min_prec` requires in the enclosing position. Parenthesisation is
/// structural: the decision is taken from the node kind, so a `Conditional`
/// operand wraps and a string literal whose TEXT contains `" | "` does not.
fn render_operand(
    store: &SemanticGraphStore,
    id: SemanticNodeId,
    needs: DisplayNeeds,
    depth: usize,
    visited: &mut Vec<SemanticNodeId>,
    min_prec: Prec,
) -> String {
    let rendered = display_type_node(store, id, needs, depth, visited).0;
    // Postfix-base position (`X[]`, `X[K]`): a readonly array/tuple operand
    // renders a leading `readonly` that would otherwise re-read as the OUTER
    // postfix operator's modifier, so it must parenthesise even though its own
    // precedence (`Postfix`) is not looser than `min_prec`. Folded here so
    // EVERY postfix base (array element, indexed-access object, and any future
    // postfix base) is uniformly correct.
    if min_prec == Prec::Postfix && element_renders_leading_readonly(store, id, needs) {
        return format!("({rendered})");
    }
    if node_precedence(store, id) < min_prec {
        format!("({rendered})")
    } else {
        rendered
    }
}
