use std::sync::Arc;

use rustc_hash::FxHashMap;
use verter_semantic::analysis::type_solver::prepared::PreparedExternalDep;
use verter_semantic::analysis::type_solver::{
    PreparedTypeDecl, PreparedValueDecl, ResolvedRootIdentity,
};

use super::{ExportTarget, ShallowFileState};

/// Prepare a local type declaration from a canonical shallow file state.
///
/// Populates local_deps, external_deps from the shallow symbol, and
/// auto-builds the member index for object-like bodies.
pub fn prepare_local_type_decl(
    canonical_id: &str,
    state: &ShallowFileState,
    symbol_name: &str,
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
        .map(|dep| PreparedExternalDep {
            canonical_id: dep.source_specifier.clone(),
            symbol_name: dep.imported_name.clone(),
        })
        .collect();
    prepared.build_member_index();
    Some(prepared)
}

/// Prepare a named exported type declaration after routing has selected the
/// defining file.
pub fn prepare_exported_type_decl(
    canonical_id: &str,
    state: &ShallowFileState,
    exported_name: &str,
) -> Option<PreparedTypeDecl> {
    let ExportTarget::Local { symbol_name } = state.export_target(exported_name)? else {
        return None;
    };

    let mut prepared = prepare_local_type_decl(canonical_id, state, symbol_name)?;
    prepared.exported_name = Some(exported_name.to_string());
    Some(prepared)
}

/// Prepare a local value declaration from a canonical shallow file state.
pub fn prepare_local_value_decl(
    canonical_id: &str,
    state: &ShallowFileState,
    symbol_name: &str,
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
    Some(prepared)
}

/// Prepare a named exported value declaration after routing has selected the
/// defining file.
pub fn prepare_exported_value_decl(
    canonical_id: &str,
    state: &ShallowFileState,
    exported_name: &str,
) -> Option<PreparedValueDecl> {
    let ExportTarget::Local { symbol_name } = state.export_target(exported_name)? else {
        return None;
    };

    let mut prepared = prepare_local_value_decl(canonical_id, state, symbol_name)?;
    prepared.exported_name = Some(exported_name.to_string());
    Some(prepared)
}

/// Build the host-owned prepared type declaration cache for one defining file.
pub fn build_prepared_type_decl_cache(
    canonical_id: &str,
    state: &ShallowFileState,
) -> FxHashMap<String, Arc<PreparedTypeDecl>> {
    state
        .symbols
        .keys()
        .filter_map(|symbol_name| {
            prepare_local_type_decl(canonical_id, state, symbol_name)
                .map(|prepared| (symbol_name.clone(), Arc::new(prepared)))
        })
        .collect()
}

/// Build the host-owned prepared value declaration cache for one defining file.
pub fn build_prepared_value_decl_cache(
    canonical_id: &str,
    state: &ShallowFileState,
) -> FxHashMap<String, Arc<PreparedValueDecl>> {
    state
        .value_symbols
        .keys()
        .filter_map(|symbol_name| {
            prepare_local_value_decl(canonical_id, state, symbol_name)
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

        let prepared = prepare_exported_type_decl("/src/types.ts", &state, "Props")
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

        let prepared = prepare_exported_value_decl("/src/types.ts", &state, "defaults")
            .expect("defaults should prepare");

        assert_eq!(prepared.root_identity.canonical_id, "/src/types.ts");
        assert_eq!(prepared.root_identity.symbol_name, "defaults");
        assert_eq!(prepared.exported_name.as_deref(), Some("defaults"));
        assert_eq!(prepared.kind, ValueDeclKind::Const);
        assert!(prepared.type_annotation.is_some());
    }

    #[test]
    fn does_not_prepare_reexport_without_frontier_routing() {
        let source = r#"export { Props } from "./inner""#;
        let analysis = make_analysis(source);
        let env = parse_and_build_env(source);
        let state = ShallowFileState::from_analysis(Hash16::default(), analysis, Some(&env));

        assert!(prepare_exported_type_decl("/src/barrel.ts", &state, "Props").is_none());
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

        let prepared = prepare_exported_type_decl("/src/types.ts", &state, "Props")
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

        let type_cache = build_prepared_type_decl_cache("/src/types.ts", &state);
        let value_cache = build_prepared_value_decl_cache("/src/types.ts", &state);

        assert!(type_cache.contains_key("Props"));
        assert!(value_cache.contains_key("defaults"));
    }
}
