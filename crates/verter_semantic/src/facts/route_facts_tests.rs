//! Byte-parity oracle for the route-fact producer/closure split: the GOLDEN module is
//! an INDEPENDENT port of the legacy `ShallowFileState` route-closure walkers
//! (whole-route crux verbatim from the pre-slot tree, member edges from the
//! current ordered-contributor extractor) over `&TypeExpr` + a mock
//! symbol/import table; the NEW pipeline is `produce_shallow_route_facts` +
//! `route_closure_over_facts` over the same fixtures. The byte-parity target
//! is `unresolved_external` (5 fields + route, deduped/merged/ordered) plus
//! `status` — exactly what the frontier consumes.
//!
//! Golden expectations for the load-bearing cases are HAND-BUILT literals
//! (never the new pipeline's output), so the goldens are non-tautological;
//! discrimination cases mutate the produced facts (drop a context tag, drop a
//! path segment, flip a route) and assert the closure DIVERGES from golden.

use std::sync::Arc;

use rustc_hash::{FxHashMap, FxHashSet};

use verter_type_expr::facts::{
    ExternalRouteRefFact, KeySourceFact, ShallowRouteFacts, WholeRouteEdgeFact,
};
use verter_type_expr::locators::LocatorSymbolSpace;
use verter_type_expr::{
    FunctionExpr, FunctionParam, LiteralValue, MethodSignature, ObjectExpr, ObjectMember,
    ObjectProperty, PrimitiveName, RouteDemand, TypeExpr,
};

use super::{
    produce_key_source_fact, produce_shallow_route_facts, ImportRouteTarget, RouteFactLens,
};
use crate::facts::route_closure::{
    local_closure_over_facts, route_closure_over_facts, ClassifiedRouteDeps, FactClosureStatus,
    KeySourceLookup, RouteClosureProvider,
};
use crate::facts::SymbolSpace;

// ===========================================================================
// Shared fixture state
// ===========================================================================

/// One mock import binding.
#[derive(Debug, Clone)]
struct MockImport {
    specifier: String,
    imported_name: String,
    canonical_id: Option<String>,
}

/// The mock file: ordered decls (name → contributor bodies) + import table.
/// Implements the GOLDEN walk's state reads directly, the NEW producer's lens,
/// and the NEW closure's provider — one fixture, two independent pipelines.
#[derive(Debug, Default)]
struct MockFile {
    decls: Vec<(String, Vec<TypeExpr>)>,
    imports: FxHashMap<String, MockImport>,
}

impl MockFile {
    fn decl(mut self, name: &str, body: TypeExpr) -> Self {
        self.decls.push((name.to_string(), vec![body]));
        self
    }

    fn merged_decl(mut self, name: &str, bodies: Vec<TypeExpr>) -> Self {
        self.decls.push((name.to_string(), bodies));
        self
    }

    fn import(mut self, local: &str, specifier: &str, imported: &str, canonical: &str) -> Self {
        self.imports.insert(
            local.to_string(),
            MockImport {
                specifier: specifier.to_string(),
                imported_name: imported.to_string(),
                canonical_id: (!canonical.is_empty()).then(|| canonical.to_string()),
            },
        );
        self
    }

    fn bodies(&self, name: &str) -> Option<&[TypeExpr]> {
        self.decls
            .iter()
            .find(|(decl, _)| decl == name)
            .map(|(_, bodies)| bodies.as_slice())
    }

    fn has_decl(&self, name: &str) -> bool {
        self.decls.iter().any(|(decl, _)| decl == name)
    }

    fn external_shell(&self, local: &str, route: RouteDemand) -> Option<ExternalRouteRefFact> {
        let import = self.imports.get(local)?;
        Some(ExternalRouteRefFact {
            local_name: local.to_string(),
            source_specifier: import.specifier.clone(),
            imported_name: import.imported_name.clone(),
            canonical_id: import.canonical_id.as_deref().map(Arc::from),
            route,
        })
    }

    /// The FIXTURE dependency-edge classification shared by both pipelines
    /// (the session's `type_deps` survives the split unchanged, so it is an
    /// INPUT here, not part of the algorithm under test): dependency names are
    /// the direct type refs of the contributor bodies, split local/external
    /// with the session's ordering.
    fn classified_deps_fixture(&self, name: &str) -> Option<ClassifiedRouteDeps> {
        let bodies = self.bodies(name)?;
        let mut dep_names = Vec::new();
        for body in bodies {
            golden::collect_type_refs(body, &mut dep_names);
        }

        let mut local_set = FxHashSet::default();
        for reference in &dep_names {
            let root = reference.split('.').next().unwrap_or(reference.as_str());
            if self.imports.contains_key(root) {
                continue;
            }
            if root != name && self.has_decl(root) {
                local_set.insert(root.to_string());
            }
        }
        let mut local_deps: Vec<String> = local_set.into_iter().collect();
        local_deps.sort();

        let mut external_deps = Vec::new();
        let mut seen_external: FxHashSet<(String, String)> = FxHashSet::default();
        for dep_name in &dep_names {
            let root = dep_name.split('.').next().unwrap_or(dep_name.as_str());
            let Some(import) = self.imports.get(root) else {
                continue;
            };
            if !seen_external.insert((import.specifier.clone(), import.imported_name.clone())) {
                continue;
            }
            external_deps.push(ExternalRouteRefFact {
                local_name: dep_name.clone(),
                source_specifier: import.specifier.clone(),
                imported_name: import.imported_name.clone(),
                canonical_id: import.canonical_id.as_deref().map(Arc::from),
                route: RouteDemand::Whole,
            });
        }
        external_deps.sort_by(|left, right| {
            left.local_name
                .cmp(&right.local_name)
                .then_with(|| left.source_specifier.cmp(&right.source_specifier))
                .then_with(|| left.imported_name.cmp(&right.imported_name))
        });

        Some(ClassifiedRouteDeps {
            local_deps,
            external_deps,
        })
    }
}

impl RouteFactLens for MockFile {
    fn resolve_import_route(&self, local: &str, _space: SymbolSpace) -> Option<ImportRouteTarget> {
        let import = self.imports.get(local)?;
        Some(ImportRouteTarget {
            source_specifier: Arc::from(import.specifier.as_str()),
            imported_name: Arc::from(import.imported_name.as_str()),
            canonical_id: import.canonical_id.as_deref().map(Arc::from),
        })
    }
    fn has_type_symbol(&self, name: &str) -> bool {
        self.has_decl(name)
    }
    fn own_canonical_id(&self) -> Arc<str> {
        Arc::from("mock:file")
    }
    fn own_top_level_owner(&self) -> verter_type_expr::TopLevelOwnerId {
        verter_type_expr::TopLevelOwnerId::ordinary_file()
    }
}

impl RouteClosureProvider for MockFile {
    fn has_type_symbol(&self, name: &str) -> bool {
        self.has_decl(name)
    }
    fn route_facts(&self, name: &str) -> Option<ShallowRouteFacts> {
        let bodies = self.bodies(name)?;
        Some(produce_shallow_route_facts(bodies, self))
    }
    fn classified_deps(&self, name: &str) -> Option<ClassifiedRouteDeps> {
        self.classified_deps_fixture(name)
    }
    fn is_import_local(&self, name: &str) -> bool {
        self.imports.contains_key(name)
    }
    fn import_route_target(&self, name: &str) -> Option<ExternalRouteRefFact> {
        self.external_shell(name, RouteDemand::Whole)
    }
    fn key_source_lookup(&self, name: &str) -> KeySourceLookup {
        fixture_key_source_lookup(self, name)
    }
}

/// The fixture's ENGINE-side deferred key-source alias follow — the same
/// produced-fact fold the session dispatch runs: one content-free
/// `produce_key_source_fact` mint per visited decl, alias refs followed
/// through a visited set, an unavailable hop poisons the whole enumeration,
/// and only a COMPLETED set is sorted/deduped. The closure core under test
/// receives only the tri-state outcome — never a body.
fn fixture_key_source_lookup(file: &MockFile, name: &str) -> KeySourceLookup {
    if !file.has_decl(name) {
        return KeySourceLookup::MissingTypeSymbol;
    }
    let mut visited = FxHashSet::default();
    let mut keys: Vec<String> = Vec::new();
    let mut pending = vec![name.to_string()];
    visited.insert(name.to_string());
    while let Some(current) = pending.pop() {
        let Some(bodies) = file.bodies(&current) else {
            return KeySourceLookup::Unavailable;
        };
        match produce_key_source_fact(bodies, file) {
            KeySourceFact::NoFiniteKeys => {}
            KeySourceFact::LiteralAliasUnion { literals, aliases } => {
                keys.extend(literals.iter().cloned());
                for alias in aliases.iter() {
                    let symbol = alias.anchor.symbol.as_ref();
                    // A ref that names no fixture decl enumerates to zero
                    // keys (the legacy guard arm).
                    if !file.has_decl(symbol) {
                        continue;
                    }
                    if visited.insert(symbol.to_string()) {
                        pending.push(symbol.to_string());
                    }
                }
            }
        }
    }
    keys.sort();
    keys.dedup();
    KeySourceLookup::Ready(keys)
}

/// A provider wrapper that rewrites one decl's produced facts — the
/// fact-mutation knob for the discrimination battery.
struct MutatedProvider<'m> {
    inner: &'m MockFile,
    target: &'m str,
    mutate: &'m dyn Fn(ShallowRouteFacts) -> ShallowRouteFacts,
}

impl RouteClosureProvider for MutatedProvider<'_> {
    fn has_type_symbol(&self, name: &str) -> bool {
        self.inner.has_decl(name)
    }
    fn route_facts(&self, name: &str) -> Option<ShallowRouteFacts> {
        let facts = RouteClosureProvider::route_facts(self.inner, name)?;
        Some(if name == self.target {
            (self.mutate)(facts)
        } else {
            facts
        })
    }
    fn classified_deps(&self, name: &str) -> Option<ClassifiedRouteDeps> {
        self.inner.classified_deps_fixture(name)
    }
    fn is_import_local(&self, name: &str) -> bool {
        self.inner.imports.contains_key(name)
    }
    fn import_route_target(&self, name: &str) -> Option<ExternalRouteRefFact> {
        self.inner.external_shell(name, RouteDemand::Whole)
    }
    fn key_source_lookup(&self, name: &str) -> KeySourceLookup {
        fixture_key_source_lookup(self.inner, name)
    }
}

/// A provider modeling a genuinely UNAVAILABLE deferred key-source hand-off
/// (the session's broken-lease / decl-demand-miss arm): `key_source_lookup`
/// is `Unavailable` for every alias — all other reads answer like the full
/// fixture. The fail-closed CONTROL double.
struct UnavailableKeySourceProvider<'m> {
    inner: &'m MockFile,
}

impl RouteClosureProvider for UnavailableKeySourceProvider<'_> {
    fn has_type_symbol(&self, name: &str) -> bool {
        self.inner.has_decl(name)
    }
    fn route_facts(&self, name: &str) -> Option<ShallowRouteFacts> {
        RouteClosureProvider::route_facts(self.inner, name)
    }
    fn classified_deps(&self, name: &str) -> Option<ClassifiedRouteDeps> {
        self.inner.classified_deps_fixture(name)
    }
    fn is_import_local(&self, name: &str) -> bool {
        self.inner.imports.contains_key(name)
    }
    fn import_route_target(&self, name: &str) -> Option<ExternalRouteRefFact> {
        self.inner.external_shell(name, RouteDemand::Whole)
    }
    fn key_source_lookup(&self, _name: &str) -> KeySourceLookup {
        KeySourceLookup::Unavailable
    }
}

// ===========================================================================
// GOLDEN — the independent legacy port
// ===========================================================================

mod golden {
    use super::*;

    /// Golden closure status (1:1 with the legacy `LocalClosureStatus`).
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub enum GoldenStatus {
        Resolved,
        ResolvedWithExternalDeps,
        MissingLocalSymbol { name: String },
        BudgetExceeded,
    }

    #[derive(Debug, Clone)]
    pub struct GoldenResult {
        pub status: GoldenStatus,
        pub unresolved_external: Vec<ExternalRouteRefFact>,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Ctx {
        Root,
        CallableParam,
        LeafProperty,
    }

    /// The golden's OWN external accumulator with its OWN inline route-merge
    /// port (independent of the production `merge_route_demands`).
    #[derive(Default)]
    struct Acc {
        index: FxHashMap<(String, String), usize>,
        refs: Vec<ExternalRouteRefFact>,
    }

    impl Acc {
        fn add(&mut self, ext: ExternalRouteRefFact) {
            match self
                .index
                .entry((ext.source_specifier.clone(), ext.imported_name.clone()))
            {
                std::collections::hash_map::Entry::Occupied(entry) => {
                    let existing = &mut self.refs[*entry.get()];
                    existing.route = golden_merge(&existing.route, &ext.route);
                }
                std::collections::hash_map::Entry::Vacant(entry) => {
                    entry.insert(self.refs.len());
                    self.refs.push(ext);
                }
            }
        }
        fn into_vec(self) -> Vec<ExternalRouteRefFact> {
            self.refs
        }
        fn is_empty(&self) -> bool {
            self.refs.is_empty()
        }
    }

    /// Inline port of the legacy conservative route merge.
    fn golden_merge(a: &RouteDemand, b: &RouteDemand) -> RouteDemand {
        if a == b {
            return a.clone();
        }
        match (a, b) {
            (RouteDemand::Whole, _) | (_, RouteDemand::Whole) => RouteDemand::Whole,
            (RouteDemand::MemberPath(pa), RouteDemand::MemberPath(pb)) => {
                let common: Vec<String> = pa
                    .iter()
                    .zip(pb.iter())
                    .take_while(|(l, r)| l == r)
                    .map(|(s, _)| s.clone())
                    .collect();
                if !common.is_empty() {
                    RouteDemand::member_path(common)
                } else {
                    let mut members = Vec::new();
                    members.extend(pa.first().cloned());
                    members.extend(pb.first().cloned());
                    if members.is_empty() {
                        RouteDemand::Whole
                    } else {
                        RouteDemand::pick(members)
                    }
                }
            }
            (RouteDemand::MemberPath(p), RouteDemand::Pick(ps))
            | (RouteDemand::Pick(ps), RouteDemand::MemberPath(p)) => {
                let mut merged: Vec<String> = ps.as_slice().to_vec();
                merged.extend(p.first().cloned());
                if merged.is_empty() {
                    RouteDemand::Whole
                } else {
                    RouteDemand::pick(merged)
                }
            }
            (RouteDemand::Pick(a), RouteDemand::Pick(b)) => {
                let mut merged: Vec<String> = a.as_slice().to_vec();
                merged.extend(b.iter().cloned());
                RouteDemand::pick(merged)
            }
            (RouteDemand::Omit(a_omit), RouteDemand::MemberPath(p)) => {
                if p.first().is_some_and(|first| !a_omit.contains(first)) {
                    RouteDemand::Omit(a_omit.clone())
                } else {
                    RouteDemand::Whole
                }
            }
            (RouteDemand::MemberPath(p), RouteDemand::Omit(b_omit)) => {
                if p.first().is_some_and(|first| !b_omit.contains(first)) {
                    RouteDemand::Omit(b_omit.clone())
                } else {
                    RouteDemand::Whole
                }
            }
            _ => RouteDemand::Whole,
        }
    }

    /// Golden `collect_type_refs` (shared with the fixture dep derivation —
    /// dependency-edge classification is fixture INPUT for both pipelines).
    pub fn collect_type_refs(expr: &TypeExpr, out: &mut Vec<String>) {
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
            _ => {}
        }
    }

    /// Legacy `TypeDeclBody::lookup_object`: single → raw body; merged → a
    /// synthetic object of every contributor's direct members (FORWARD
    /// intersection/paren descent, duplicates preserved, heritage refs
    /// dropped).
    fn lookup_body(bodies: &[TypeExpr]) -> TypeExpr {
        if let [single] = bodies {
            return single.clone();
        }
        let mut members = Vec::new();
        for body in bodies {
            collect_direct_object_members(body, &mut members);
        }
        TypeExpr::Object(Arc::new(ObjectExpr {
            properties: members,
        }))
    }

    fn collect_direct_object_members(body: &TypeExpr, out: &mut Vec<ObjectMember>) {
        match body {
            TypeExpr::Object(object) => out.extend(object.properties.iter().cloned()),
            TypeExpr::Intersection(parts) => {
                for part in parts.iter() {
                    collect_direct_object_members(part, out);
                }
            }
            TypeExpr::Parenthesized(inner) => collect_direct_object_members(inner, out),
            _ => {}
        }
    }

    fn direct_object_properties(body: &TypeExpr) -> Vec<&ObjectProperty> {
        let mut result = Vec::new();
        let mut seen = FxHashSet::default();
        collect_direct_object_properties(body, &mut result, &mut seen);
        result
    }

    fn collect_direct_object_properties<'a>(
        body: &'a TypeExpr,
        out: &mut Vec<&'a ObjectProperty>,
        seen: &mut FxHashSet<String>,
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
            TypeExpr::Parenthesized(inner) => collect_direct_object_properties(inner, out, seen),
            _ => {}
        }
    }

    fn direct_object_property<'a>(body: &'a TypeExpr, name: &str) -> Option<&'a ObjectProperty> {
        direct_object_properties(body)
            .into_iter()
            .find(|prop| prop.name == name)
    }

    fn direct_object_member_names(body: &TypeExpr) -> Option<Vec<String>> {
        let names = direct_object_properties(body)
            .into_iter()
            .map(|prop| prop.name.clone())
            .collect::<Vec<_>>();
        (!names.is_empty()).then_some(names)
    }

    /// Legacy current-tree `extract_member_deps` over the ordered contributor
    /// bodies (first-contributor precedence via the shared seen set).
    fn extract_member_deps(bodies: &[TypeExpr]) -> FxHashMap<String, Vec<String>> {
        let mut result = FxHashMap::default();
        let mut seen = FxHashSet::default();
        for body in bodies {
            let mut props = Vec::new();
            collect_direct_object_properties(body, &mut props, &mut seen);
            for prop in props {
                let mut refs = Vec::new();
                collect_type_refs(&prop.ty, &mut refs);
                if !refs.is_empty() {
                    result.insert(prop.name.clone(), refs);
                }
            }
        }
        result
    }

    struct Walk<'s> {
        state: &'s MockFile,
        budget: usize,
        visited: FxHashSet<String>,
        local_used: Vec<String>,
        acc: Acc,
        steps: u64,
    }

    impl Walk<'_> {
        fn collect_whole_route_refs(&mut self, expr: &TypeExpr, context: Ctx) -> bool {
            match expr {
                TypeExpr::Parenthesized(inner) | TypeExpr::KeyOf(inner) | TypeExpr::Rest(inner) => {
                    self.collect_whole_route_refs(inner, context)
                }
                TypeExpr::Union(types) | TypeExpr::Intersection(types) => {
                    for inner in types.iter() {
                        if !self.collect_whole_route_refs(inner, context) {
                            return false;
                        }
                    }
                    true
                }
                TypeExpr::Array { element, .. } => self.collect_whole_route_refs(element, context),
                TypeExpr::Tuple { elements, .. } => {
                    for element in elements.iter() {
                        if !self.collect_whole_route_refs(&element.ty, context) {
                            return false;
                        }
                    }
                    true
                }
                TypeExpr::Object(obj) => {
                    if matches!(context, Ctx::LeafProperty) {
                        return true;
                    }
                    for member in &obj.properties {
                        let status = match member {
                            ObjectMember::Property(prop) => {
                                self.collect_whole_route_refs(&prop.ty, Ctx::LeafProperty)
                            }
                            ObjectMember::IndexSignature(sig) => {
                                self.collect_whole_route_refs(&sig.value_type, Ctx::LeafProperty)
                            }
                            ObjectMember::CallSignature(func)
                            | ObjectMember::ConstructSignature(func) => {
                                self.collect_whole_route_function_refs(func)
                            }
                            ObjectMember::Method(method) => {
                                self.collect_whole_route_function_refs(&method.function)
                            }
                        };
                        if !status {
                            return false;
                        }
                    }
                    true
                }
                TypeExpr::Function(func) | TypeExpr::ConstructorType(func) => {
                    if matches!(context, Ctx::LeafProperty) {
                        return true;
                    }
                    self.collect_whole_route_function_refs(func)
                }
                TypeExpr::Ref {
                    name,
                    type_arguments,
                } => {
                    let symbol_name = name.as_ref();
                    if let Some(shell) = self.state.external_shell(symbol_name, RouteDemand::Whole)
                    {
                        if matches!(context, Ctx::Root | Ctx::CallableParam) {
                            self.acc.add(shell);
                        }
                        return true;
                    }
                    if let Some(route) = self.utility_route_for_ref(symbol_name, type_arguments) {
                        return self.follow_routed_expr(&type_arguments[0], route);
                    }
                    if matches!(
                        symbol_name,
                        "Partial" | "Required" | "Readonly" | "NonNullable"
                    ) && !type_arguments.is_empty()
                        && !matches!(context, Ctx::LeafProperty)
                    {
                        return self.collect_whole_route_refs(&type_arguments[0], context);
                    }
                    if self.state.has_decl(symbol_name) {
                        return self.follow_local_symbol_precise(symbol_name, context);
                    }
                    true
                }
                TypeExpr::IndexedAccess { .. } => {
                    let Some((base_expr, route)) = self.extract_indexed_access_route(expr) else {
                        return true;
                    };
                    self.follow_routed_expr(base_expr, route)
                }
                TypeExpr::Conditional {
                    check,
                    extends,
                    true_type,
                    false_type,
                } => {
                    for inner in [check, extends, true_type, false_type] {
                        if !self.collect_whole_route_refs(inner, context) {
                            return false;
                        }
                    }
                    true
                }
                TypeExpr::Mapped {
                    source,
                    value,
                    name_type,
                    ..
                } => {
                    if !self.collect_whole_route_refs(source, context) {
                        return false;
                    }
                    if !self.collect_whole_route_refs(value, context) {
                        return false;
                    }
                    if let Some(name_type) = name_type.as_deref() {
                        return self.collect_whole_route_refs(name_type, context);
                    }
                    true
                }
                TypeExpr::TemplateLiteral { expressions, .. } => {
                    for inner in expressions.iter() {
                        if !self.collect_whole_route_refs(inner, context) {
                            return false;
                        }
                    }
                    true
                }
                TypeExpr::TypeOf(value_ref) => {
                    if let Some(root) = value_ref.path.first() {
                        if let Some(shell) =
                            self.state.external_shell(root.as_str(), RouteDemand::Whole)
                        {
                            if matches!(context, Ctx::Root | Ctx::CallableParam) {
                                self.acc.add(shell);
                            }
                        }
                    }
                    true
                }
                TypeExpr::ImportType { type_arguments, .. } => {
                    for argument in type_arguments.iter() {
                        if !self.collect_whole_route_refs(argument, context) {
                            return false;
                        }
                    }
                    true
                }
                _ => true,
            }
        }

        fn collect_whole_route_function_refs(&mut self, func: &FunctionExpr) -> bool {
            for param in &func.parameters {
                if !self.collect_whole_route_refs(&param.ty, Ctx::CallableParam) {
                    return false;
                }
            }
            for type_param in &func.type_parameters {
                if let Some(constraint) = type_param.constraint.as_deref() {
                    if !self.collect_whole_route_refs(constraint, Ctx::CallableParam) {
                        return false;
                    }
                }
                if let Some(default) = type_param.default.as_deref() {
                    if !self.collect_whole_route_refs(default, Ctx::CallableParam) {
                        return false;
                    }
                }
            }
            true
        }

        fn follow_local_symbol_precise(&mut self, symbol_name: &str, context: Ctx) -> bool {
            if !self.visited.insert(symbol_name.to_string()) {
                return true;
            }
            self.steps += 1;
            if self.steps as usize >= self.budget {
                return false;
            }
            let Some(bodies) = self.state.bodies(symbol_name) else {
                return true;
            };
            self.local_used.push(symbol_name.to_string());
            let lookup = lookup_body(bodies);
            self.collect_whole_route_refs(&lookup, context)
        }

        fn follow_routed_expr(&mut self, expr: &TypeExpr, route: RouteDemand) -> bool {
            match expr {
                TypeExpr::Parenthesized(inner) => self.follow_routed_expr(inner, route),
                TypeExpr::Ref {
                    name,
                    type_arguments,
                } if type_arguments.is_empty() => {
                    let symbol_name = name.as_ref();
                    if let Some(shell) = self.state.external_shell(symbol_name, route.clone()) {
                        self.acc.add(shell);
                        return true;
                    }
                    if self.state.has_decl(symbol_name) {
                        match &route {
                            RouteDemand::Whole => {
                                self.follow_local_symbol_precise(symbol_name, Ctx::Root)
                            }
                            RouteDemand::MemberPath(path) => {
                                let Some(bodies) = self.state.bodies(symbol_name) else {
                                    return true;
                                };
                                let mut seed_names = Vec::new();
                                let mut seed_external = Acc::default();
                                let mut seen_symbols = FxHashSet::default();
                                let lookup = lookup_body(bodies);
                                let found_path = collect_member_path_seed_names(
                                    self.state,
                                    &lookup,
                                    path,
                                    &mut seed_names,
                                    &mut seed_external,
                                    &mut seen_symbols,
                                );
                                if !found_path {
                                    return true;
                                }
                                for ext in seed_external.into_vec() {
                                    self.acc.add(ext);
                                }
                                for seed_name in seed_names {
                                    if let Some(shell) = self
                                        .state
                                        .external_shell(seed_name.as_str(), RouteDemand::Whole)
                                    {
                                        self.acc.add(shell);
                                    } else if self.state.has_decl(seed_name.as_str())
                                        && !self.follow_local_symbol_precise(
                                            seed_name.as_str(),
                                            Ctx::Root,
                                        )
                                    {
                                        return false;
                                    }
                                }
                                true
                            }
                            RouteDemand::Pick(_) | RouteDemand::Omit(_) => {
                                let closure =
                                    route_closure(self.state, symbol_name, &route, self.budget);
                                if matches!(closure.status, GoldenStatus::BudgetExceeded) {
                                    return false;
                                }
                                for local_name in closure.local_symbols_used {
                                    if self.visited.insert(local_name.clone()) {
                                        self.local_used.push(local_name);
                                    }
                                }
                                for ext in closure.unresolved_external {
                                    self.acc.add(ext);
                                }
                                true
                            }
                        }
                    } else {
                        true
                    }
                }
                _ => true,
            }
        }

        fn utility_route_for_ref(
            &self,
            name: &str,
            type_arguments: &[TypeExpr],
        ) -> Option<RouteDemand> {
            if type_arguments.len() != 2 {
                return None;
            }
            let mut seen_locals = FxHashSet::default();
            let keys = extract_string_literal_keys_from_type_expr(
                self.state,
                &type_arguments[1],
                &mut seen_locals,
            );
            if keys.is_empty() {
                return None;
            }
            match name {
                "Pick" => Some(RouteDemand::pick(keys)),
                "Omit" => Some(RouteDemand::omit(keys)),
                _ => None,
            }
        }

        fn extract_indexed_access_route<'a>(
            &self,
            expr: &'a TypeExpr,
        ) -> Option<(&'a TypeExpr, RouteDemand)> {
            let TypeExpr::IndexedAccess { object, index } = expr else {
                return None;
            };
            let mut seen_locals = FxHashSet::default();
            let keys =
                extract_string_literal_keys_from_type_expr(self.state, index, &mut seen_locals);
            if keys.is_empty() {
                return None;
            }
            let (base_expr, mut path) = self.extract_indexed_access_base(object.as_ref())?;
            if keys.len() == 1 {
                path.push(keys.into_iter().next().expect("len checked"));
                Some((base_expr, RouteDemand::member_path(path)))
            } else if path.is_empty() {
                Some((base_expr, RouteDemand::pick(keys)))
            } else {
                None
            }
        }

        fn extract_indexed_access_base<'a>(
            &self,
            expr: &'a TypeExpr,
        ) -> Option<(&'a TypeExpr, Vec<String>)> {
            match expr {
                TypeExpr::Parenthesized(inner) => self.extract_indexed_access_base(inner),
                TypeExpr::IndexedAccess { .. } => {
                    let (base_expr, route) = self.extract_indexed_access_route(expr)?;
                    match route {
                        RouteDemand::MemberPath(path) => Some((base_expr, path.to_vec())),
                        _ => None,
                    }
                }
                _ => Some((expr, Vec::new())),
            }
        }
    }

    /// Legacy cross-decl string-literal key extraction (the golden keeps the
    /// full recursive alias follow).
    fn extract_string_literal_keys_from_type_expr(
        state: &MockFile,
        expr: &TypeExpr,
        seen_locals: &mut FxHashSet<String>,
    ) -> Vec<String> {
        match expr {
            TypeExpr::Literal(LiteralValue::String(value)) => vec![value.clone()],
            TypeExpr::Union(types) => {
                let mut keys = Vec::new();
                for inner in types.iter() {
                    keys.extend(extract_string_literal_keys_from_type_expr(
                        state,
                        inner,
                        seen_locals,
                    ));
                }
                keys.sort();
                keys.dedup();
                keys
            }
            TypeExpr::Parenthesized(inner) => {
                extract_string_literal_keys_from_type_expr(state, inner, seen_locals)
            }
            TypeExpr::Ref {
                name,
                type_arguments,
            } if type_arguments.is_empty() && state.has_decl(name.as_ref()) => {
                if !seen_locals.insert(name.to_string()) {
                    return Vec::new();
                }
                let keys = state
                    .bodies(name.as_ref())
                    .map(|bodies| {
                        let lookup = lookup_body(bodies);
                        extract_string_literal_keys_from_type_expr(state, &lookup, seen_locals)
                    })
                    .unwrap_or_default();
                seen_locals.remove(name.as_ref());
                keys
            }
            _ => Vec::new(),
        }
    }

    fn collect_member_path_seed_names(
        state: &MockFile,
        expr: &TypeExpr,
        path: &[String],
        seed_names: &mut Vec<String>,
        seed_external: &mut Acc,
        seen_symbols: &mut FxHashSet<String>,
    ) -> bool {
        if path.is_empty() {
            collect_type_refs(expr, seed_names);
            seed_names.sort();
            seed_names.dedup();
            return true;
        }
        match expr {
            TypeExpr::Ref {
                name,
                type_arguments,
            } if type_arguments.is_empty() => {
                let symbol_name = name.to_string();
                if let Some(shell) = state.external_shell(
                    symbol_name.as_str(),
                    RouteDemand::member_path(path.iter().cloned()),
                ) {
                    seed_external.add(shell);
                    return true;
                }
                if !seen_symbols.insert(symbol_name.clone()) {
                    return false;
                }
                let result = state.bodies(symbol_name.as_str()).is_some_and(|bodies| {
                    let lookup = lookup_body(bodies);
                    collect_member_path_seed_names(
                        state,
                        &lookup,
                        path,
                        seed_names,
                        seed_external,
                        seen_symbols,
                    )
                });
                seen_symbols.remove(symbol_name.as_str());
                result
            }
            TypeExpr::Parenthesized(inner) => collect_member_path_seed_names(
                state,
                inner,
                path,
                seed_names,
                seed_external,
                seen_symbols,
            ),
            _ => {
                let Some(prop) = direct_object_property(expr, path[0].as_str()) else {
                    return false;
                };
                if path.len() == 1 {
                    collect_type_refs(&prop.ty, seed_names);
                    seed_names.sort();
                    seed_names.dedup();
                    true
                } else {
                    collect_member_path_seed_names(
                        state,
                        &prop.ty,
                        &path[1..],
                        seed_names,
                        seed_external,
                        seen_symbols,
                    )
                }
            }
        }
    }

    struct GoldenClosureOut {
        status: GoldenStatus,
        local_symbols_used: Vec<String>,
        unresolved_external: Vec<ExternalRouteRefFact>,
    }

    pub fn local_closure(state: &MockFile, symbol_name: &str, budget: usize) -> GoldenResult {
        let out = local_closure_inner(state, symbol_name, budget);
        GoldenResult {
            status: out.status,
            unresolved_external: out.unresolved_external,
        }
    }

    fn local_closure_inner(state: &MockFile, symbol_name: &str, budget: usize) -> GoldenClosureOut {
        let mut visited = FxHashSet::default();
        let mut pending = vec![symbol_name.to_string()];
        let mut acc = Acc::default();
        let mut local_used = Vec::new();
        let mut steps = 0usize;

        while let Some(current) = pending.pop() {
            if !visited.insert(current.clone()) {
                continue;
            }
            steps += 1;
            if steps >= budget {
                return GoldenClosureOut {
                    status: GoldenStatus::BudgetExceeded,
                    local_symbols_used: local_used,
                    unresolved_external: acc.into_vec(),
                };
            }
            if let Some(deps) = state.classified_deps_fixture(&current) {
                local_used.push(current.clone());
                for dep in &deps.local_deps {
                    if !visited.contains(dep.as_str()) {
                        pending.push(dep.clone());
                    }
                }
                for ext in &deps.external_deps {
                    acc.add(ext.clone());
                }
            } else if state.imports.contains_key(&current) {
                if let Some(shell) = state.external_shell(&current, RouteDemand::Whole) {
                    acc.add(shell);
                } else {
                    return GoldenClosureOut {
                        status: GoldenStatus::MissingLocalSymbol { name: current },
                        local_symbols_used: local_used,
                        unresolved_external: acc.into_vec(),
                    };
                }
            } else {
                return GoldenClosureOut {
                    status: GoldenStatus::MissingLocalSymbol { name: current },
                    local_symbols_used: local_used,
                    unresolved_external: acc.into_vec(),
                };
            }
        }
        finish(local_used, acc)
    }

    fn finish(local_used: Vec<String>, acc: Acc) -> GoldenClosureOut {
        let unresolved_external = acc.into_vec();
        let status = if unresolved_external.is_empty() {
            GoldenStatus::Resolved
        } else {
            GoldenStatus::ResolvedWithExternalDeps
        };
        GoldenClosureOut {
            status,
            local_symbols_used: local_used,
            unresolved_external,
        }
    }

    fn whole_route_closure(state: &MockFile, symbol_name: &str, budget: usize) -> GoldenClosureOut {
        let Some(bodies) = state.bodies(symbol_name) else {
            return local_closure_inner(state, symbol_name, budget);
        };
        let mut walk = Walk {
            state,
            budget,
            visited: FxHashSet::default(),
            local_used: Vec::new(),
            acc: Acc::default(),
            steps: 1,
        };
        walk.visited.insert(symbol_name.to_string());
        walk.local_used.push(symbol_name.to_string());
        let lookup = lookup_body(bodies);
        if !walk.collect_whole_route_refs(&lookup, Ctx::Root) {
            return GoldenClosureOut {
                status: GoldenStatus::BudgetExceeded,
                local_symbols_used: walk.local_used,
                unresolved_external: walk.acc.into_vec(),
            };
        }
        finish(walk.local_used, walk.acc)
    }

    fn member_route_closure(
        state: &MockFile,
        symbol_name: &str,
        members: &[&str],
        budget: usize,
    ) -> GoldenClosureOut {
        let Some(bodies) = state.bodies(symbol_name) else {
            return local_closure_inner(state, symbol_name, budget);
        };
        let member_deps = extract_member_deps(bodies);
        if member_deps.is_empty() {
            return local_closure_inner(state, symbol_name, budget);
        }
        let lookup = lookup_body(bodies);
        let mut seed_names: Vec<String> = Vec::new();
        let mut saw_known_member = false;
        for member in members {
            if let Some(deps) = member_deps.get(*member) {
                saw_known_member = true;
                for dep in deps {
                    if !seed_names.contains(dep) {
                        seed_names.push(dep.clone());
                    }
                }
                continue;
            }
            if let Some(prop) = direct_object_property(&lookup, member) {
                saw_known_member = true;
                let mut refs = Vec::new();
                collect_type_refs(&prop.ty, &mut refs);
                for dep in refs {
                    if !seed_names.contains(&dep) {
                        seed_names.push(dep);
                    }
                }
            }
        }
        if !saw_known_member {
            return local_closure_inner(state, symbol_name, budget);
        }
        if seed_names.is_empty() {
            return GoldenClosureOut {
                status: GoldenStatus::Resolved,
                local_symbols_used: vec![symbol_name.to_string()],
                unresolved_external: Vec::new(),
            };
        }
        seeded_bfs(state, symbol_name, seed_names, Acc::default(), budget)
    }

    fn member_path_route_closure(
        state: &MockFile,
        symbol_name: &str,
        path: &[String],
        budget: usize,
    ) -> GoldenClosureOut {
        if path.len() == 1 {
            return member_route_closure(state, symbol_name, &[path[0].as_str()], budget);
        }
        let Some(bodies) = state.bodies(symbol_name) else {
            return local_closure_inner(state, symbol_name, budget);
        };
        let mut seed_names = Vec::new();
        let mut seed_external = Acc::default();
        let mut seen_symbols = FxHashSet::default();
        let lookup = lookup_body(bodies);
        let found_path = collect_member_path_seed_names(
            state,
            &lookup,
            path,
            &mut seed_names,
            &mut seed_external,
            &mut seen_symbols,
        );
        if !found_path || (seed_names.is_empty() && seed_external.is_empty()) {
            return GoldenClosureOut {
                status: GoldenStatus::Resolved,
                local_symbols_used: vec![symbol_name.to_string()],
                unresolved_external: Vec::new(),
            };
        }
        seeded_bfs(state, symbol_name, seed_names, seed_external, budget)
    }

    fn seeded_bfs(
        state: &MockFile,
        symbol_name: &str,
        seed_names: Vec<String>,
        seed_external: Acc,
        budget: usize,
    ) -> GoldenClosureOut {
        let mut visited = FxHashSet::default();
        visited.insert(symbol_name.to_string());
        let mut pending = seed_names;
        let mut acc = seed_external;
        let mut local_used = vec![symbol_name.to_string()];
        let mut steps = 1usize;

        while let Some(current) = pending.pop() {
            if !visited.insert(current.clone()) {
                continue;
            }
            steps += 1;
            if steps >= budget {
                return GoldenClosureOut {
                    status: GoldenStatus::BudgetExceeded,
                    local_symbols_used: local_used,
                    unresolved_external: acc.into_vec(),
                };
            }
            if let Some(dep_edges) = state.classified_deps_fixture(&current) {
                local_used.push(current.clone());
                for dep in &dep_edges.local_deps {
                    if !visited.contains(dep.as_str()) {
                        pending.push(dep.clone());
                    }
                }
                for ext in &dep_edges.external_deps {
                    acc.add(ext.clone());
                }
            } else if state.imports.contains_key(&current) {
                if let Some(shell) = state.external_shell(&current, RouteDemand::Whole) {
                    acc.add(shell);
                }
            }
        }
        finish(local_used, acc)
    }

    fn route_closure(
        state: &MockFile,
        symbol_name: &str,
        route: &RouteDemand,
        budget: usize,
    ) -> GoldenClosureOut {
        match route {
            RouteDemand::Whole => whole_route_closure(state, symbol_name, budget),
            RouteDemand::MemberPath(path) if !path.is_empty() => {
                member_path_route_closure(state, symbol_name, path, budget)
            }
            RouteDemand::MemberPath(_) => local_closure_inner(state, symbol_name, budget),
            RouteDemand::Pick(members) => {
                let refs: Vec<&str> = members.iter().map(|s| s.as_str()).collect();
                member_route_closure(state, symbol_name, &refs, budget)
            }
            RouteDemand::Omit(omitted) => {
                let Some(bodies) = state.bodies(symbol_name) else {
                    return local_closure_inner(state, symbol_name, budget);
                };
                let lookup = lookup_body(bodies);
                let Some(members) = direct_object_member_names(&lookup) else {
                    return local_closure_inner(state, symbol_name, budget);
                };
                let remaining = members
                    .into_iter()
                    .filter(|name| !omitted.contains(name.as_str()))
                    .collect::<Vec<_>>();
                if remaining.is_empty() {
                    return GoldenClosureOut {
                        status: GoldenStatus::Resolved,
                        local_symbols_used: vec![symbol_name.to_string()],
                        unresolved_external: Vec::new(),
                    };
                }
                let refs: Vec<&str> = remaining.iter().map(|s| s.as_str()).collect();
                member_route_closure(state, symbol_name, &refs, budget)
            }
        }
    }

    pub fn run(
        state: &MockFile,
        symbol_name: &str,
        route: &RouteDemand,
        budget: usize,
    ) -> GoldenResult {
        let out = route_closure(state, symbol_name, route, budget);
        GoldenResult {
            status: out.status,
            unresolved_external: out.unresolved_external,
        }
    }
}

// ===========================================================================
// Parity harness
// ===========================================================================

fn map_status(status: &FactClosureStatus) -> golden::GoldenStatus {
    match status {
        FactClosureStatus::Resolved => golden::GoldenStatus::Resolved,
        FactClosureStatus::ResolvedWithExternalDeps => {
            golden::GoldenStatus::ResolvedWithExternalDeps
        }
        FactClosureStatus::MissingLocalSymbol { name } => {
            golden::GoldenStatus::MissingLocalSymbol { name: name.clone() }
        }
        FactClosureStatus::BudgetExceeded => golden::GoldenStatus::BudgetExceeded,
    }
}

/// Run BOTH pipelines and assert `unresolved_external` + `status`
/// BYTE-IDENTICAL. Returns the golden result for expected-value pinning.
#[track_caller]
fn assert_parity(
    state: &MockFile,
    symbol: &str,
    route: &RouteDemand,
    budget: usize,
) -> golden::GoldenResult {
    let golden_result = golden::run(state, symbol, route, budget);
    let new_result = route_closure_over_facts(state, symbol, route, budget);
    assert_eq!(
        map_status(&new_result.status),
        golden_result.status,
        "status parity for {symbol} route {route:?}"
    );
    assert_eq!(
        new_result.unresolved_external, golden_result.unresolved_external,
        "unresolved_external parity for {symbol} route {route:?}"
    );
    golden_result
}

fn ext(
    local: &str,
    specifier: &str,
    imported: &str,
    canonical: Option<&str>,
    route: RouteDemand,
) -> ExternalRouteRefFact {
    ExternalRouteRefFact {
        local_name: local.to_string(),
        source_specifier: specifier.to_string(),
        imported_name: imported.to_string(),
        canonical_id: canonical.map(Arc::from),
        route,
    }
}

fn obj(props: Vec<(&str, TypeExpr)>) -> TypeExpr {
    TypeExpr::Object(Arc::new(ObjectExpr {
        properties: props
            .into_iter()
            .map(|(name, ty)| {
                ObjectMember::Property(ObjectProperty::synthetic_public(
                    name.to_string(),
                    ty,
                    false,
                    false,
                ))
            })
            .collect(),
    }))
}

fn method_member(name: &str, params: Vec<(&str, TypeExpr)>) -> ObjectMember {
    ObjectMember::Method(MethodSignature::synthetic_public(
        name.to_string(),
        FunctionExpr::synthetic(
            params
                .into_iter()
                .map(|(pname, ty)| {
                    FunctionParam::synthetic(Some(pname.to_string()), ty, false, false)
                })
                .collect(),
            None,
            Vec::new(),
        ),
        false,
    ))
}

fn pick_of(base: TypeExpr, keys: &[&str]) -> TypeExpr {
    let key_expr = if keys.len() == 1 {
        TypeExpr::string_literal(keys[0])
    } else {
        TypeExpr::union(keys.iter().map(|k| TypeExpr::string_literal(*k)).collect())
    };
    TypeExpr::named_with_args("Pick", vec![base, key_expr])
}

fn indexed(base: TypeExpr, key: TypeExpr) -> TypeExpr {
    TypeExpr::IndexedAccess {
        object: Arc::new(base),
        index: Arc::new(key),
    }
}

const BUDGET: usize = 500;

// ===========================================================================
// Mandatory parity cases
// ===========================================================================

#[test]
fn parity_whole_alias_chain_and_callable_param() {
    // V1 load-bearing: `type Props = Base & { m(p: Imported): void };
    // type Base = Imported2` — the alias hop AND the callable param both emit.
    let state = MockFile::default()
        .import("Imported", "./a", "Imported", "/ws/a.ts")
        .import("Imported2", "./b", "Imported2", "/ws/b.ts")
        .decl(
            "Props",
            TypeExpr::intersection(vec![
                TypeExpr::named("Base"),
                TypeExpr::Object(Arc::new(ObjectExpr {
                    properties: vec![method_member("m", vec![("p", TypeExpr::named("Imported"))])],
                })),
            ]),
        )
        .decl("Base", TypeExpr::named("Imported2"));

    let result = assert_parity(&state, "Props", &RouteDemand::Whole, BUDGET);
    // Hand-built legacy expectation (order: alias hop first, then the param).
    assert_eq!(
        result.unresolved_external,
        vec![
            ext(
                "Imported2",
                "./b",
                "Imported2",
                Some("/ws/b.ts"),
                RouteDemand::Whole
            ),
            ext(
                "Imported",
                "./a",
                "Imported",
                Some("/ws/a.ts"),
                RouteDemand::Whole
            ),
        ]
    );
}

#[test]
fn parity_leaf_property_trio() {
    // (a) `type D = B; type B = { y: Pick<Q,'a'> }` → B followed at Root:
    // the object descends, the leaf utility STILL emits.
    let state_a = MockFile::default()
        .import("Q", "./q", "Q", "/ws/q.ts")
        .decl("D", TypeExpr::named("B"))
        .decl("B", obj(vec![("y", pick_of(TypeExpr::named("Q"), &["a"]))]));
    let result_a = assert_parity(&state_a, "D", &RouteDemand::Whole, BUDGET);
    assert_eq!(
        result_a.unresolved_external,
        vec![ext(
            "Q",
            "./q",
            "Q",
            Some("/ws/q.ts"),
            RouteDemand::pick(["a"])
        )]
    );

    // (b) `type D = { x: B }; type B = { y: Pick<Q,'a'> }` → B followed at
    // LeafProperty: the object top STOPS — Q is NOT reached.
    let state_b = MockFile::default()
        .import("Q", "./q", "Q", "/ws/q.ts")
        .decl("D", obj(vec![("x", TypeExpr::named("B"))]))
        .decl("B", obj(vec![("y", pick_of(TypeExpr::named("Q"), &["a"]))]));
    let result_b = assert_parity(&state_b, "D", &RouteDemand::Whole, BUDGET);
    assert_eq!(result_b.unresolved_external, Vec::new());
    assert_eq!(result_b.status, golden::GoldenStatus::Resolved);

    // (c) `type D = { x: B }; type B = Pick<Q,'a'>` → the TOP-LEVEL utility
    // is context-independent: Q IS reached even under the leaf follow.
    let state_c = MockFile::default()
        .import("Q", "./q", "Q", "/ws/q.ts")
        .decl("D", obj(vec![("x", TypeExpr::named("B"))]))
        .decl("B", pick_of(TypeExpr::named("Q"), &["a"]));
    let result_c = assert_parity(&state_c, "D", &RouteDemand::Whole, BUDGET);
    assert_eq!(
        result_c.unresolved_external,
        vec![ext(
            "Q",
            "./q",
            "Q",
            Some("/ws/q.ts"),
            RouteDemand::pick(["a"])
        )]
    );
}

#[test]
fn parity_guarded_emitting_only_carriers() {
    // (d) `type D = { x: B }; type B = Partial<Pick<Q,'a'>>` — the
    // Partial-family gate requires a non-leaf context: nothing under a leaf
    // follow.
    let state_d = MockFile::default()
        .import("Q", "./q", "Q", "/ws/q.ts")
        .decl("D", obj(vec![("x", TypeExpr::named("B"))]))
        .decl(
            "B",
            TypeExpr::named_with_args("Partial", vec![pick_of(TypeExpr::named("Q"), &["a"])]),
        );
    let result_d = assert_parity(&state_d, "D", &RouteDemand::Whole, BUDGET);
    assert_eq!(result_d.unresolved_external, Vec::new());

    // (e) same B followed at Root → the guarded utility emits.
    let state_e = MockFile::default()
        .import("Q", "./q", "Q", "/ws/q.ts")
        .decl("D", TypeExpr::named("B"))
        .decl(
            "B",
            TypeExpr::named_with_args("Partial", vec![pick_of(TypeExpr::named("Q"), &["a"])]),
        );
    let result_e = assert_parity(&state_e, "D", &RouteDemand::Whole, BUDGET);
    assert_eq!(
        result_e.unresolved_external,
        vec![ext(
            "Q",
            "./q",
            "Q",
            Some("/ws/q.ts"),
            RouteDemand::pick(["a"])
        )]
    );

    // (f) `type B = { m(p: Q): void }` under a leaf follow: the object top
    // stops BEFORE the callable param.
    let state_f = MockFile::default()
        .import("Q", "./q", "Q", "/ws/q.ts")
        .decl("D", obj(vec![("x", TypeExpr::named("B"))]))
        .decl(
            "B",
            TypeExpr::Object(Arc::new(ObjectExpr {
                properties: vec![method_member("m", vec![("p", TypeExpr::named("Q"))])],
            })),
        );
    let result_f = assert_parity(&state_f, "D", &RouteDemand::Whole, BUDGET);
    assert_eq!(result_f.unresolved_external, Vec::new());

    // (g) `type B = Pick<R,'b'> & { z: Q }` under a leaf follow: the
    // transparent intersection arm's utility emits, the object arm stops.
    let state_g = MockFile::default()
        .import("Q", "./q", "Q", "/ws/q.ts")
        .import("R", "./r", "R", "/ws/r.ts")
        .decl("D", obj(vec![("x", TypeExpr::named("B"))]))
        .decl(
            "B",
            TypeExpr::intersection(vec![
                pick_of(TypeExpr::named("R"), &["b"]),
                obj(vec![("z", TypeExpr::named("Q"))]),
            ]),
        );
    let result_g = assert_parity(&state_g, "D", &RouteDemand::Whole, BUDGET);
    assert_eq!(
        result_g.unresolved_external,
        vec![ext(
            "R",
            "./r",
            "R",
            Some("/ws/r.ts"),
            RouteDemand::pick(["b"])
        )]
    );
}

#[test]
fn parity_first_visit_context_order() {
    // `type D = { x: B } & B` — B is FIRST reached under LeafProperty (object
    // arm precedes the bare ref in source order), so the later Root reach is
    // visited-skipped and Q never emits...
    let state_leaf_first = MockFile::default()
        .import("Q", "./q", "Q", "/ws/q.ts")
        .decl(
            "D",
            TypeExpr::intersection(vec![
                obj(vec![("x", TypeExpr::named("B"))]),
                TypeExpr::named("B"),
            ]),
        )
        .decl("B", obj(vec![("y", pick_of(TypeExpr::named("Q"), &["a"]))]));
    let result_leaf_first = assert_parity(&state_leaf_first, "D", &RouteDemand::Whole, BUDGET);
    assert_eq!(result_leaf_first.unresolved_external, Vec::new());

    // ...while `type D = B & { x: B }` reaches B at Root first → Q emits.
    let state_root_first = MockFile::default()
        .import("Q", "./q", "Q", "/ws/q.ts")
        .decl(
            "D",
            TypeExpr::intersection(vec![
                TypeExpr::named("B"),
                obj(vec![("x", TypeExpr::named("B"))]),
            ]),
        )
        .decl("B", obj(vec![("y", pick_of(TypeExpr::named("Q"), &["a"]))]));
    let result_root_first = assert_parity(&state_root_first, "D", &RouteDemand::Whole, BUDGET);
    assert_eq!(
        result_root_first.unresolved_external,
        vec![ext(
            "Q",
            "./q",
            "Q",
            Some("/ws/q.ts"),
            RouteDemand::pick(["a"])
        )]
    );
}

#[test]
fn parity_member_path_nested_and_imported_tail_with_canonical() {
    // Exact nested terminal: `A['a']['b']` over `type A = { a: { b: Imported } }`.
    let state = MockFile::default()
        .import("Imported", "./dep", "Imported", "/ws/dep.ts")
        .decl(
            "A",
            obj(vec![("a", obj(vec![("b", TypeExpr::named("Imported"))]))]),
        );
    let route = RouteDemand::member_path(["a", "b"]);
    let result = assert_parity(&state, "A", &route, BUDGET);
    assert_eq!(
        result.unresolved_external,
        vec![ext(
            "Imported",
            "./dep",
            "Imported",
            Some("/ws/dep.ts"),
            RouteDemand::Whole
        )]
    );

    // Cross-decl forward: `type A = { a: B }; type B = { b: Imported }`.
    let state_fwd = MockFile::default()
        .import("Imported", "./dep", "Imported", "/ws/dep.ts")
        .decl("A", obj(vec![("a", TypeExpr::named("B"))]))
        .decl("B", obj(vec![("b", TypeExpr::named("Imported"))]));
    assert_parity(&state_fwd, "A", &route, BUDGET);

    // Imported tail with canonical id: `type A = { a: ImpB }` queried
    // `[a,b]` forwards `MemberPath([b])` INTO the import.
    let state_tail = MockFile::default()
        .import("ImpB", "./ext", "B", "/ws/ext.ts")
        .decl("A", obj(vec![("a", TypeExpr::named("ImpB"))]));
    let result_tail = assert_parity(&state_tail, "A", &route, BUDGET);
    assert_eq!(
        result_tail.unresolved_external,
        vec![ext(
            "ImpB",
            "./ext",
            "B",
            Some("/ws/ext.ts"),
            RouteDemand::member_path(["b"])
        )]
    );
}

#[test]
fn parity_union_terminal_misses_deeper_path() {
    // `{ primary: Alpha | Beta }` queried `[primary,label]` = legacy MISS
    // (the flat prefix+append template would wrongly forward both arms).
    let state = MockFile::default()
        .import("ImportedA", "./a", "ImportedA", "/ws/a.ts")
        .decl(
            "Props",
            obj(vec![(
                "primary",
                TypeExpr::union(vec![TypeExpr::named("Alpha"), TypeExpr::named("Beta")]),
            )]),
        )
        .decl("Alpha", obj(vec![("label", TypeExpr::named("ImportedA"))]))
        .decl(
            "Beta",
            obj(vec![("label", TypeExpr::primitive(PrimitiveName::String))]),
        );

    let miss = assert_parity(
        &state,
        "Props",
        &RouteDemand::member_path(["primary", "label"]),
        BUDGET,
    );
    assert_eq!(miss.status, golden::GoldenStatus::Resolved);
    assert_eq!(miss.unresolved_external, Vec::new());

    // The single-segment query still resolves through the union refs.
    assert_parity(
        &state,
        "Props",
        &RouteDemand::member_path(["primary"]),
        BUDGET,
    );
}

#[test]
fn parity_pick_omit_and_depless_members() {
    let state = MockFile::default()
        .import("Imported", "./dep", "Imported", "/ws/dep.ts")
        .decl(
            "P",
            obj(vec![
                ("x", TypeExpr::named("X")),
                ("y", TypeExpr::primitive(PrimitiveName::String)),
                ("z", TypeExpr::named("Imported")),
            ]),
        )
        .decl("X", TypeExpr::named("Imported"));

    // Pick over a ref-carrying member expands its deps.
    let picked = assert_parity(&state, "P", &RouteDemand::pick(["x"]), BUDGET);
    assert_eq!(
        picked.unresolved_external,
        vec![ext(
            "Imported",
            "./dep",
            "Imported",
            Some("/ws/dep.ts"),
            RouteDemand::Whole
        )]
    );

    // Dep-less member: found, empty closure.
    let depless = assert_parity(&state, "P", &RouteDemand::pick(["y"]), BUDGET);
    assert_eq!(depless.status, golden::GoldenStatus::Resolved);
    assert_eq!(depless.unresolved_external, Vec::new());

    // Omit keeps the non-omitted members' deps.
    let omitted = assert_parity(&state, "P", &RouteDemand::omit(["x", "y"]), BUDGET);
    assert_eq!(
        omitted.unresolved_external,
        vec![ext(
            "Imported",
            "./dep",
            "Imported",
            Some("/ws/dep.ts"),
            RouteDemand::Whole
        )]
    );

    // Omit of everything: minimal resolved closure.
    let all_omitted = assert_parity(&state, "P", &RouteDemand::omit(["x", "y", "z"]), BUDGET);
    assert_eq!(all_omitted.status, golden::GoldenStatus::Resolved);
    assert_eq!(all_omitted.unresolved_external, Vec::new());
}

#[test]
fn parity_nested_index_routes() {
    // Whole-route through `Imported['a']['b']`: path-precise MemberPath.
    let state = MockFile::default()
        .import("Imported", "./dep", "Imported", "/ws/dep.ts")
        .decl(
            "D",
            indexed(
                indexed(TypeExpr::named("Imported"), TypeExpr::string_literal("a")),
                TypeExpr::string_literal("b"),
            ),
        );
    let result = assert_parity(&state, "D", &RouteDemand::Whole, BUDGET);
    assert_eq!(
        result.unresolved_external,
        vec![ext(
            "Imported",
            "./dep",
            "Imported",
            Some("/ws/dep.ts"),
            RouteDemand::member_path(["a", "b"])
        )]
    );

    // Multi-key index over a bare base → Pick.
    let state_pick = MockFile::default()
        .import("Imported", "./dep", "Imported", "/ws/dep.ts")
        .decl(
            "D",
            indexed(
                TypeExpr::named("Imported"),
                TypeExpr::union(vec![
                    TypeExpr::string_literal("a"),
                    TypeExpr::string_literal("b"),
                ]),
            ),
        );
    let result_pick = assert_parity(&state_pick, "D", &RouteDemand::Whole, BUDGET);
    assert_eq!(
        result_pick.unresolved_external,
        vec![ext(
            "Imported",
            "./dep",
            "Imported",
            Some("/ws/dep.ts"),
            RouteDemand::pick(["a", "b"])
        )]
    );

    // Local base with whole-route seed imports SURVIVES a leaf follow (the
    // ungated indexed route resets the seed follow to Root).
    let state_leaf = MockFile::default()
        .import("Imported", "./dep", "Imported", "/ws/dep.ts")
        .decl("D", obj(vec![("x", TypeExpr::named("B"))]))
        .decl(
            "B",
            indexed(TypeExpr::named("Local2"), TypeExpr::string_literal("a")),
        )
        .decl("Local2", obj(vec![("a", TypeExpr::named("Imported"))]));
    let result_leaf = assert_parity(&state_leaf, "D", &RouteDemand::Whole, BUDGET);
    assert_eq!(
        result_leaf.unresolved_external,
        vec![ext(
            "Imported",
            "./dep",
            "Imported",
            Some("/ws/dep.ts"),
            RouteDemand::Whole
        )]
    );
}

#[test]
fn parity_cross_decl_pick_follow_slot() {
    // V3: `Pick<Imported, LocalKeys>` with a literal-union local alias.
    let state = MockFile::default()
        .import("Imported", "./dep", "Imported", "/ws/dep.ts")
        .decl(
            "D",
            TypeExpr::named_with_args(
                "Pick",
                vec![TypeExpr::named("Imported"), TypeExpr::named("LocalKeys")],
            ),
        )
        .decl(
            "LocalKeys",
            TypeExpr::union(vec![
                TypeExpr::string_literal("a"),
                TypeExpr::string_literal("b"),
            ]),
        );
    let result = assert_parity(&state, "D", &RouteDemand::Whole, BUDGET);
    assert_eq!(
        result.unresolved_external,
        vec![ext(
            "Imported",
            "./dep",
            "Imported",
            Some("/ws/dep.ts"),
            RouteDemand::pick(["a", "b"])
        )]
    );

    // Chained alias: `type LocalKeys = K2; type K2 = 'a'`.
    let state_chain = MockFile::default()
        .import("Imported", "./dep", "Imported", "/ws/dep.ts")
        .decl(
            "D",
            TypeExpr::named_with_args(
                "Pick",
                vec![TypeExpr::named("Imported"), TypeExpr::named("LocalKeys")],
            ),
        )
        .decl("LocalKeys", TypeExpr::named("K2"))
        .decl("K2", TypeExpr::string_literal("a"));
    let chained = assert_parity(&state_chain, "D", &RouteDemand::Whole, BUDGET);
    assert_eq!(
        chained.unresolved_external,
        vec![ext(
            "Imported",
            "./dep",
            "Imported",
            Some("/ws/dep.ts"),
            RouteDemand::pick(["a"])
        )]
    );

    // Non-literal key source fails CLOSED on both pipelines.
    let state_open = MockFile::default()
        .import("Imported", "./dep", "Imported", "/ws/dep.ts")
        .decl(
            "D",
            TypeExpr::named_with_args(
                "Pick",
                vec![TypeExpr::named("Imported"), TypeExpr::named("LocalKeys")],
            ),
        )
        .decl(
            "LocalKeys",
            TypeExpr::KeyOf(Arc::new(TypeExpr::named("Imported"))),
        );
    let open = assert_parity(&state_open, "D", &RouteDemand::Whole, BUDGET);
    assert_eq!(open.unresolved_external, Vec::new());

    // Self-referential alias terminates (the extraction cycle guard).
    let state_cycle = MockFile::default()
        .import("Imported", "./dep", "Imported", "/ws/dep.ts")
        .decl(
            "D",
            TypeExpr::named_with_args(
                "Pick",
                vec![TypeExpr::named("Imported"), TypeExpr::named("LocalKeys")],
            ),
        )
        .decl("LocalKeys", TypeExpr::named("LocalKeys"));
    let cyclic = assert_parity(&state_cycle, "D", &RouteDemand::Whole, BUDGET);
    assert_eq!(cyclic.unresolved_external, Vec::new());
}

#[test]
fn parity_userland_pick_empty_keys_fallback() {
    // Literal-empty keys (`keyof`-shaped) with a USERLAND local `Pick` decl:
    // the utility fall-through follows the userland decl whole.
    let state = MockFile::default()
        .import("Q", "./q", "Q", "/ws/q.ts")
        .import("Imported2", "./b", "Imported2", "/ws/b.ts")
        .decl(
            "D",
            TypeExpr::named_with_args(
                "Pick",
                vec![
                    TypeExpr::named("Q"),
                    TypeExpr::KeyOf(Arc::new(TypeExpr::named("Y"))),
                ],
            ),
        )
        .decl("Pick", TypeExpr::named("Imported2"));
    let result = assert_parity(&state, "D", &RouteDemand::Whole, BUDGET);
    assert_eq!(
        result.unresolved_external,
        vec![ext(
            "Imported2",
            "./b",
            "Imported2",
            Some("/ws/b.ts"),
            RouteDemand::Whole
        )]
    );

    // The DEFERRED-empty variant: the key alias resolves to no literal keys
    // downstream → the stored fallback follows the userland decl.
    let state_deferred = MockFile::default()
        .import("Q", "./q", "Q", "/ws/q.ts")
        .import("Imported2", "./b", "Imported2", "/ws/b.ts")
        .decl(
            "D",
            TypeExpr::named_with_args(
                "Pick",
                vec![TypeExpr::named("Q"), TypeExpr::named("LocalKeys")],
            ),
        )
        .decl("LocalKeys", TypeExpr::KeyOf(Arc::new(TypeExpr::named("Q"))))
        .decl("Pick", TypeExpr::named("Imported2"));
    let deferred = assert_parity(&state_deferred, "D", &RouteDemand::Whole, BUDGET);
    assert_eq!(
        deferred.unresolved_external,
        vec![ext(
            "Imported2",
            "./b",
            "Imported2",
            Some("/ws/b.ts"),
            RouteDemand::Whole
        )]
    );
}

#[test]
fn deferred_key_source_unavailable_fails_closed_not_wrong_route() {
    // `type LocalKeys = 'a'; type Pick = Imported2;
    //  type D = Pick<Imported, LocalKeys>` — the deferred edge carries BOTH a
    // key-source recipe AND the userland empty-keys fallback.
    let state = MockFile::default()
        .import("Imported", "./dep", "Imported", "/ws/dep.ts")
        .import("Imported2", "./b", "Imported2", "/ws/b.ts")
        .decl(
            "D",
            TypeExpr::named_with_args(
                "Pick",
                vec![TypeExpr::named("Imported"), TypeExpr::named("LocalKeys")],
            ),
        )
        .decl("LocalKeys", TypeExpr::string_literal("a"))
        .decl("Pick", TypeExpr::named("Imported2"));

    // Key source AVAILABLE: both pipelines resolve the deferred keys and
    // route the imported base.
    let followable = assert_parity(&state, "D", &RouteDemand::Whole, BUDGET);
    assert_eq!(
        followable.unresolved_external,
        vec![ext(
            "Imported",
            "./dep",
            "Imported",
            Some("/ws/dep.ts"),
            RouteDemand::pick(["a"])
        )]
    );

    // Key source UNAVAILABLE (`key_source_lookup → Unavailable`): the
    // enumeration is UNDECIDED, not empty — the deferred edge must contribute
    // NOTHING.
    // Firing the empty-keys fallback here would follow the userland `Pick`
    // decl and emit `Imported2` whole — a route the authoring walk never
    // produced (it emits `Imported: Pick(['a'])`).
    let unavailable = UnavailableKeySourceProvider { inner: &state };
    let result = route_closure_over_facts(&unavailable, "D", &RouteDemand::Whole, BUDGET);
    assert!(
        result
            .unresolved_external
            .iter()
            .all(|ext_ref| ext_ref.imported_name != "Imported2"),
        "an unavailable key source must not fire the empty-keys fallback (wrong route): {:?}",
        result.unresolved_external
    );
    assert_eq!(
        result.unresolved_external,
        Vec::new(),
        "an unavailable key source fails closed: the deferred edge contributes nothing"
    );
    assert_eq!(result.status, FactClosureStatus::Resolved);
}

/// PRODUCER discrimination: the key-source normalize is FLAT, LOCAL, and
/// NON-TRANSITIVE — literals flatten through unions/parens in SOURCE order
/// (unsorted: the engine sorts only a completed enumeration), a bare
/// zero-argument ref stays an UNRESOLVED alias arm anchored on the producing
/// canonical (its target's literals never leak into the produced fact), and a
/// non-conforming union arm contributes nothing.
#[test]
fn produce_key_source_fact_is_flat_local_and_non_transitive() {
    let file = MockFile::default().decl("K2", TypeExpr::string_literal("z"));
    // 'b' | ('a') | K2 | keyof Q
    let body = TypeExpr::union(vec![
        TypeExpr::string_literal("b"),
        TypeExpr::Parenthesized(Arc::new(TypeExpr::string_literal("a"))),
        TypeExpr::named("K2"),
        TypeExpr::KeyOf(Arc::new(TypeExpr::named("Q"))),
    ]);

    let fact = produce_key_source_fact(std::slice::from_ref(&body), &file);
    let KeySourceFact::LiteralAliasUnion { literals, aliases } = fact else {
        panic!("a literal/alias union normalizes to LiteralAliasUnion, got {fact:?}");
    };
    assert_eq!(
        literals.as_ref(),
        ["b".to_string(), "a".to_string()],
        "literal arms flatten in SOURCE order (paren erased), unsorted"
    );
    assert!(
        !literals.contains(&"z".to_string()),
        "NON-TRANSITIVE: the alias target's literal must not leak into the produced fact"
    );
    assert_eq!(
        aliases.len(),
        1,
        "one unresolved alias arm, got {aliases:?}"
    );
    assert_eq!(aliases[0].anchor.symbol.as_ref(), "K2");
    assert_eq!(
        aliases[0].anchor.canonical_id.as_ref(),
        "mock:file",
        "the alias ref anchors on the PRODUCING file's canonical"
    );
    assert_eq!(aliases[0].anchor.space, LocatorSymbolSpace::Type);
}

/// PRODUCER fail-closed control: a merged (multi-contributor) surface, an
/// open key expression (`keyof`), and an argumented ref all normalize to
/// `NoFiniteKeys` — a zero-key contribution, never a fabricated literal set.
#[test]
fn produce_key_source_fact_open_or_merged_shapes_have_no_finite_keys() {
    let file = MockFile::default();

    let merged = [
        obj(vec![("x", TypeExpr::string_literal("a"))]),
        obj(vec![("y", TypeExpr::string_literal("b"))]),
    ];
    assert_eq!(
        produce_key_source_fact(&merged, &file),
        KeySourceFact::NoFiniteKeys,
        "a multi-contributor (merged) surface enumerates no finite keys"
    );

    let keyof = TypeExpr::KeyOf(Arc::new(TypeExpr::named("Q")));
    assert_eq!(
        produce_key_source_fact(std::slice::from_ref(&keyof), &file),
        KeySourceFact::NoFiniteKeys,
        "an open key expression enumerates no finite keys"
    );

    let args_ref = TypeExpr::named_with_args("Wrap", vec![TypeExpr::string_literal("a")]);
    assert_eq!(
        produce_key_source_fact(std::slice::from_ref(&args_ref), &file),
        KeySourceFact::NoFiniteKeys,
        "a ref WITH type arguments is not a followable bare alias"
    );
}

/// ENGINE-fold control: a MERGED (multi-contributor) key alias enumerates to
/// ZERO keys — a DECIDED outcome (`NoFiniteKeys`, unlike an unavailable hop),
/// so the deferred edge applies the legacy empty-keys fall-through (the
/// userland local `Pick` decl follows whole) and never fabricates a Pick key
/// set for the imported base.
#[test]
fn deferred_key_source_merged_alias_resolves_to_zero_keys_fallback() {
    let state = MockFile::default()
        .import("Imported", "./dep", "Imported", "/ws/dep.ts")
        .import("Imported2", "./b", "Imported2", "/ws/b.ts")
        .decl(
            "D",
            TypeExpr::named_with_args(
                "Pick",
                vec![TypeExpr::named("Imported"), TypeExpr::named("K")],
            ),
        )
        .merged_decl(
            "K",
            vec![
                obj(vec![("x", TypeExpr::string_literal("a"))]),
                obj(vec![("y", TypeExpr::string_literal("b"))]),
            ],
        )
        .decl("Pick", TypeExpr::named("Imported2"));

    let result = route_closure_over_facts(&state, "D", &RouteDemand::Whole, BUDGET);
    assert!(
        result
            .unresolved_external
            .iter()
            .all(|ext_ref| ext_ref.imported_name != "Imported"),
        "a merged key alias must not fabricate Pick keys for the imported base: {:?}",
        result.unresolved_external
    );
    assert_eq!(
        result.unresolved_external,
        vec![ext(
            "Imported2",
            "./b",
            "Imported2",
            Some("/ws/b.ts"),
            RouteDemand::Whole
        )],
        "zero keys is DECIDED: the legacy empty-keys fall-through follows the userland Pick decl"
    );
}

/// CHARACTERIZATION (documented safe-direction gap, NOT parity): a composite
/// key expression containing a produce-time-local alias (`Pick<Q, K | 'c'>`,
/// `Pick<Q, K1 | K2>`) poisons produce-time key extraction — the producer
/// mints NO recipe and the closure emits NOTHING — while the authoring walk
/// resolves the alias(es) cross-decl and routes the imported base. The
/// single-symbol `KeyDomainFact::FollowSlot` recipe structurally cannot carry
/// a composite key expression, so this under-production persists until a
/// richer recipe lands. Both sides are pinned exactly: if the fact pipeline
/// ever starts emitting here (right OR wrong), this test trips and the gap
/// record must be revisited.
#[test]
fn characterize_composite_key_alias_under_production_vs_legacy() {
    // `type K = 'a' | 'b'; type D = Pick<Q, K | 'c'>` with Q imported.
    let state = MockFile::default()
        .import("Q", "./q", "Q", "/ws/q.ts")
        .decl(
            "D",
            TypeExpr::named_with_args(
                "Pick",
                vec![
                    TypeExpr::named("Q"),
                    TypeExpr::union(vec![TypeExpr::named("K"), TypeExpr::string_literal("c")]),
                ],
            ),
        )
        .decl(
            "K",
            TypeExpr::union(vec![
                TypeExpr::string_literal("a"),
                TypeExpr::string_literal("b"),
            ]),
        );

    // The authoring walk resolves the local alias INSIDE the composite.
    let golden_result = golden::run(&state, "D", &RouteDemand::Whole, BUDGET);
    assert_eq!(
        golden_result.unresolved_external,
        vec![ext(
            "Q",
            "./q",
            "Q",
            Some("/ws/q.ts"),
            RouteDemand::pick(["a", "b", "c"])
        )]
    );

    // The fact pipeline deliberately under-produces: poisoned key extraction
    // mints no recipe, nothing is emitted — never a wrong route.
    let new_result = route_closure_over_facts(&state, "D", &RouteDemand::Whole, BUDGET);
    assert_eq!(new_result.unresolved_external, Vec::new());
    assert_eq!(new_result.status, FactClosureStatus::Resolved);
    assert_ne!(
        golden_result.unresolved_external,
        new_result.unresolved_external,
        "the documented composite-key gap: the authoring walk resolves cross-decl, facts under-produce"
    );

    // Two local aliases in the composite land in the same class.
    let state_two = MockFile::default()
        .import("Q", "./q", "Q", "/ws/q.ts")
        .decl(
            "D",
            TypeExpr::named_with_args(
                "Pick",
                vec![
                    TypeExpr::named("Q"),
                    TypeExpr::union(vec![TypeExpr::named("K1"), TypeExpr::named("K2")]),
                ],
            ),
        )
        .decl("K1", TypeExpr::string_literal("a"))
        .decl("K2", TypeExpr::string_literal("b"));
    let golden_two = golden::run(&state_two, "D", &RouteDemand::Whole, BUDGET);
    assert_eq!(
        golden_two.unresolved_external,
        vec![ext(
            "Q",
            "./q",
            "Q",
            Some("/ws/q.ts"),
            RouteDemand::pick(["a", "b"])
        )]
    );
    let new_two = route_closure_over_facts(&state_two, "D", &RouteDemand::Whole, BUDGET);
    assert_eq!(new_two.unresolved_external, Vec::new());
    assert_eq!(new_two.status, FactClosureStatus::Resolved);
}

#[test]
fn parity_budget_exceeded_and_cycles() {
    // A long alias chain under a tiny budget trips identically (status AND
    // the partial external set).
    let mut state = MockFile::default().import("Imported", "./dep", "Imported", "/ws/dep.ts");
    for i in 0..10 {
        let next = if i == 9 {
            TypeExpr::named("Imported")
        } else {
            TypeExpr::named(format!("T{}", i + 1))
        };
        state = state.decl(&format!("T{i}"), next);
    }
    let tripped = assert_parity(&state, "T0", &RouteDemand::Whole, 5);
    assert_eq!(tripped.status, golden::GoldenStatus::BudgetExceeded);

    // Same-file cycle terminates and still emits the reachable utility.
    let state_cycle = MockFile::default()
        .import("Q", "./q", "Q", "/ws/q.ts")
        .decl("A", TypeExpr::named("B"))
        .decl(
            "B",
            TypeExpr::union(vec![
                TypeExpr::named("A"),
                pick_of(TypeExpr::named("Q"), &["a"]),
            ]),
        );
    let cyclic = assert_parity(&state_cycle, "A", &RouteDemand::Whole, BUDGET);
    assert_eq!(
        cyclic.unresolved_external,
        vec![ext(
            "Q",
            "./q",
            "Q",
            Some("/ws/q.ts"),
            RouteDemand::pick(["a"])
        )]
    );
}

#[test]
fn parity_merged_decl_flattened_walk_and_first_contributor_member_precedence() {
    // Merged contributors walk as ONE flattened member surface: the heritage
    // intersection ref is DROPPED (merged lookup semantics), the callable
    // param emits.
    let state = MockFile::default()
        .import("Q", "./q", "Q", "/ws/q.ts")
        .import("Base", "./base", "Base", "/ws/base.ts")
        .merged_decl(
            "M",
            vec![
                TypeExpr::intersection(vec![
                    TypeExpr::named("Base"),
                    obj(vec![("a", TypeExpr::named("Q"))]),
                ]),
                TypeExpr::Object(Arc::new(ObjectExpr {
                    properties: vec![method_member("m", vec![("p", TypeExpr::named("Q"))])],
                })),
            ],
        );
    let result = assert_parity(&state, "M", &RouteDemand::Whole, BUDGET);
    // Only the callable param reaches an emitting context: the heritage ref
    // vanished in the flatten, `a: Q` is leaf-gated.
    assert_eq!(
        result.unresolved_external,
        vec![ext("Q", "./q", "Q", Some("/ws/q.ts"), RouteDemand::Whole)]
    );

    // Member edges fold with FIRST-contributor precedence (the CURRENT
    // ordered-contributor extractor, NOT the pre-slot single-body one).
    let state_members = MockFile::default()
        .import("First", "./f", "First", "/ws/f.ts")
        .import("Second", "./s", "Second", "/ws/s.ts")
        .merged_decl(
            "M",
            vec![
                obj(vec![("a", TypeExpr::named("First"))]),
                obj(vec![("a", TypeExpr::named("Second"))]),
            ],
        );
    let picked = assert_parity(&state_members, "M", &RouteDemand::pick(["a"]), BUDGET);
    assert_eq!(
        picked.unresolved_external,
        vec![ext(
            "First",
            "./f",
            "First",
            Some("/ws/f.ts"),
            RouteDemand::Whole
        )]
    );
}

#[test]
fn parity_route_merge_across_sites() {
    // Two utility routes to the SAME import merge (key union) in insertion
    // order — the accumulator contract.
    let state = MockFile::default()
        .import("Q", "./q", "Q", "/ws/q.ts")
        .decl(
            "D",
            TypeExpr::union(vec![
                pick_of(TypeExpr::named("Q"), &["a"]),
                pick_of(TypeExpr::named("Q"), &["b"]),
            ]),
        );
    let result = assert_parity(&state, "D", &RouteDemand::Whole, BUDGET);
    assert_eq!(
        result.unresolved_external,
        vec![ext(
            "Q",
            "./q",
            "Q",
            Some("/ws/q.ts"),
            RouteDemand::pick(["a", "b"])
        )]
    );

    // A Whole reach widens the merged route to Whole.
    let state_widen = MockFile::default()
        .import("Q", "./q", "Q", "/ws/q.ts")
        .decl(
            "D",
            TypeExpr::union(vec![
                pick_of(TypeExpr::named("Q"), &["a"]),
                TypeExpr::named("Q"),
            ]),
        );
    let widened = assert_parity(&state_widen, "D", &RouteDemand::Whole, BUDGET);
    assert_eq!(
        widened.unresolved_external,
        vec![ext("Q", "./q", "Q", Some("/ws/q.ts"), RouteDemand::Whole)]
    );
}

#[test]
fn parity_typeof_import_gating() {
    // `typeof importedValue` emits at Root, is suppressed under a leaf follow.
    let typeof_expr = TypeExpr::TypeOf(verter_type_expr::ValueRef {
        path: vec!["importedValue".to_string()],
        type_args: Vec::new(),
    });
    let state_root = MockFile::default()
        .import("importedValue", "./v", "value", "/ws/v.ts")
        .decl("D", typeof_expr.clone());
    let rooted = assert_parity(&state_root, "D", &RouteDemand::Whole, BUDGET);
    assert_eq!(
        rooted.unresolved_external,
        vec![ext(
            "importedValue",
            "./v",
            "value",
            Some("/ws/v.ts"),
            RouteDemand::Whole
        )]
    );

    let state_leaf = MockFile::default()
        .import("importedValue", "./v", "value", "/ws/v.ts")
        .decl("D", obj(vec![("x", TypeExpr::named("B"))]))
        .decl("B", typeof_expr);
    let leafed = assert_parity(&state_leaf, "D", &RouteDemand::Whole, BUDGET);
    assert_eq!(leafed.unresolved_external, Vec::new());
}

#[test]
fn parity_missing_local_and_plain_local_closure() {
    // The plain local closure (empty MemberPath) and its missing-symbol arm.
    let state = MockFile::default()
        .import("Imported", "./dep", "Imported", "/ws/dep.ts")
        .decl("A", TypeExpr::named("Imported"));
    let plain = assert_parity(&state, "A", &RouteDemand::MemberPath(Arc::from([])), BUDGET);
    assert_eq!(
        plain.unresolved_external,
        vec![ext(
            "Imported",
            "./dep",
            "Imported",
            Some("/ws/dep.ts"),
            RouteDemand::Whole
        )]
    );

    let golden_missing = golden::local_closure(&state, "NotThere", BUDGET);
    let new_missing = local_closure_over_facts(&state, "NotThere", BUDGET);
    assert_eq!(map_status(&new_missing.status), golden_missing.status);
    assert_eq!(
        new_missing.unresolved_external,
        golden_missing.unresolved_external
    );
    assert_eq!(
        golden_missing.status,
        golden::GoldenStatus::MissingLocalSymbol {
            name: "NotThere".to_string()
        }
    );
}

// ===========================================================================
// Discrimination — perturb the produced facts, oracle goes RED
// ===========================================================================

/// Trio case (b)'s state: B guarded behind an object property.
fn trio_b_state() -> MockFile {
    MockFile::default()
        .import("Q", "./q", "Q", "/ws/q.ts")
        .decl("D", obj(vec![("x", TypeExpr::named("B"))]))
        .decl("B", obj(vec![("y", pick_of(TypeExpr::named("Q"), &["a"]))]))
}

#[test]
fn discrimination_dropping_external_context_diverges_from_golden() {
    // Rewrite B's External edge contexts to Root — simulating the
    // context-less External schema (context on Local only). The leaf follow
    // then WRONGLY keeps the guarded utility emit: the oracle diverges,
    // proving the per-edge External context is byte-parity-load-bearing.
    let state = trio_b_state();
    let golden_result = golden::run(&state, "D", &RouteDemand::Whole, BUDGET);
    assert_eq!(golden_result.unresolved_external, Vec::new());

    let flatten_context = |facts: ShallowRouteFacts| -> ShallowRouteFacts {
        let edges: Vec<WholeRouteEdgeFact> = facts
            .whole_route_edges
            .iter()
            .cloned()
            .map(|edge| match edge {
                WholeRouteEdgeFact::External { external_ref, .. } => WholeRouteEdgeFact::External {
                    external_ref,
                    context: verter_type_expr::facts::WholeRouteContextFact::Root,
                },
                other => other,
            })
            .collect();
        ShallowRouteFacts {
            whole_route_edges: edges.into(),
            ..facts
        }
    };
    let mutated = MutatedProvider {
        inner: &state,
        target: "B",
        mutate: &flatten_context,
    };
    let mutated_result = route_closure_over_facts(&mutated, "D", &RouteDemand::Whole, BUDGET);
    assert_ne!(
        mutated_result.unresolved_external, golden_result.unresolved_external,
        "dropping the External site context must break byte-parity (RED)"
    );

    // Revert (the unmutated pipeline) → GREEN.
    let clean = route_closure_over_facts(&state, "D", &RouteDemand::Whole, BUDGET);
    assert_eq!(clean.unresolved_external, golden_result.unresolved_external);
}

#[test]
fn discrimination_dropping_local_context_diverges_from_golden() {
    // Trio (b) again, but perturb the LOCAL edge's context (D's `x: B` edge
    // rewritten Root): the leaf gate vanishes and B walks at Root → Q leaks.
    let state = trio_b_state();
    let golden_result = golden::run(&state, "D", &RouteDemand::Whole, BUDGET);

    let root_locals = |facts: ShallowRouteFacts| -> ShallowRouteFacts {
        let edges: Vec<WholeRouteEdgeFact> = facts
            .whole_route_edges
            .iter()
            .cloned()
            .map(|edge| match edge {
                WholeRouteEdgeFact::Local { name, route, .. } => WholeRouteEdgeFact::Local {
                    name,
                    route,
                    context: verter_type_expr::facts::WholeRouteContextFact::Root,
                },
                other => other,
            })
            .collect();
        ShallowRouteFacts {
            whole_route_edges: edges.into(),
            ..facts
        }
    };
    let mutated = MutatedProvider {
        inner: &state,
        target: "D",
        mutate: &root_locals,
    };
    let mutated_result = route_closure_over_facts(&mutated, "D", &RouteDemand::Whole, BUDGET);
    assert_ne!(
        mutated_result.unresolved_external, golden_result.unresolved_external,
        "dropping the Local edge context must break byte-parity (RED)"
    );
}

#[test]
fn discrimination_flipping_route_diverges_from_golden() {
    // Trio (c): flip B's External route Pick → Whole. Byte-parity fails on
    // the route field.
    let state = MockFile::default()
        .import("Q", "./q", "Q", "/ws/q.ts")
        .decl("D", TypeExpr::named("B"))
        .decl("B", pick_of(TypeExpr::named("Q"), &["a"]));
    let golden_result = golden::run(&state, "D", &RouteDemand::Whole, BUDGET);

    let widen_routes = |facts: ShallowRouteFacts| -> ShallowRouteFacts {
        let edges: Vec<WholeRouteEdgeFact> = facts
            .whole_route_edges
            .iter()
            .cloned()
            .map(|edge| match edge {
                WholeRouteEdgeFact::External {
                    external_ref,
                    context,
                } => WholeRouteEdgeFact::External {
                    external_ref: ExternalRouteRefFact {
                        route: RouteDemand::Whole,
                        ..external_ref
                    },
                    context,
                },
                other => other,
            })
            .collect();
        ShallowRouteFacts {
            whole_route_edges: edges.into(),
            ..facts
        }
    };
    let mutated = MutatedProvider {
        inner: &state,
        target: "B",
        mutate: &widen_routes,
    };
    let mutated_result = route_closure_over_facts(&mutated, "D", &RouteDemand::Whole, BUDGET);
    assert_ne!(
        mutated_result.unresolved_external, golden_result.unresolved_external,
        "flipping a stored route must break byte-parity (RED)"
    );
}

#[test]
fn discrimination_dropping_seed_path_segment_diverges_from_golden() {
    // Truncate the forward edge's consumed prefix: the tail mis-forwards.
    let state = MockFile::default()
        .import("ImpB", "./ext", "B", "/ws/ext.ts")
        .decl("A", obj(vec![("a", TypeExpr::named("ImpB"))]));
    let route = RouteDemand::member_path(["a", "b"]);
    let golden_result = golden::run(&state, "A", &route, BUDGET);

    let drop_segment = |facts: ShallowRouteFacts| -> ShallowRouteFacts {
        let edges: Vec<verter_type_expr::facts::MemberPathSeedEdge> = facts
            .member_path_seed_edges
            .iter()
            .cloned()
            .map(|mut edge| {
                if !edge.path.is_empty() {
                    edge.path =
                        Arc::from(edge.path[..edge.path.len() - 1].to_vec().into_boxed_slice());
                }
                edge
            })
            .collect();
        ShallowRouteFacts {
            member_path_seed_edges: edges.into(),
            ..facts
        }
    };
    let mutated = MutatedProvider {
        inner: &state,
        target: "A",
        mutate: &drop_segment,
    };
    let mutated_result = route_closure_over_facts(&mutated, "A", &route, BUDGET);
    assert_ne!(
        mutated_result.unresolved_external, golden_result.unresolved_external,
        "dropping a seed-path segment must break byte-parity (RED)"
    );
}

#[test]
fn discrimination_accumulator_merges_same_target_routes() {
    // The closure MERGES same-(specifier, imported_name) refs by route union
    // in first-insertion order — a merge-dropping implementation would emit
    // two entries.
    let state = MockFile::default()
        .import("Q", "./q", "Q", "/ws/q.ts")
        .decl(
            "D",
            TypeExpr::union(vec![
                pick_of(TypeExpr::named("Q"), &["a"]),
                pick_of(TypeExpr::named("Q"), &["b"]),
            ]),
        );
    let result = route_closure_over_facts(&state, "D", &RouteDemand::Whole, BUDGET);
    assert_eq!(
        result.unresolved_external.len(),
        1,
        "merged, not duplicated"
    );
    assert_eq!(
        result.unresolved_external[0].route,
        RouteDemand::pick(["a", "b"])
    );
}
