#[cfg(test)]
use std::cell::Cell;
use std::sync::{Arc, OnceLock};

use rustc_hash::{FxHashMap, FxHashSet};
use verter_semantic::analysis::type_solver::prepared::PreparedExternalDep;
use verter_semantic::analysis::type_solver::{
    PreparedTypeDecl, PreparedValueDecl, ResolvedRootIdentity,
};

use super::{ExportTarget, ShallowFileState};

type PreparedTypeDeclSlot = Arc<OnceLock<Option<Arc<PreparedTypeDecl>>>>;
type PreparedTypeDeclSlots = Arc<FxHashMap<String, PreparedTypeDeclSlot>>;
type PreparedValueDeclSlot = Arc<OnceLock<Option<Arc<PreparedValueDecl>>>>;
type PreparedValueDeclSlots = Arc<FxHashMap<String, PreparedValueDeclSlot>>;

/// Import binding: maps a local import name to its resolved target.
/// Used by the declaration-scope solver host to resolve cross-file references.
#[derive(Debug, Clone)]
pub struct ImportBinding {
    pub canonical_id: String,
    pub exported_name: String,
}

/// Script-setup generic type-parameter binding for
/// `<script setup lang="ts" generic="T extends Item = Item">`
/// parameters.
///
/// The binding carries the parameter name plus its declaration-site
/// `extends` constraint and `=` default as **unlowered** [`TypeExpr`]
/// values; the dispatch lowering path interns them on demand into
/// [`SemanticNodeData::TypeParam`](crate::semantic_query::SemanticNodeData::TypeParam)
/// via `shallow_lower_type_expr`. `PreparedTypeDecl` would be the
/// wrong category for this data — type parameters do not have alias
/// bodies, scope-local `name_resolution`, or the rest of the
/// prepared-decl surface.
///
/// `ordinal` carries the 0-based clause position into the lowered
/// `SemanticNodeData::TypeParam.param_index`, disambiguating same-name
/// parameters across multiple script-setup declarations within one
/// file.
#[derive(Debug, Clone)]
pub struct TypeParamBinding {
    pub name: Arc<str>,
    /// 0-based position in the
    /// `<script setup generic="T, U, V">` clause, used as
    /// [`SemanticNodeData::TypeParam.param_index`](crate::semantic_query::SemanticNodeData::TypeParam)
    /// so multiple script-setup parameters in one file get distinct
    /// identity tuples.
    pub ordinal: u16,
    pub constraint: Option<Arc<verter_type_expr::TypeExpr>>,
    pub default: Option<Arc<verter_type_expr::TypeExpr>>,
}

fn resolve_import_target(
    owner_canonical_id: &str,
    dep_edges: Option<&FxHashMap<String, String>>,
    source_specifier: &str,
    canonical_id: Option<&str>,
) -> String {
    if let Some(canonical_id) = dep_edges
        .and_then(|edges| edges.get(source_specifier))
        .cloned()
    {
        return canonical_id;
    }

    if let Some(canonical_id) = canonical_id.filter(|canonical_id| !canonical_id.is_empty()) {
        return canonical_id.to_string();
    }

    let relative_last_segment = source_specifier
        .rsplit('/')
        .next()
        .unwrap_or(source_specifier);
    if source_specifier.starts_with('.') && relative_last_segment.contains('.') {
        crate::id::resolve_external(owner_canonical_id, source_specifier)
    } else {
        source_specifier.to_string()
    }
}

/// Prepare a local type declaration from a canonical shallow file state.
///
/// Populates local_deps, external_deps, and name_resolution from the
/// shallow symbol, and auto-builds the member index for object-like bodies.
///
/// `dep_edges` maps import specifiers (e.g. `./types`) to resolved canonical
/// IDs (e.g. `/src/types.ts`). When provided, external deps and
/// `name_resolution` entries use the resolved canonical IDs. When `None`,
/// raw import specifiers are used as-is.
pub fn prepare_local_type_decl(
    canonical_id: &str,
    state: &ShallowFileState,
    symbol_name: &str,
    dep_edges: Option<&FxHashMap<String, String>>,
) -> Option<PreparedTypeDecl> {
    let symbol = state.symbol(symbol_name)?;
    if state.is_import_local(symbol_name) {
        return None;
    }

    #[cfg(test)]
    PREPARED_TYPE_DECL_BUILD_COUNT.with(|count| {
        count.set(count.get().saturating_add(1));
    });

    let mut prepared = PreparedTypeDecl::new(
        ResolvedRootIdentity::new(canonical_id, symbol_name),
        symbol.kind,
        symbol.raw_body.clone(),
    );
    prepared.type_parameters = symbol.type_parameters.clone();
    prepared.local_deps = symbol.local_deps.clone();
    prepared.external_deps = symbol
        .external_deps
        .iter()
        .map(|dep| {
            let resolved_id = resolve_import_target(
                canonical_id,
                dep_edges,
                &dep.source_specifier,
                Some(dep.canonical_id.as_str()),
            );
            PreparedExternalDep {
                canonical_id: resolved_id,
                symbol_name: dep.imported_name.clone(),
            }
        })
        .collect();

    // Build name_resolution: maps bare names in the body to resolved identities
    // Local deps resolve to the same file
    for dep_name in state.symbols.keys() {
        prepared.name_resolution.insert(
            dep_name.clone(),
            ResolvedRootIdentity::new(canonical_id, dep_name),
        );
    }
    for dep_name in state.value_symbols.keys() {
        prepared.name_resolution.insert(
            dep_name.clone(),
            ResolvedRootIdentity::new(canonical_id, dep_name),
        );
    }
    // External deps resolve through import bindings → canonical_id
    for (local_name, target) in state.import_targets.iter() {
        let resolved_id = resolve_import_target(
            canonical_id,
            dep_edges,
            &target.source_specifier,
            Some(target.canonical_id.as_str()),
        );
        prepared.name_resolution.insert(
            local_name.clone(),
            ResolvedRootIdentity::new(&resolved_id, &target.imported_name),
        );
    }

    // Populate cache deps for invalidation
    let hash_u64 = u64::from_le_bytes(state.whole_hash[..8].try_into().unwrap_or_default());
    prepared.cache_deps.defining_file = Some((canonical_id.to_string(), hash_u64));
    prepared.cache_deps.local_closure_participants = symbol.local_deps.clone();

    prepared.build_member_index();
    prepared.classify_wrapper_shape();
    prepared.classify_projection();
    Some(prepared)
}

/// Prepare a named exported type declaration after routing has selected the
/// defining file.
pub fn prepare_exported_type_decl(
    canonical_id: &str,
    state: &ShallowFileState,
    exported_name: &str,
    dep_edges: Option<&FxHashMap<String, String>>,
) -> Option<PreparedTypeDecl> {
    let ExportTarget::Local { symbol_name } = state.export_target(exported_name)? else {
        return None;
    };

    let mut prepared = prepare_local_type_decl(canonical_id, state, symbol_name, dep_edges)?;
    prepared.exported_name = Some(exported_name.to_string());
    prepared.provenance.route_kind = Some("direct".to_string());
    Some(prepared)
}

/// Prepare a local value declaration from a canonical shallow file state.
pub fn prepare_local_value_decl(
    canonical_id: &str,
    state: &ShallowFileState,
    symbol_name: &str,
    dep_edges: Option<&FxHashMap<String, String>>,
) -> Option<PreparedValueDecl> {
    let symbol = state.value_symbol(symbol_name)?;
    if state.is_import_local(symbol_name) {
        return None;
    }

    let mut prepared = PreparedValueDecl::new(
        ResolvedRootIdentity::new(canonical_id, symbol_name),
        symbol.kind,
    );
    prepared.type_annotation = symbol.type_annotation.clone();
    prepared.function_signature = symbol.function_signature.clone();
    prepared.object_shape = symbol.object_shape.clone();
    prepared.enum_members = symbol.enum_members.clone();

    for local_name in state.symbols.keys() {
        prepared.name_resolution.insert(
            local_name.clone(),
            ResolvedRootIdentity::new(canonical_id, local_name),
        );
    }
    for local_name in state.value_symbols.keys() {
        prepared.name_resolution.insert(
            local_name.clone(),
            ResolvedRootIdentity::new(canonical_id, local_name),
        );
    }

    // Build name_resolution for type annotations that reference
    // imported or local types in the defining file
    // Index all import targets as potential name resolution entries
    for (local_name, target) in state.import_targets.iter() {
        let resolved_id = resolve_import_target(
            canonical_id,
            dep_edges,
            &target.source_specifier,
            Some(target.canonical_id.as_str()),
        );
        prepared.name_resolution.insert(
            local_name.clone(),
            ResolvedRootIdentity::new(&resolved_id, &target.imported_name),
        );
    }

    let hash_u64 = u64::from_le_bytes(state.whole_hash[..8].try_into().unwrap_or_default());
    prepared.cache_deps.defining_file = Some((canonical_id.to_string(), hash_u64));

    Some(prepared)
}

/// Prepare a named exported value declaration after routing has selected the
/// defining file.
pub fn prepare_exported_value_decl(
    canonical_id: &str,
    state: &ShallowFileState,
    exported_name: &str,
    dep_edges: Option<&FxHashMap<String, String>>,
) -> Option<PreparedValueDecl> {
    let ExportTarget::Local { symbol_name } = state.export_target(exported_name)? else {
        return None;
    };

    let mut prepared = prepare_local_value_decl(canonical_id, state, symbol_name, dep_edges)?;
    prepared.exported_name = Some(exported_name.to_string());
    Some(prepared)
}

#[derive(Clone)]
pub struct PreparedTypeDeclCache {
    canonical_id: Arc<str>,
    state: Arc<ShallowFileState>,
    dep_edges: Arc<FxHashMap<String, String>>,
    slots: PreparedTypeDeclSlots,
}

impl PreparedTypeDeclCache {
    pub fn len(&self) -> usize {
        self.slots.len()
    }

    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }

    pub fn contains_key(&self, symbol_name: &str) -> bool {
        self.slots.contains_key(symbol_name)
    }

    /// The defining file's content identity — the `whole_hash` of the
    /// [`ShallowFileState`] every `PreparedTypeDecl` in this cache is
    /// built from.
    ///
    /// Provenance source for a query-identity cache producer whose
    /// value is derived from a `PreparedTypeDecl`: the producer reads
    /// the decl AND this hash from the SAME bundle, so the value and
    /// its self-root fact signature are provably one content version
    /// (untorn against a racing `upsert`). The hash is also view-correct
    /// — an overlay-bearing bundle (materialised through
    /// `prepared_decl_bundle_with_context`) carries the overlay's
    /// `ShallowFileState`, so the hash reflects whatever view the
    /// bundle was built from.
    pub fn defining_content_hash(&self) -> verter_semantic::analysis::Hash16 {
        self.state.whole_hash
    }

    pub fn get(&self, symbol_name: &str) -> Option<Arc<PreparedTypeDecl>> {
        let slot = self.slots.get(symbol_name)?;
        slot.get_or_init(|| {
            prepare_local_type_decl(
                self.canonical_id.as_ref(),
                self.state.as_ref(),
                symbol_name,
                (!self.dep_edges.is_empty()).then_some(self.dep_edges.as_ref()),
            )
            .map(Arc::new)
        })
        .clone()
    }
}

#[derive(Clone)]
pub struct PreparedValueDeclCache {
    canonical_id: Arc<str>,
    state: Arc<ShallowFileState>,
    dep_edges: Arc<FxHashMap<String, String>>,
    slots: PreparedValueDeclSlots,
}

impl PreparedValueDeclCache {
    pub fn len(&self) -> usize {
        self.slots.len()
    }

    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }

    pub fn contains_key(&self, symbol_name: &str) -> bool {
        self.slots.contains_key(symbol_name)
    }

    pub fn get(&self, symbol_name: &str) -> Option<Arc<PreparedValueDecl>> {
        let slot = self.slots.get(symbol_name)?;
        slot.get_or_init(|| {
            prepare_local_value_decl(
                self.canonical_id.as_ref(),
                self.state.as_ref(),
                symbol_name,
                (!self.dep_edges.is_empty()).then_some(self.dep_edges.as_ref()),
            )
            .map(Arc::new)
        })
        .clone()
    }
}

/// Atomic declaration-surface bundle for one canonical file.
///
/// Valid as long as its `ImportRoute` and `FileWholeHash` facts match the
/// current store view. Never incrementally merged — rebuilt wholesale when the
/// import graph or file content changes.
#[derive(Clone)]
pub struct PreparedDeclBundle {
    /// The content version (`ShallowFileState::whole_hash`) of the
    /// canonical file this bundle was built from. A consumer that
    /// resolves a declaration through this bundle and roots a cache
    /// entry on the bundle's declaring file reads this — an OBSERVED
    /// identity captured when the bundle was materialised, never a
    /// current-content re-read at the consumer's signature-build time.
    pub owner_whole_hash: crate::resolver_core::ResolverHash16,
    pub prepared_type_decls: PreparedTypeDeclCache,
    pub prepared_value_decls: PreparedValueDeclCache,
    /// The dep_edges snapshot used to build this bundle.
    /// Stored so `SessionSolverHost::with_declaration_scope` can read it
    /// instead of recomputing dependency resolutions from the store view.
    pub dep_edges: Arc<FxHashMap<String, String>>,
    /// Resolved import bindings: local name → (canonical_id, exported_name).
    /// Built from the owner file's import targets + dep_edges during
    /// bundle materialization.
    pub import_bindings: FxHashMap<String, ImportBinding>,
    /// Same-file type names visible in the declaration scope.
    pub scope_type_names: FxHashSet<String>,
    /// Same-file value names visible in the declaration scope.
    pub scope_value_names: FxHashSet<String>,
    /// Script-setup generic type parameter bindings (Vue SFC only).
    /// Empty for non-Vue files. Populated once during bundle materialization
    /// so the solver hot path never calls `current_eval_state`.
    ///
    /// Each entry is a [`TypeParamBinding`] — type parameters are not
    /// type aliases, so they do not flow through `PreparedTypeDecl`.
    pub script_setup_type_bindings: FxHashMap<String, TypeParamBinding>,
}

/// Build an atomic declaration-surface bundle from a shallow file state and
/// resolved dependency edges. The bundle is immutable after construction.
///
/// `script_setup_type_bindings` are supplied by the caller (host_manage) because
/// extracting them requires access to the host's source/parse state, which is a
/// session-level concern. For non-Vue files the caller passes an empty map.
///
/// Each entry is a [`TypeParamBinding`]; type parameters do not flow
/// through `PreparedTypeDecl` because they are not type aliases.
pub fn build_prepared_decl_bundle(
    canonical_id: &str,
    state: Arc<ShallowFileState>,
    dep_edges: FxHashMap<String, String>,
    script_setup_type_bindings: FxHashMap<String, TypeParamBinding>,
) -> PreparedDeclBundle {
    let dep_edges = Arc::new(dep_edges);

    // Build import bindings from shallow state import_targets + dep_edges.
    let mut import_bindings = FxHashMap::default();
    for (local_name, target) in state.import_targets.iter() {
        let resolved_id = if target.canonical_id.is_empty() {
            dep_edges.get(&target.source_specifier).cloned()
        } else {
            Some(target.canonical_id.clone())
        };
        if let Some(resolved_id) = resolved_id {
            import_bindings.insert(
                local_name.clone(),
                ImportBinding {
                    canonical_id: resolved_id,
                    exported_name: target.imported_name.clone(),
                },
            );
        }
    }

    // Collect same-file symbol name sets.
    let scope_type_names: FxHashSet<String> = state.symbols.keys().cloned().collect();
    let scope_value_names: FxHashSet<String> = state.value_symbols.keys().cloned().collect();

    let owner_whole_hash = state.whole_hash;
    PreparedDeclBundle {
        owner_whole_hash,
        prepared_type_decls: build_prepared_type_decl_cache(
            canonical_id,
            Arc::clone(&state),
            Arc::clone(&dep_edges),
        ),
        prepared_value_decls: build_prepared_value_decl_cache(
            canonical_id,
            Arc::clone(&state),
            Arc::clone(&dep_edges),
        ),
        dep_edges,
        import_bindings,
        scope_type_names,
        scope_value_names,
        script_setup_type_bindings,
    }
}

/// Build the host-owned prepared type declaration cache for one defining file.
pub fn build_prepared_type_decl_cache(
    canonical_id: &str,
    state: Arc<ShallowFileState>,
    dep_edges: Arc<FxHashMap<String, String>>,
) -> PreparedTypeDeclCache {
    let slots = state
        .symbols
        .keys()
        .filter(|symbol_name| !state.is_import_local(symbol_name))
        .map(|symbol_name| (symbol_name.clone(), Arc::new(OnceLock::new())))
        .collect();

    PreparedTypeDeclCache {
        canonical_id: Arc::from(canonical_id),
        state,
        dep_edges,
        slots: Arc::new(slots),
    }
}

/// Build the host-owned prepared value declaration cache for one defining file.
pub fn build_prepared_value_decl_cache(
    canonical_id: &str,
    state: Arc<ShallowFileState>,
    dep_edges: Arc<FxHashMap<String, String>>,
) -> PreparedValueDeclCache {
    let slots = state
        .value_symbols
        .keys()
        .filter(|symbol_name| !state.is_import_local(symbol_name))
        .map(|symbol_name| (symbol_name.clone(), Arc::new(OnceLock::new())))
        .collect();

    PreparedValueDeclCache {
        canonical_id: Arc::from(canonical_id),
        state,
        dep_edges,
        slots: Arc::new(slots),
    }
}

#[cfg(test)]
thread_local! {
    static PREPARED_TYPE_DECL_BUILD_COUNT: Cell<usize> = const { Cell::new(0) };
}

#[cfg(test)]
pub(crate) fn reset_prepared_type_decl_build_count_for_tests() {
    PREPARED_TYPE_DECL_BUILD_COUNT.with(|count| count.set(0));
}

#[cfg(test)]
pub(crate) fn prepared_type_decl_build_count_for_tests() -> usize {
    PREPARED_TYPE_DECL_BUILD_COUNT.with(|count| count.get())
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use verter_compiler::utils::oxc::vue::resolve_type::{
        analyze_external_type_source, AnalyzedExternalTypeSource,
    };
    use verter_semantic::analysis::type_eval::ValueDeclKind;
    use verter_semantic::analysis::type_eval_build::parse_and_build_env;
    use verter_semantic::analysis::Hash16;

    use super::*;

    fn make_analysis(source: &str) -> Arc<AnalyzedExternalTypeSource> {
        let alloc = oxc_allocator::Allocator::new();
        Arc::new(analyze_external_type_source(source, &alloc))
    }

    #[test]
    fn prepares_local_exported_type_decl_from_shallow_file_state() {
        let source = "export interface Props { label: string }";
        let analysis = make_analysis(source);
        let env = parse_and_build_env(source);
        let state = ShallowFileState::from_analysis(Hash16::default(), analysis, Some(&env));

        let prepared = prepare_exported_type_decl("/src/types.ts", &state, "Props", None)
            .expect("Props should prepare");

        assert_eq!(prepared.root_identity.canonical_id, "/src/types.ts");
        assert_eq!(prepared.root_identity.symbol_name, "Props");
        assert_eq!(prepared.exported_name.as_deref(), Some("Props"));

        // Member index should be auto-populated for interface with properties
        assert!(
            prepared.member_index.contains_key("label"),
            "member index should contain 'label' property"
        );
    }

    #[test]
    fn prepares_local_value_decl_from_shallow_file_state() {
        let source = r#"
export interface Props { label: string }
export const defaults: Props = { label: 'ok' }
"#;
        let analysis = make_analysis(source);
        let env = parse_and_build_env(source);
        let state = ShallowFileState::from_analysis(Hash16::default(), analysis, Some(&env));

        let prepared = prepare_exported_value_decl("/src/types.ts", &state, "defaults", None)
            .expect("defaults should prepare");

        assert_eq!(prepared.root_identity.canonical_id, "/src/types.ts");
        assert_eq!(prepared.root_identity.symbol_name, "defaults");
        assert_eq!(prepared.exported_name.as_deref(), Some("defaults"));
        assert_eq!(prepared.kind, ValueDeclKind::Const);
        assert!(prepared.type_annotation.is_some());
    }

    #[test]
    fn prepared_type_decl_name_resolution_includes_typeof_imports() {
        let source = r#"
import type { ComponentConfig } from './types'
import { theme } from './theme'

export type Button = ComponentConfig<typeof theme>
"#;
        let analysis = make_analysis(source);
        let env = parse_and_build_env(source);
        let state = ShallowFileState::from_analysis(Hash16::default(), analysis, Some(&env));
        let dep_edges = FxHashMap::from_iter([
            ("./types".to_string(), "/src/types.ts".to_string()),
            ("./theme".to_string(), "/src/theme.ts".to_string()),
        ]);

        let prepared =
            prepare_exported_type_decl("/src/button-types.ts", &state, "Button", Some(&dep_edges))
                .expect("Button should prepare");

        assert_eq!(
            prepared
                .name_resolution
                .get("theme")
                .map(|id| (id.canonical_id.as_str(), id.symbol_name.as_str())),
            Some(("/src/theme.ts", "theme"))
        );
    }

    #[test]
    fn prepared_type_decl_falls_back_to_canonical_relative_targets_without_dep_edges() {
        let source = r#"
import type { ComponentConfig } from './tv.ts'
import type { AppConfig } from './schema.ts'
import theme from './theme.ts'

export type Button = ComponentConfig<typeof theme, AppConfig, 'button'>
"#;
        let analysis = make_analysis(source);
        let env = parse_and_build_env(source);
        let state = ShallowFileState::from_analysis(Hash16::default(), analysis, Some(&env));

        let prepared = prepare_exported_type_decl("/src/Button.vue", &state, "Button", None)
            .expect("Button should prepare");

        assert_eq!(
            prepared
                .name_resolution
                .get("ComponentConfig")
                .map(|id| (id.canonical_id.as_str(), id.symbol_name.as_str())),
            Some(("/src/tv.ts", "ComponentConfig"))
        );
        assert_eq!(
            prepared
                .name_resolution
                .get("AppConfig")
                .map(|id| (id.canonical_id.as_str(), id.symbol_name.as_str())),
            Some(("/src/schema.ts", "AppConfig"))
        );
        assert_eq!(
            prepared
                .name_resolution
                .get("theme")
                .map(|id| (id.canonical_id.as_str(), id.symbol_name.as_str())),
            Some(("/src/theme.ts", "default"))
        );
    }

    #[test]
    fn prepared_value_decl_falls_back_to_canonical_relative_targets_without_dep_edges() {
        let source = r#"
import type { Theme } from './theme.ts'

export const defaults: Theme = {} as Theme
"#;
        let analysis = make_analysis(source);
        let env = parse_and_build_env(source);
        let state = ShallowFileState::from_analysis(Hash16::default(), analysis, Some(&env));

        let prepared = prepare_exported_value_decl("/src/Button.vue", &state, "defaults", None)
            .expect("defaults should prepare");

        assert_eq!(
            prepared
                .name_resolution
                .get("Theme")
                .map(|id| (id.canonical_id.as_str(), id.symbol_name.as_str())),
            Some(("/src/theme.ts", "Theme"))
        );
    }

    #[test]
    fn does_not_prepare_reexport_without_frontier_routing() {
        let source = r#"export { Props } from "./inner""#;
        let analysis = make_analysis(source);
        let env = parse_and_build_env(source);
        let state = ShallowFileState::from_analysis(Hash16::default(), analysis, Some(&env));

        assert!(prepare_exported_type_decl("/src/barrel.ts", &state, "Props", None).is_none());
    }

    #[test]
    fn prepared_type_decl_populates_deps_from_shallow_symbol() {
        let source = r#"
import { Inner } from "./inner"
type Local = { x: number }
export interface Props { child: Inner; data: Local }
"#;
        let analysis = make_analysis(source);
        let env = parse_and_build_env(source);
        let state = ShallowFileState::from_analysis(Hash16::default(), analysis, Some(&env));

        let prepared = prepare_exported_type_decl("/src/types.ts", &state, "Props", None)
            .expect("Props should prepare");

        // Should have a member index for 'child' and 'data'
        assert!(
            prepared.member_index.contains_key("child"),
            "member index should contain 'child'"
        );
        assert!(
            prepared.member_index.contains_key("data"),
            "member index should contain 'data'"
        );
    }

    #[test]
    fn builds_local_prepared_decl_caches_from_shallow_file_state() {
        let source = r#"
export interface Props { label: string }
export const defaults: Props = { label: 'ok' }
"#;
        let analysis = make_analysis(source);
        let env = parse_and_build_env(source);
        let state = ShallowFileState::from_analysis(Hash16::default(), analysis, Some(&env));

        let type_cache = build_prepared_type_decl_cache(
            "/src/types.ts",
            Arc::new(state.clone()),
            Arc::new(FxHashMap::default()),
        );
        let value_cache = build_prepared_value_decl_cache(
            "/src/types.ts",
            Arc::new(state),
            Arc::new(FxHashMap::default()),
        );

        assert!(type_cache.contains_key("Props"));
        assert!(value_cache.contains_key("defaults"));
    }

    #[test]
    fn prepared_type_decl_build_counter_is_thread_local() {
        reset_prepared_type_decl_build_count_for_tests();

        let source = "export interface Props { label: string }";
        let analysis = make_analysis(source);
        let env = parse_and_build_env(source);
        let state = ShallowFileState::from_analysis(Hash16::default(), analysis, Some(&env));

        let prepared = prepare_exported_type_decl("/src/types.ts", &state, "Props", None)
            .expect("Props should prepare");
        assert_eq!(prepared.root_identity.symbol_name, "Props");
        assert_eq!(prepared_type_decl_build_count_for_tests(), 1);

        let other_thread_count = std::thread::spawn(prepared_type_decl_build_count_for_tests)
            .join()
            .expect("thread-local counter probe should join cleanly");
        assert_eq!(
            other_thread_count, 0,
            "prepared decl build counters should not leak across test threads",
        );
    }
}
