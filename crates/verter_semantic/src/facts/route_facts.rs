//! Graph-free per-decl route-fact PRODUCER + its import-classification lens.
//!
//! [`produce_shallow_route_facts`] runs at the lazy decl-body lowering hook
//! over the TRANSIENT contributor bodies of ONE declaration and emits the
//! per-decl DIRECT [`ShallowRouteFacts`]: whole-route edges (deferred local
//! follows + direct external emits + deferred-key utilities), per-member
//! dependency edges, member-path seed edges, and the member-name route. It is
//! a free function over `&[TypeExpr]` + a [`RouteFactLens`]: NO shallow-state
//! access, NO sibling body demand, NO locator deref, NO cross-file
//! resolution — same-file TRANSITIVE closure lives downstream in the session
//! resolver (see [`super::route_closure`]), reading sibling decls' stored
//! facts.
//!
//! [`RouteFactLens`] is a SEPARATE hash-free view of the owning file's shallow
//! import/header tables (the full three-field import target incl. the resolved
//! canonical, plus header-membership). It is deliberately NOT the fingerprint
//! [`CrossDeclLens`](super::CrossDeclLens): the fingerprint lens keeps only
//! the source specifier (parse-domain R12 — resolved canonicals are withheld
//! from `parse_stable_hash`), while route facts NEED `imported_name` +
//! `canonical_id` to mint frontier-ready external refs. The fingerprint
//! grammar is untouched.

use std::sync::Arc;

use verter_type_expr::facts::{
    DeferredKeyUtilityEdge, DeferredKeyUtilityKind, ExternalRouteRefFact, KeyDomainFact,
    KeySourceFact, KeySourceRefFact, MemberDependencyEdge, MemberNamesRoute, MemberPathSeedEdge,
    MemberPathSeedTarget, RouteDependencyRefFact, ShallowRouteFacts, WholeRouteContextFact,
    WholeRouteEdgeFact,
};
use verter_type_expr::locators::{AuthoredAnchor, LocatorSymbolSpace, SymbolBodyLocator};
use verter_type_expr::{LiteralValue, ObjectMember, ObjectProperty, RouteDemand, TypeExpr};

use super::SymbolSpace;

// ---------------------------------------------------------------------------
// Lens
// ---------------------------------------------------------------------------

/// The resolved import target of a local import binding — the full three-field
/// view (specifier + original exported name + optionally-resolved canonical)
/// the route producer classifies `Ref` sites against.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportRouteTarget {
    /// The authored import specifier (e.g. `./types`, `reka-ui`).
    pub source_specifier: Arc<str>,
    /// The original exported name in the source module.
    pub imported_name: Arc<str>,
    /// The resolved canonical file id, when the specifier resolved.
    pub canonical_id: Option<Arc<str>>,
}

/// Hash-free import/header classification view for the route-fact producer.
///
/// A SECOND view of the same shallow tables the fingerprint lens reads — NOT a
/// widening of `CrossDeclLens`/`ShallowLens` (R12: the fingerprint grammar
/// carries only the source specifier; this lens carries the full import target
/// including the resolve-domain canonical, and never feeds a hash).
pub trait RouteFactLens {
    /// Resolve a local name to its import target. `None` ⇒ not an import
    /// binding (a local declaration, a type parameter, or an unknown name —
    /// the caller distinguishes via [`Self::has_type_symbol`]).
    fn resolve_import_route(&self, local: &str, space: SymbolSpace) -> Option<ImportRouteTarget>;

    /// Whether `name` is a file-scope TYPE symbol (header-level membership —
    /// no body demand). Gates the deferred-key-alias classification and the
    /// userland `Pick`/`Omit` empty-keys fallback exactly like the legacy
    /// walk's `has_type_symbol` gate.
    fn has_type_symbol(&self, name: &str) -> bool;

    /// The canonical id of the file whose decls are being produced — anchors
    /// the deferred key-source recipe locator.
    fn own_canonical_id(&self) -> Arc<str>;
}

/// A lens with no imports and no local symbols — for enum-only seeded paths
/// (whose scalar bodies carry no `Ref` sites) and shape-only tests.
#[derive(Debug, Default)]
pub struct EmptyRouteFactLens;

impl RouteFactLens for EmptyRouteFactLens {
    fn resolve_import_route(&self, _local: &str, _space: SymbolSpace) -> Option<ImportRouteTarget> {
        None
    }
    fn has_type_symbol(&self, _name: &str) -> bool {
        false
    }
    fn own_canonical_id(&self) -> Arc<str> {
        Arc::from("")
    }
}

// ---------------------------------------------------------------------------
// Producer
// ---------------------------------------------------------------------------

/// Produce the per-decl DIRECT route facts from one declaration's transient
/// contributor bodies (source order; one element for a single declaration,
/// several for a merged group, empty for an enum's scalar-union projection).
///
/// Graph-free by construction: the walk visits ONLY the supplied bodies; every
/// same-file reference becomes a deferred `Local`/seed/forward edge for the
/// downstream closure, never a produce-time follow.
pub fn produce_shallow_route_facts(
    dep_bodies: &[TypeExpr],
    lens: &dyn RouteFactLens,
) -> ShallowRouteFacts {
    let producer = RouteFactProducer { lens };

    // member_names: the direct-object property NAME enumeration over the
    // contributor bodies (first-contributor precedence via the shared seen
    // set). `Closed(empty)` ⇔ not a direct object with properties — the
    // downstream Omit arm falls back to the plain local closure (the legacy
    // `direct_object_member_names → None` contract). `OpenKeyDomain` is
    // reserved for the genuine L1 open/undecidable class, which this
    // syntactic producer never claims.
    let direct_props = direct_object_properties(dep_bodies);
    let member_names = MemberNamesRoute::Closed(
        direct_props
            .iter()
            .map(|prop| prop.name.clone())
            .collect::<Vec<_>>()
            .into(),
    );

    // member_dependency_edges: per direct property, the type refs of its value
    // annotation (collect order), classified local-first (the BFS pops
    // classify `type_deps` before the import branch). Ref-less properties
    // produce NO edge — membership stays observable through `member_names`.
    let mut member_dependency_edges = Vec::new();
    for prop in &direct_props {
        let mut refs = Vec::new();
        collect_type_refs(&prop.ty, &mut refs);
        if refs.is_empty() {
            continue;
        }
        let depends_on: Vec<RouteDependencyRefFact> = refs
            .into_iter()
            .map(|name| producer.classify_dep_local_first(name))
            .collect();
        member_dependency_edges.push(MemberDependencyEdge {
            member: prop.name.clone(),
            depends_on: depends_on.into(),
        });
    }

    // member_path_seed_edges: the decl's OWN object structure enumerated by
    // prefix — TerminalDeps at every property path (exact-match), a
    // ForwardBoundary at every bare-ref carrier (tail-append), nothing below a
    // complex terminal (fail-closed MISS).
    let mut member_path_seed_edges = Vec::new();
    if let [single_body] = dep_bodies {
        if let Some(name) = bare_ref_name(single_body) {
            member_path_seed_edges.push(MemberPathSeedEdge {
                path: Arc::from(Vec::new().into_boxed_slice()),
                depends_on: MemberPathSeedTarget::ForwardBoundary(
                    producer.classify_dep_local_first(name.to_string()),
                ),
            });
        }
    }
    for prop in &direct_props {
        producer.enumerate_seed_edges(prop, vec![prop.name.clone()], &mut member_path_seed_edges);
    }

    // whole_route_edges: the decl's own body walked once from `Root`. A merged
    // group walks the flattened direct object members of every contributor
    // (forward intersection/parenthesized descent, duplicates preserved,
    // heritage refs dropped) — the legacy merged lookup surface; a single body
    // walks raw.
    let mut whole_route_edges = Vec::new();
    match dep_bodies {
        [] => {}
        [single_body] => {
            producer.walk_whole_route(
                single_body,
                WholeRouteContextFact::Root,
                false,
                &mut whole_route_edges,
            );
        }
        merged => {
            let mut members = Vec::new();
            for body in merged {
                collect_direct_object_members(body, &mut members);
            }
            producer.walk_object_members(members.iter().copied(), false, &mut whole_route_edges);
        }
    }

    ShallowRouteFacts {
        member_names,
        member_path_seed_edges: member_path_seed_edges.into(),
        member_dependency_edges: member_dependency_edges.into(),
        whole_route_edges: whole_route_edges.into(),
    }
}

/// Produce the FLAT content-free deferred key-source fact from ONE
/// declaration's transient contributor bodies — the local, NON-TRANSITIVE
/// normalization half of the key-source producer/dispatch split.
///
/// Normalization rules (single contributor body only): a string literal
/// contributes a `literals` arm; a top-level union flattens its arms; a
/// parenthesized body erases; a zero-argument bare `Ref` becomes an UNRESOLVED
/// [`KeySourceRefFact`] anchored on `lens.own_canonical_id()` (never followed
/// here — cross-decl alias following, cycles, and availability are the session
/// engine's demand-time job; a transitive producer would be a second
/// below-session resolver); any other arm inside a union contributes nothing
/// (the legacy no-keys arm); any other whole-body shape — and a
/// multi-contributor (merged) surface — is [`KeySourceFact::NoFiniteKeys`].
pub fn produce_key_source_fact(bodies: &[TypeExpr], lens: &dyn RouteFactLens) -> KeySourceFact {
    // A merged (multi-contributor) lookup surface is an object — objects
    // enumerate no literal keys (the legacy no-keys arm).
    let [single] = bodies else {
        return KeySourceFact::NoFiniteKeys;
    };
    let mut literals = Vec::new();
    let mut aliases = Vec::new();
    if !collect_key_source_arms(single, lens, &mut literals, &mut aliases) {
        return KeySourceFact::NoFiniteKeys;
    }
    KeySourceFact::LiteralAliasUnion {
        literals: literals.into(),
        aliases: aliases.into(),
    }
}

/// One local normalization step of [`produce_key_source_fact`]. Returns
/// whether `expr` is a conforming key-source shape: a non-conforming TOP-LEVEL
/// body makes the whole fact `NoFiniteKeys`, while a non-conforming UNION arm
/// simply contributes nothing (the caller ignores the return inside a union —
/// the legacy per-arm no-keys behavior).
fn collect_key_source_arms(
    expr: &TypeExpr,
    lens: &dyn RouteFactLens,
    literals: &mut Vec<String>,
    aliases: &mut Vec<KeySourceRefFact>,
) -> bool {
    match expr {
        TypeExpr::Literal(LiteralValue::String(value)) => {
            literals.push(value.clone());
            true
        }
        TypeExpr::Union(types) => {
            for inner in types.iter() {
                // Non-conforming arms contribute nothing; the union itself
                // stays a finite surface.
                let _ = collect_key_source_arms(inner, lens, literals, aliases);
            }
            true
        }
        TypeExpr::Parenthesized(inner) => collect_key_source_arms(inner, lens, literals, aliases),
        TypeExpr::Ref {
            name,
            type_arguments,
        } if type_arguments.is_empty() => {
            aliases.push(KeySourceRefFact {
                anchor: AuthoredAnchor {
                    canonical_id: lens.own_canonical_id(),
                    symbol: Arc::from(name.as_ref()),
                    space: LocatorSymbolSpace::Type,
                },
            });
            true
        }
        _ => false,
    }
}

/// Produce-time key extraction outcome for a `Pick`/`Omit`/indexed key
/// argument (the graph-free tri-state of the legacy cross-decl-capable
/// `extract_string_literal_keys_from_type_expr`).
enum KeyExtraction {
    /// Fully-literal keys, sorted + deduped (possibly empty — empty keeps the
    /// legacy `utility → None` fall-through).
    Literal(Vec<String>),
    /// The whole key expression is a bare LOCAL-symbol alias — the
    /// deferred class (recipe recorded; enumerated downstream).
    DeferredAlias(String),
    /// A local-symbol ref nested inside a composite key expression — the
    /// single-symbol recipe cannot carry it; fail closed (the utility
    /// contributes nothing — never a wrong route).
    Poisoned,
}

struct RouteFactProducer<'l> {
    lens: &'l dyn RouteFactLens,
}

impl RouteFactProducer<'_> {
    /// Classify a dependency NAME with the BFS pop-time precedence: a local
    /// type symbol wins over an import binding, and an unknown name stays a
    /// `Local` shell the downstream `has_type_symbol` gate no-ops.
    fn classify_dep_local_first(&self, name: String) -> RouteDependencyRefFact {
        if self.lens.has_type_symbol(&name) {
            return RouteDependencyRefFact::Local {
                name,
                route: RouteDemand::Whole,
            };
        }
        if let Some(target) = self.lens.resolve_import_route(&name, SymbolSpace::Type) {
            return RouteDependencyRefFact::External(external_ref(
                name,
                &target,
                RouteDemand::Whole,
            ));
        }
        RouteDependencyRefFact::Local {
            name,
            route: RouteDemand::Whole,
        }
    }

    /// Enumerate the member-path seed edges below one direct property.
    fn enumerate_seed_edges(
        &self,
        prop: &ObjectProperty,
        path: Vec<String>,
        out: &mut Vec<MemberPathSeedEdge>,
    ) {
        // Terminal edge: the exact-path seed refs (the legacy
        // `path.len() == 1 → collect_type_refs(prop.ty)` arm). Ref-less
        // terminals produce no edge — observationally identical to the
        // found-with-empty-seeds legacy result.
        let mut refs = Vec::new();
        collect_type_refs(&prop.ty, &mut refs);
        if !refs.is_empty() {
            let depends_on: Vec<RouteDependencyRefFact> = refs
                .into_iter()
                .map(|name| self.classify_dep_local_first(name))
                .collect();
            out.push(MemberPathSeedEdge {
                path: path.clone().into(),
                depends_on: MemberPathSeedTarget::TerminalDeps(depends_on.into()),
            });
        }

        // Forward boundary: a bare (paren-transparent, no-type-args) ref
        // carrier appends the remaining query tail downstream. Nothing
        // enumerates BELOW a carrier (the cross-decl descent is downstream).
        if let Some(name) = bare_ref_name(&prop.ty) {
            out.push(MemberPathSeedEdge {
                path: path.into(),
                depends_on: MemberPathSeedTarget::ForwardBoundary(
                    self.classify_dep_local_first(name.to_string()),
                ),
            });
            return;
        }

        // Object descent: direct properties of the value (object /
        // intersection / parenthesized — reversed intersection arms, seen
        // dedup), one segment deeper. Complex values enumerate nothing —
        // the fail-closed MISS.
        for child in direct_object_properties(std::slice::from_ref(&prop.ty)) {
            let mut child_path = path.clone();
            child_path.push(child.name.clone());
            self.enumerate_seed_edges(child, child_path, out);
        }
    }

    /// The site context an edge is STORED with: an emitting-only carrier
    /// position (`Partial`-family type argument) normalizes `Root` to
    /// `CallableParam` — behaviorally identical under an emitting follow
    /// (every legacy gate treats `Root` and `CallableParam` jointly) and
    /// correctly dropped under a `LeafProperty` follow (the family gate
    /// requires a non-leaf context).
    fn stored_context(
        &self,
        context: WholeRouteContextFact,
        emitting_only_guard: bool,
    ) -> WholeRouteContextFact {
        if emitting_only_guard && matches!(context, WholeRouteContextFact::Root) {
            WholeRouteContextFact::CallableParam
        } else {
            context
        }
    }

    /// The whole-route walk over the decl's own body — the graph-free
    /// reproduction of the legacy `collect_whole_route_refs`, emitting edges
    /// in the same depth-first order the legacy walk emitted/visited.
    /// `guard` = the path crossed an emitting-only carrier (`Partial`-family
    /// argument).
    fn walk_whole_route(
        &self,
        expr: &TypeExpr,
        context: WholeRouteContextFact,
        guard: bool,
        out: &mut Vec<WholeRouteEdgeFact>,
    ) {
        match expr {
            TypeExpr::Parenthesized(inner) | TypeExpr::KeyOf(inner) | TypeExpr::Rest(inner) => {
                self.walk_whole_route(inner, context, guard, out);
            }
            TypeExpr::Union(types) | TypeExpr::Intersection(types) => {
                for inner in types.iter() {
                    self.walk_whole_route(inner, context, guard, out);
                }
            }
            TypeExpr::Array { element, .. } => {
                self.walk_whole_route(element, context, guard, out);
            }
            TypeExpr::Tuple { elements, .. } => {
                for element in elements.iter() {
                    self.walk_whole_route(&element.ty, context, guard, out);
                }
            }
            TypeExpr::Object(obj) => {
                if matches!(context, WholeRouteContextFact::LeafProperty) {
                    return;
                }
                self.walk_object_members(&obj.properties, guard, out);
            }
            TypeExpr::Function(func) | TypeExpr::ConstructorType(func) => {
                if matches!(context, WholeRouteContextFact::LeafProperty) {
                    return;
                }
                self.walk_function_refs(func, guard, out);
            }
            TypeExpr::Ref {
                name,
                type_arguments,
            } => {
                let symbol_name = name.as_ref();
                if let Some(target) = self
                    .lens
                    .resolve_import_route(symbol_name, SymbolSpace::Type)
                {
                    // Import: gated emit (Root/CallableParam), NEVER recurses
                    // type arguments.
                    if matches!(
                        context,
                        WholeRouteContextFact::Root | WholeRouteContextFact::CallableParam
                    ) {
                        out.push(WholeRouteEdgeFact::External {
                            external_ref: external_ref(
                                symbol_name.to_string(),
                                &target,
                                RouteDemand::Whole,
                            ),
                            context: self.stored_context(context, guard),
                        });
                    }
                    return;
                }

                if matches!(symbol_name, "Pick" | "Omit") && type_arguments.len() == 2 {
                    self.emit_utility_edges(symbol_name, type_arguments, context, guard, out);
                    return;
                }

                if matches!(
                    symbol_name,
                    "Partial" | "Required" | "Readonly" | "NonNullable"
                ) && !type_arguments.is_empty()
                    && !matches!(context, WholeRouteContextFact::LeafProperty)
                {
                    // Emitting-only carrier: descend the payload with the SAME
                    // context, under the guard (a leaf-context walk never
                    // descends here — the produced edges must drop under a
                    // LeafProperty follow).
                    self.walk_whole_route(&type_arguments[0], context, true, out);
                    return;
                }

                // Local / unknown ref: a deferred follow edge. Unknown names
                // (type parameters, free names) stay edges — the downstream
                // `has_type_symbol` gate no-ops them charging no budget,
                // exactly like the legacy walk's pre-follow gate. Type
                // arguments are NOT walked (legacy parity).
                out.push(WholeRouteEdgeFact::Local {
                    name: symbol_name.to_string(),
                    route: RouteDemand::Whole,
                    context: self.stored_context(context, guard),
                });
            }
            TypeExpr::IndexedAccess { .. } => {
                self.emit_indexed_access_edges(expr, context, guard, out);
            }
            TypeExpr::Conditional {
                check,
                extends,
                true_type,
                false_type,
            } => {
                for inner in [check, extends, true_type, false_type] {
                    self.walk_whole_route(inner, context, guard, out);
                }
            }
            TypeExpr::Mapped {
                source,
                value,
                name_type,
                ..
            } => {
                self.walk_whole_route(source, context, guard, out);
                self.walk_whole_route(value, context, guard, out);
                if let Some(name_type) = name_type.as_deref() {
                    self.walk_whole_route(name_type, context, guard, out);
                }
            }
            TypeExpr::TemplateLiteral { expressions, .. } => {
                for inner in expressions.iter() {
                    self.walk_whole_route(inner, context, guard, out);
                }
            }
            TypeExpr::TypeOf(value_ref) => {
                if let Some(root) = value_ref.path.first() {
                    if let Some(target) = self
                        .lens
                        .resolve_import_route(root.as_str(), SymbolSpace::Value)
                    {
                        if matches!(
                            context,
                            WholeRouteContextFact::Root | WholeRouteContextFact::CallableParam
                        ) {
                            out.push(WholeRouteEdgeFact::External {
                                external_ref: external_ref(
                                    root.clone(),
                                    &target,
                                    RouteDemand::Whole,
                                ),
                                context: self.stored_context(context, guard),
                            });
                        }
                    }
                }
            }
            // The `specifier`/`qualifier` of an import type are leaf module
            // strings; only the nested type-argument exprs walk (legacy
            // parity — the cross-file dependency EDGE for the specifier
            // remains the deferred read-set follow-up on the legacy walk).
            TypeExpr::ImportType { type_arguments, .. } => {
                for argument in type_arguments.iter() {
                    self.walk_whole_route(argument, context, guard, out);
                }
            }
            TypeExpr::Primitive(_)
            | TypeExpr::Literal(_)
            | TypeExpr::TypeParameter(_)
            | TypeExpr::Infer { .. }
            | TypeExpr::RecursiveRef { .. }
            | TypeExpr::SyntheticSlotBinding(_)
            | TypeExpr::Unknown { .. } => {}
        }
    }

    /// The object-member half of the walk (also the entry for a merged
    /// group's flattened member surface): property / index-signature values
    /// walk under `LeafProperty`; call/construct/method signatures walk their
    /// callable positions.
    fn walk_object_members<'m>(
        &self,
        members: impl IntoIterator<Item = &'m ObjectMember>,
        guard: bool,
        out: &mut Vec<WholeRouteEdgeFact>,
    ) {
        for member in members {
            match member {
                ObjectMember::Property(prop) => {
                    self.walk_whole_route(
                        &prop.ty,
                        WholeRouteContextFact::LeafProperty,
                        guard,
                        out,
                    );
                }
                ObjectMember::IndexSignature(sig) => {
                    self.walk_whole_route(
                        &sig.value_type,
                        WholeRouteContextFact::LeafProperty,
                        guard,
                        out,
                    );
                }
                ObjectMember::CallSignature(func) | ObjectMember::ConstructSignature(func) => {
                    self.walk_function_refs(func, guard, out);
                }
                ObjectMember::Method(method) => {
                    self.walk_function_refs(&method.function, guard, out);
                }
            }
        }
    }

    /// Callable positions: parameters + function type-param constraint/default
    /// walk as `CallableParam`; the return type NEVER walks (legacy parity).
    fn walk_function_refs(
        &self,
        func: &verter_type_expr::FunctionExpr,
        guard: bool,
        out: &mut Vec<WholeRouteEdgeFact>,
    ) {
        for param in &func.parameters {
            self.walk_whole_route(&param.ty, WholeRouteContextFact::CallableParam, guard, out);
        }
        for type_param in &func.type_parameters {
            if let Some(constraint) = type_param.constraint.as_deref() {
                self.walk_whole_route(constraint, WholeRouteContextFact::CallableParam, guard, out);
            }
            if let Some(default) = type_param.default.as_deref() {
                self.walk_whole_route(default, WholeRouteContextFact::CallableParam, guard, out);
            }
        }
    }

    /// A `Pick`/`Omit` utility site: literal keys route immediately through
    /// the routed-base classification; a bare local key alias records the
    /// deferred edge; empty keys fall through to the userland local-decl
    /// follow (when one exists); a poisoned key expression contributes
    /// nothing (fail closed).
    fn emit_utility_edges(
        &self,
        utility_name: &str,
        type_arguments: &[TypeExpr],
        context: WholeRouteContextFact,
        guard: bool,
        out: &mut Vec<WholeRouteEdgeFact>,
    ) {
        match self.extract_literal_keys(&type_arguments[1]) {
            KeyExtraction::Literal(keys) if !keys.is_empty() => {
                let route = if utility_name == "Pick" {
                    RouteDemand::pick(keys)
                } else {
                    RouteDemand::omit(keys)
                };
                self.emit_routed_base(&type_arguments[0], route, context, guard, out);
            }
            KeyExtraction::Literal(_) => {
                // Empty keys: the legacy `utility → None` fall-through — the
                // name is never `Partial`-family, so the ONLY remaining arm is
                // the userland local decl named `Pick`/`Omit`.
                if self.lens.has_type_symbol(utility_name) {
                    out.push(WholeRouteEdgeFact::Local {
                        name: utility_name.to_string(),
                        route: RouteDemand::Whole,
                        context: self.stored_context(context, guard),
                    });
                }
            }
            KeyExtraction::DeferredAlias(alias) => {
                let kind = if utility_name == "Pick" {
                    DeferredKeyUtilityKind::Pick
                } else {
                    DeferredKeyUtilityKind::Omit
                };
                out.push(WholeRouteEdgeFact::DeferredKeyUtility(
                    DeferredKeyUtilityEdge {
                        kind,
                        base: self.classify_routed_base(&type_arguments[0]),
                        base_path: Arc::from(Vec::new().into_boxed_slice()),
                        key_source: self.key_source_recipe(&alias),
                        empty_keys_fallback: self
                            .lens
                            .has_type_symbol(utility_name)
                            .then(|| utility_name.to_string()),
                        context: self.stored_context(context, guard),
                    },
                ));
            }
            KeyExtraction::Poisoned => {}
        }
    }

    /// An indexed-access site: extract the (base, route) exactly like the
    /// legacy `extract_indexed_access_route` for literal keys; a bare local
    /// alias in the OUTERMOST index position (with a fully-literal inner
    /// chain) records the deferred edge; anything else contributes nothing.
    fn emit_indexed_access_edges(
        &self,
        expr: &TypeExpr,
        context: WholeRouteContextFact,
        guard: bool,
        out: &mut Vec<WholeRouteEdgeFact>,
    ) {
        let TypeExpr::IndexedAccess { object, index } = expr else {
            return;
        };
        match self.extract_literal_keys(index) {
            KeyExtraction::Literal(keys) if !keys.is_empty() => {
                let Some((base_expr, path)) = self.extract_indexed_access_base(object.as_ref())
                else {
                    return;
                };
                let route = if keys.len() == 1 {
                    let mut full = path;
                    full.push(keys.into_iter().next().expect("len checked"));
                    RouteDemand::member_path(full)
                } else if path.is_empty() {
                    RouteDemand::pick(keys)
                } else {
                    return;
                };
                self.emit_routed_base(base_expr, route, context, guard, out);
            }
            KeyExtraction::Literal(_) => {}
            KeyExtraction::DeferredAlias(alias) => {
                let Some((base_expr, path)) = self.extract_indexed_access_base(object.as_ref())
                else {
                    return;
                };
                out.push(WholeRouteEdgeFact::DeferredKeyUtility(
                    DeferredKeyUtilityEdge {
                        kind: DeferredKeyUtilityKind::IndexedAccess,
                        base: self.classify_routed_base(base_expr),
                        base_path: path.into(),
                        key_source: self.key_source_recipe(&alias),
                        empty_keys_fallback: None,
                        context: self.stored_context(context, guard),
                    },
                ));
            }
            KeyExtraction::Poisoned => {}
        }
    }

    /// The deferred-key recipe: the deferred key-source alias as an existing
    /// `KeyDomainFact::FollowSlot` symbol locator (recipe-only — enumeration
    /// re-minted downstream through the key-domain machinery).
    fn key_source_recipe(&self, alias: &str) -> KeyDomainFact {
        KeyDomainFact::FollowSlot(SymbolBodyLocator {
            anchor: AuthoredAnchor {
                canonical_id: self.lens.own_canonical_id(),
                symbol: Arc::from(alias),
                space: LocatorSymbolSpace::Type,
            },
        })
    }

    /// The legacy `follow_routed_expr` head applied at produce time: a bare
    /// ref base becomes an edge carrying the routed demand (import ⇒ direct
    /// external emit — context-independent; local/unknown ⇒ deferred local
    /// follow); every other base shape contributes nothing.
    fn emit_routed_base(
        &self,
        base: &TypeExpr,
        route: RouteDemand,
        context: WholeRouteContextFact,
        guard: bool,
        out: &mut Vec<WholeRouteEdgeFact>,
    ) {
        let Some(name) = bare_ref_name(base) else {
            return;
        };
        if let Some(target) = self.lens.resolve_import_route(name, SymbolSpace::Type) {
            out.push(WholeRouteEdgeFact::External {
                external_ref: external_ref(name.to_string(), &target, route),
                context: self.stored_context(context, guard),
            });
            return;
        }
        out.push(WholeRouteEdgeFact::Local {
            name: name.to_string(),
            route,
            context: self.stored_context(context, guard),
        });
    }

    /// Classify a deferred edge's base shell (`RouteDemand::Whole` placeholder
    /// — the downstream closure substitutes the resolved route).
    fn classify_routed_base(&self, base: &TypeExpr) -> Option<RouteDependencyRefFact> {
        let name = bare_ref_name(base)?;
        if let Some(target) = self.lens.resolve_import_route(name, SymbolSpace::Type) {
            return Some(RouteDependencyRefFact::External(external_ref(
                name.to_string(),
                &target,
                RouteDemand::Whole,
            )));
        }
        Some(RouteDependencyRefFact::Local {
            name: name.to_string(),
            route: RouteDemand::Whole,
        })
    }

    /// Produce-time literal key extraction (the graph-free tri-state of the
    /// legacy cross-decl extractor): string literals and literal unions
    /// resolve; a bare produce-time-local alias defers (the deferred-key class); a local alias
    /// nested in a composite poisons (fail closed); unknown/import names and
    /// every other shape contribute nothing — exactly the legacy
    /// `has_type_symbol`-gated fall-through.
    fn extract_literal_keys(&self, expr: &TypeExpr) -> KeyExtraction {
        match expr {
            // The legacy extractor's Ref arm gates on `has_type_symbol` ALONE
            // (a local type header shadows an import binding here), so the
            // produce-time deferral mirrors that gate exactly.
            TypeExpr::Ref {
                name,
                type_arguments,
            } if type_arguments.is_empty() && self.lens.has_type_symbol(name.as_ref()) => {
                KeyExtraction::DeferredAlias(name.to_string())
            }
            TypeExpr::Parenthesized(inner) => self.extract_literal_keys(inner),
            _ => match self.extract_literal_keys_inner(expr) {
                Some(mut keys) => {
                    keys.sort();
                    keys.dedup();
                    KeyExtraction::Literal(keys)
                }
                None => KeyExtraction::Poisoned,
            },
        }
    }

    /// Literal-only extraction below the top level: `None` = poisoned (a
    /// local-symbol ref inside a composite).
    fn extract_literal_keys_inner(&self, expr: &TypeExpr) -> Option<Vec<String>> {
        match expr {
            TypeExpr::Literal(verter_type_expr::LiteralValue::String(value)) => {
                Some(vec![value.clone()])
            }
            TypeExpr::Union(types) => {
                let mut keys = Vec::new();
                for inner in types.iter() {
                    keys.extend(self.extract_literal_keys_inner(inner)?);
                }
                Some(keys)
            }
            TypeExpr::Parenthesized(inner) => self.extract_literal_keys_inner(inner),
            TypeExpr::Ref {
                name,
                type_arguments,
            } if type_arguments.is_empty() && self.lens.has_type_symbol(name.as_ref()) => {
                // A produce-time-local alias nested in a composite key
                // expression: the legacy walk resolves it cross-decl; the
                // single-symbol recipe cannot carry it — poison (fail closed).
                None
            }
            // Unknown / import / generic refs and every other shape contribute
            // no keys (the legacy `_ => Vec::new()` + `has_type_symbol` gate).
            _ => Some(Vec::new()),
        }
    }

    /// The legacy `extract_indexed_access_base`: peel parens; a nested indexed
    /// access must itself resolve to a fully-LITERAL single-key member path
    /// (a deferred or multi-key inner level fails the whole chain — fail
    /// closed); anything else is the base with an empty path.
    fn extract_indexed_access_base<'e>(
        &self,
        expr: &'e TypeExpr,
    ) -> Option<(&'e TypeExpr, Vec<String>)> {
        match expr {
            TypeExpr::Parenthesized(inner) => self.extract_indexed_access_base(inner),
            TypeExpr::IndexedAccess { object, index } => {
                let KeyExtraction::Literal(keys) = self.extract_literal_keys(index) else {
                    return None;
                };
                if keys.is_empty() {
                    return None;
                }
                let (base_expr, mut path) = self.extract_indexed_access_base(object.as_ref())?;
                if keys.len() == 1 {
                    path.push(keys.into_iter().next().expect("len checked"));
                    Some((base_expr, path))
                } else {
                    // A multi-key inner level would be a Pick route — the
                    // legacy base extraction accepts member paths only.
                    None
                }
            }
            _ => Some((expr, Vec::new())),
        }
    }
}

/// Build the 5-field external route ref from a lens target.
fn external_ref(
    local_name: String,
    target: &ImportRouteTarget,
    route: RouteDemand,
) -> ExternalRouteRefFact {
    ExternalRouteRefFact {
        local_name,
        source_specifier: target.source_specifier.to_string(),
        imported_name: target.imported_name.to_string(),
        canonical_id: target.canonical_id.clone(),
        route,
    }
}

/// The bare-ref carrier probe: a (paren-transparent) `Ref` with NO type
/// arguments.
fn bare_ref_name(expr: &TypeExpr) -> Option<&str> {
    match expr {
        TypeExpr::Parenthesized(inner) => bare_ref_name(inner),
        TypeExpr::Ref {
            name,
            type_arguments,
        } if type_arguments.is_empty() => Some(name.as_ref()),
        _ => None,
    }
}

/// Direct object PROPERTIES over an ordered contributor body list: per body a
/// reversed-intersection / parenthesized descent, with ONE property-level
/// `seen` set spanning every contributor (first-contributor precedence) —
/// the legacy merged lookup surface for property reads.
fn direct_object_properties(bodies: &[TypeExpr]) -> Vec<&ObjectProperty> {
    let mut out = Vec::new();
    let mut seen = rustc_hash::FxHashSet::default();
    for body in bodies {
        collect_direct_object_properties(body, &mut out, &mut seen);
    }
    out
}

fn collect_direct_object_properties<'a>(
    body: &'a TypeExpr,
    out: &mut Vec<&'a ObjectProperty>,
    seen: &mut rustc_hash::FxHashSet<String>,
) {
    match body {
        TypeExpr::Object(obj) => {
            for member in &obj.properties {
                if let ObjectMember::Property(prop) = member {
                    if seen.insert(prop.name.clone()) {
                        out.push(prop);
                    }
                }
            }
        }
        TypeExpr::Intersection(parts) => {
            for part in parts.iter().rev() {
                collect_direct_object_properties(part, out, seen);
            }
        }
        TypeExpr::Parenthesized(inner) => {
            collect_direct_object_properties(inner, out, seen);
        }
        _ => {}
    }
}

/// Direct object MEMBERS (all five member kinds, duplicates preserved, FORWARD
/// intersection descent) — the merged whole-route walk surface (heritage refs
/// from `extends`/`implements` intersections carry no direct member and drop).
fn collect_direct_object_members<'a>(body: &'a TypeExpr, out: &mut Vec<&'a ObjectMember>) {
    match body {
        TypeExpr::Object(object) => out.extend(object.properties.iter()),
        TypeExpr::Intersection(parts) => {
            for part in parts.iter() {
                collect_direct_object_members(part, out);
            }
        }
        TypeExpr::Parenthesized(inner) => collect_direct_object_members(inner, out),
        _ => {}
    }
}

#[cfg(test)]
#[path = "route_facts_tests.rs"]
mod route_facts_tests;

/// Collect all named type references from a `TypeExpr`, non-recursively (only
/// direct references, not transitive) — the producer-side copy of the legacy
/// seed/member ref enumeration, in identical traversal order.
pub(crate) fn collect_type_refs(expr: &TypeExpr, out: &mut Vec<String>) {
    match expr {
        TypeExpr::Ref {
            name,
            type_arguments,
        } => {
            out.push(name.to_string());
            for arg in type_arguments.iter() {
                collect_type_refs(arg, out);
            }
        }
        TypeExpr::Union(members) | TypeExpr::Intersection(members) => {
            for m in members.iter() {
                collect_type_refs(m, out);
            }
        }
        TypeExpr::Array { element, .. } => collect_type_refs(element, out),
        TypeExpr::Object(obj) => {
            for member in &obj.properties {
                if let ObjectMember::Property(prop) = member {
                    collect_type_refs(&prop.ty, out);
                }
            }
        }
        TypeExpr::Tuple { elements, .. } => {
            for el in elements.iter() {
                collect_type_refs(&el.ty, out);
            }
        }
        TypeExpr::IndexedAccess { object, index } => {
            collect_type_refs(object, out);
            collect_type_refs(index, out);
        }
        TypeExpr::Conditional {
            check,
            extends,
            true_type,
            false_type,
        } => {
            collect_type_refs(check, out);
            collect_type_refs(extends, out);
            collect_type_refs(true_type, out);
            collect_type_refs(false_type, out);
        }
        TypeExpr::Function(func) | TypeExpr::ConstructorType(func) => {
            for param in &func.parameters {
                collect_type_refs(&param.ty, out);
            }
            if let Some(ref ret) = func.return_type {
                collect_type_refs(ret, out);
            }
        }
        TypeExpr::Mapped { source, value, .. } => {
            collect_type_refs(source, out);
            collect_type_refs(value, out);
        }
        TypeExpr::KeyOf(inner) | TypeExpr::Rest(inner) | TypeExpr::Parenthesized(inner) => {
            collect_type_refs(inner, out);
        }
        TypeExpr::ImportType { type_arguments, .. } => {
            for arg in type_arguments.iter() {
                collect_type_refs(arg, out);
            }
        }
        TypeExpr::TypeOf { .. }
        | TypeExpr::TypeParameter(_)
        | TypeExpr::Primitive(_)
        | TypeExpr::Literal(_)
        | TypeExpr::TemplateLiteral { .. }
        | TypeExpr::Unknown { .. }
        | TypeExpr::RecursiveRef { .. }
        | TypeExpr::SyntheticSlotBinding(_)
        | TypeExpr::Infer { .. } => {}
    }
}
