use std::sync::Arc;

use rustc_hash::FxHashMap;
use verter_semantic::analysis::type_solver::prepared::PreparedExternalDep;
use verter_semantic::analysis::type_solver::{
    PreparedTypeDecl, PreparedValueDecl, ResolvedRootIdentity,
};

use super::{ExportTarget, ShallowFileState};

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

/// Atomic declaration-surface bundle for one canonical file.
///
/// Valid as long as its `ExactResolution` and `FileWholeHash` facts match the
/// current store view. Never incrementally merged — rebuilt wholesale when the
/// import graph or file content changes.
#[derive(Clone)]
pub struct PreparedDeclBundle {
    pub prepared_type_decls: FxHashMap<String, Arc<PreparedTypeDecl>>,
    pub prepared_value_decls: FxHashMap<String, Arc<PreparedValueDecl>>,
    /// The dep_edges snapshot used to build this bundle.
    /// Stored so `SessionSolverHost::with_declaration_scope` can read it
    /// instead of recomputing `dependency_resolutions_for_eval_in_view`.
    pub dep_edges: FxHashMap<String, String>,
}

/// Build an atomic declaration-surface bundle from a shallow file state and
/// resolved dependency edges. The bundle is immutable after construction.
pub fn build_prepared_decl_bundle(
    canonical_id: &str,
    state: &ShallowFileState,
    dep_edges: FxHashMap<String, String>,
) -> PreparedDeclBundle {
    let dep_edges_ref = (!dep_edges.is_empty()).then_some(&dep_edges);
    PreparedDeclBundle {
        prepared_type_decls: build_prepared_type_decl_cache(canonical_id, state, dep_edges_ref),
        prepared_value_decls: build_prepared_value_decl_cache(canonical_id, state, dep_edges_ref),
        dep_edges,
    }
}

/// Build the host-owned prepared type declaration cache for one defining file.
pub fn build_prepared_type_decl_cache(
    canonical_id: &str,
    state: &ShallowFileState,
    dep_edges: Option<&FxHashMap<String, String>>,
) -> FxHashMap<String, Arc<PreparedTypeDecl>> {
    state
        .symbols
        .keys()
        .filter_map(|symbol_name| {
            prepare_local_type_decl(canonical_id, state, symbol_name, dep_edges)
                .map(|prepared| (symbol_name.clone(), Arc::new(prepared)))
        })
        .collect()
}

/// Build the host-owned prepared value declaration cache for one defining file.
pub fn build_prepared_value_decl_cache(
    canonical_id: &str,
    state: &ShallowFileState,
    dep_edges: Option<&FxHashMap<String, String>>,
) -> FxHashMap<String, Arc<PreparedValueDecl>> {
    state
        .value_symbols
        .keys()
        .filter_map(|symbol_name| {
            prepare_local_value_decl(canonical_id, state, symbol_name, dep_edges)
                .map(|prepared| (symbol_name.clone(), Arc::new(prepared)))
        })
        .collect()
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

        let type_cache = build_prepared_type_decl_cache("/src/types.ts", &state, None);
        let value_cache = build_prepared_value_decl_cache("/src/types.ts", &state, None);

        assert!(type_cache.contains_key("Props"));
        assert!(value_cache.contains_key("defaults"));
    }
}
