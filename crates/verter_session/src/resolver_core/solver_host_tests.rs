use super::*;
use rustc_hash::FxHashMap;
use std::sync::Arc;
use verter_semantic::analysis::type_solver::host::NoopSolverHost;
use verter_semantic::analysis::Hash16;

fn seed_ts_file(host: &VerterHost, canonical_id: &str, source: &str) {
    seed_ts_file_with_routes(host, canonical_id, source, FxHashMap::default());
}

fn seed_ts_file_with_routes(
    host: &VerterHost,
    canonical_id: &str,
    source: &str,
    import_routes: FxHashMap<String, crate::types::DependencyResolution>,
) {
    use verter_compiler::utils::oxc::vue::resolve_type::analyze_external_type_source;
    use verter_semantic::analysis::type_eval_build::parse_and_build_env;

    let allocator = oxc_allocator::Allocator::new();
    let analysis = Arc::new(analyze_external_type_source(source, &allocator));
    let env = parse_and_build_env(source);
    let state = Arc::new(crate::resolver_core::ShallowFileState::from_analysis(
        Hash16::default(),
        Arc::clone(&analysis),
        Some(&env),
    ));

    host.seed_module_facts_for_test(
        canonical_id,
        Hash16::default(),
        Arc::<str>::from(source),
        None,
        None,
        None,
        analysis,
        state,
        None,
        Some(Arc::<str>::from(source)),
        import_routes,
    );
}

#[test]
fn noop_host_returns_none() {
    let host = NoopSolverHost;
    let id = ResolvedRootIdentity::new("/t.ts", "T");
    assert!(host.resolve_prepared_type_decl(&id).is_none());
}

#[test]
fn session_host_without_env() {
    let host = VerterHost::new_standalone(Default::default());
    let solver_host = SessionSolverHost::new(&host, None);
    let id = ResolvedRootIdentity::new("/t.ts", "T");
    assert!(solver_host.resolve_prepared_type_decl(&id).is_none());
}

#[test]
fn declaration_scope_prefers_cached_prepared_decl_shape() {
    use verter_compiler::utils::oxc::vue::resolve_type::analyze_external_type_source;

    let host = VerterHost::new_standalone(Default::default());
    let source = r#"
import type { Inner } from "./dep"
export interface Props { child: Inner }
"#;
    let allocator = oxc_allocator::Allocator::new();
    let analysis = Arc::new(analyze_external_type_source(source, &allocator));
    let env = verter_semantic::analysis::type_eval_build::parse_and_build_env(source);
    let state = Arc::new(crate::resolver_core::ShallowFileState::from_analysis(
        Hash16::default(),
        Arc::clone(&analysis),
        Some(&env),
    ));
    host.seed_module_facts_for_test(
        "/decl.ts",
        Hash16::default(),
        Arc::<str>::from(source),
        None,
        None,
        None,
        analysis,
        state,
        None,
        Some(Arc::<str>::from(source)),
        FxHashMap::from_iter([(
            "./dep".to_string(),
            crate::types::DependencyResolution {
                specifier: "./dep".to_string(),
                resolved_canonical_id: Some("/dep.ts".to_string()),
                possible_canonical_ids: vec!["/dep.ts".to_string()],
            },
        )]),
    );

    let solver_host = SessionSolverHost::with_declaration_scope(&host, None, "/decl.ts");
    let id = ResolvedRootIdentity::new("/decl.ts", "Props");
    let decl = solver_host
        .resolve_prepared_type_decl(&id)
        .expect("declaration-scoped host should use cached prepared decls");
    assert_eq!(
            decl.name_resolution
                .get("Inner")
                .map(|identity| identity.canonical_id.as_str()),
            Some("/dep.ts"),
            "declaration-scoped solving should preserve cached name-resolution instead of rebuilding a local decl from EvalEnv",
        );
}

#[test]
fn declaration_scope_root_identity_resolves_same_file_symbols_and_imports() {
    use verter_compiler::utils::oxc::vue::resolve_type::analyze_external_type_source;
    use verter_semantic::analysis::Hash16;

    let host = VerterHost::new_standalone(Default::default());
    let source = r#"
import type { Theme } from "./theme"
export interface Props { theme: Theme }
export const defaults: Props = {} as Props
"#;
    let allocator = oxc_allocator::Allocator::new();
    let analysis = Arc::new(analyze_external_type_source(source, &allocator));
    let env = verter_semantic::analysis::type_eval_build::parse_and_build_env(source);
    let state = Arc::new(crate::resolver_core::ShallowFileState::from_analysis(
        Hash16::default(),
        Arc::clone(&analysis),
        Some(&env),
    ));
    host.seed_module_facts_for_test(
        "/decl.ts",
        Hash16::default(),
        Arc::<str>::from(source),
        None,
        None,
        None,
        analysis,
        state,
        None,
        Some(Arc::<str>::from(source)),
        FxHashMap::from_iter([(
            "./theme".to_string(),
            crate::types::DependencyResolution {
                specifier: "./theme".to_string(),
                resolved_canonical_id: Some("/theme.ts".to_string()),
                possible_canonical_ids: vec!["/theme.ts".to_string()],
            },
        )]),
    );

    let solver_host = SessionSolverHost::with_declaration_scope(&host, None, "/decl.ts");

    let props = solver_host
        .root_identity("", "Props")
        .expect("same-file type should resolve in declaration scope");
    assert_eq!(props.canonical_id, "/decl.ts");

    let defaults = solver_host
        .root_identity("", "defaults")
        .expect("same-file value should resolve in declaration scope");
    assert_eq!(defaults.canonical_id, "/decl.ts");

    let theme = solver_host
        .root_identity("", "Theme")
        .expect("import binding should resolve from declaration scope");
    assert_eq!(theme.canonical_id, "/theme.ts");
    assert_eq!(theme.symbol_name, "Theme");
}

#[test]
fn explicit_canonical_root_identity_resolves_import_bindings_from_shallow_state() {
    use verter_compiler::utils::oxc::vue::resolve_type::analyze_external_type_source;
    use verter_semantic::analysis::type_eval_build::parse_and_build_env;
    use verter_semantic::analysis::Hash16;

    let host = VerterHost::new_standalone(Default::default());
    let allocator = oxc_allocator::Allocator::new();

    let helper_source = "export type Prettify<T> = { [K in keyof T]: T[K] }";
    let helper_analysis = Arc::new(analyze_external_type_source(helper_source, &allocator));
    let helper_env = parse_and_build_env(helper_source);
    let helper_state = Arc::new(crate::resolver_core::ShallowFileState::from_analysis(
        Hash16::default(),
        Arc::clone(&helper_analysis),
        Some(&helper_env),
    ));
    host.seed_module_facts_for_test(
        "/helper.d.ts",
        Hash16::default(),
        Arc::<str>::from(helper_source),
        None,
        None,
        None,
        helper_analysis,
        helper_state,
        None,
        Some(Arc::<str>::from(helper_source)),
        FxHashMap::default(),
    );

    let decl_source = r#"
import { Prettify } from "./helper"
export type FancyProps = Prettify<{ open: boolean }>
"#;
    let decl_analysis = Arc::new(analyze_external_type_source(decl_source, &allocator));
    let decl_env = parse_and_build_env(decl_source);
    let decl_state = Arc::new(crate::resolver_core::ShallowFileState::from_analysis(
        Hash16::default(),
        Arc::clone(&decl_analysis),
        Some(&decl_env),
    ));

    host.seed_module_facts_for_test(
        "/decl.d.ts",
        Hash16::default(),
        Arc::<str>::from(decl_source),
        None,
        None,
        None,
        decl_analysis,
        decl_state,
        None,
        Some(Arc::<str>::from(decl_source)),
        FxHashMap::from_iter([(
            "./helper".to_string(),
            crate::types::DependencyResolution {
                specifier: "./helper".to_string(),
                resolved_canonical_id: Some("/helper.d.ts".to_string()),
                possible_canonical_ids: vec!["/helper.d.ts".to_string()],
            },
        )]),
    );

    let solver_host = SessionSolverHost::new(&host, None);
    let prettify = solver_host.root_identity("/decl.d.ts", "Prettify").expect(
        "explicit canonical lookups should resolve import bindings from cached shallow state",
    );

    assert_eq!(prettify.canonical_id, "/helper.d.ts");
    assert_eq!(prettify.symbol_name, "Prettify");
}

#[test]
fn explicit_canonical_root_identity_does_not_follow_uncached_import_bindings() {
    use verter_compiler::utils::oxc::vue::resolve_type::analyze_external_type_source;
    use verter_semantic::analysis::type_eval_build::parse_and_build_env;
    use verter_semantic::analysis::Hash16;

    let host = VerterHost::new_standalone(Default::default());
    let allocator = oxc_allocator::Allocator::new();

    let helper_source = "export type Prettify<T> = { [K in keyof T]: T[K] }";
    let helper_analysis = Arc::new(analyze_external_type_source(helper_source, &allocator));
    let helper_env = parse_and_build_env(helper_source);
    let helper_state = Arc::new(crate::resolver_core::ShallowFileState::from_analysis(
        Hash16::default(),
        Arc::clone(&helper_analysis),
        Some(&helper_env),
    ));
    host.seed_module_facts_for_test(
        "/helper.d.ts",
        Hash16::default(),
        Arc::<str>::from(helper_source),
        None,
        None,
        None,
        helper_analysis,
        helper_state,
        None,
        Some(Arc::<str>::from(helper_source)),
        FxHashMap::default(),
    );

    let decl_source = r#"
import { Prettify } from "./helper"
export type FancyProps = Prettify<{ open: boolean }>
"#;
    let decl_analysis = Arc::new(analyze_external_type_source(decl_source, &allocator));
    let decl_env = parse_and_build_env(decl_source);
    let decl_state = Arc::new(crate::resolver_core::ShallowFileState::from_analysis(
        Hash16::default(),
        Arc::clone(&decl_analysis),
        Some(&decl_env),
    ));

    host.seed_module_facts_for_test(
        "/decl.d.ts",
        Hash16::default(),
        Arc::<str>::from(decl_source),
        None,
        None,
        None,
        decl_analysis,
        decl_state,
        None,
        Some(Arc::<str>::from(decl_source)),
        FxHashMap::default(),
    );

    let solver_host = SessionSolverHost::new(&host, None);
    assert!(
        solver_host
            .root_identity("/decl.d.ts", "Prettify")
            .is_none(),
        "canonical-scoped root lookups must stay cache-only and refuse uncached import routing",
    );
}

#[test]
fn prepared_type_decl_lookup_routes_barrel_targets_before_cache_lookup() {
    use verter_compiler::utils::oxc::vue::resolve_type::analyze_external_type_source;
    use verter_semantic::analysis::type_eval_build::parse_and_build_env;
    use verter_semantic::analysis::Hash16;

    let host = VerterHost::new_standalone(Default::default());
    let allocator = oxc_allocator::Allocator::new();

    let barrel_source = "export { Props } from './props'";
    let barrel_analysis = Arc::new(analyze_external_type_source(barrel_source, &allocator));
    let barrel_state = Arc::new(crate::resolver_core::ShallowFileState::from_analysis(
        Hash16::default(),
        Arc::clone(&barrel_analysis),
        None,
    ));

    host.seed_module_facts_for_test(
        "/types/index.ts",
        Hash16::default(),
        Arc::<str>::from(barrel_source),
        None,
        None,
        None,
        barrel_analysis,
        barrel_state,
        None,
        Some(Arc::<str>::from(barrel_source)),
        FxHashMap::from_iter([(
            "./props".to_string(),
            crate::types::DependencyResolution {
                specifier: "./props".to_string(),
                resolved_canonical_id: Some("/types/props.ts".to_string()),
                possible_canonical_ids: vec!["/types/props.ts".to_string()],
            },
        )]),
    );

    let props_source = "export interface Props { label: string }";
    let props_analysis = Arc::new(analyze_external_type_source(props_source, &allocator));
    let props_env = parse_and_build_env(props_source);
    let props_state = Arc::new(crate::resolver_core::ShallowFileState::from_analysis(
        Hash16::default(),
        Arc::clone(&props_analysis),
        Some(&props_env),
    ));
    host.seed_module_facts_for_test(
        "/types/props.ts",
        Hash16::default(),
        Arc::<str>::from(props_source),
        None,
        None,
        None,
        props_analysis,
        props_state,
        None,
        Some(Arc::<str>::from(props_source)),
        FxHashMap::default(),
    );

    let root = host.resolve_imported_type_root_in_view("/types/index.ts", "Props", None);
    assert_eq!(
        root,
        ("/types/props.ts".to_string(), "Props".to_string()),
        "barrel root resolution should route to the defining declaration target",
    );
    assert!(
        host.prepared_type_decl_in_view("/types/props.ts", "Props", None)
            .is_some(),
        "the defining prepared decl should be available directly once the root resolves",
    );

    let solver_host = SessionSolverHost::new(&host, None);
    let prepared = solver_host
        .resolve_prepared_type_decl(&ResolvedRootIdentity::new("/types/index.ts", "Props"))
        .expect("barrel lookup should route to the defining prepared type decl");
    assert_eq!(prepared.root_identity.canonical_id, "/types/props.ts");
    assert_eq!(prepared.root_identity.symbol_name, "Props");
}

#[test]
fn declaration_scope_root_identity_routes_barrel_import_bindings_to_final_target() {
    use verter_compiler::utils::oxc::vue::resolve_type::analyze_external_type_source;
    use verter_semantic::analysis::type_eval_build::parse_and_build_env;
    use verter_semantic::analysis::Hash16;

    let host = VerterHost::new_standalone(Default::default());
    let allocator = oxc_allocator::Allocator::new();

    let barrel_source = "export { Props } from './props'";
    let barrel_analysis = Arc::new(analyze_external_type_source(barrel_source, &allocator));
    let barrel_state = Arc::new(crate::resolver_core::ShallowFileState::from_analysis(
        Hash16::default(),
        Arc::clone(&barrel_analysis),
        None,
    ));
    host.seed_module_facts_for_test(
        "/types/index.ts",
        Hash16::default(),
        Arc::<str>::from(barrel_source),
        None,
        None,
        None,
        barrel_analysis,
        barrel_state,
        None,
        Some(Arc::<str>::from(barrel_source)),
        FxHashMap::from_iter([(
            "./props".to_string(),
            crate::types::DependencyResolution {
                specifier: "./props".to_string(),
                resolved_canonical_id: Some("/types/props.ts".to_string()),
                possible_canonical_ids: vec!["/types/props.ts".to_string()],
            },
        )]),
    );

    let props_source = "export interface Props { label: string }";
    let props_analysis = Arc::new(analyze_external_type_source(props_source, &allocator));
    let props_env = parse_and_build_env(props_source);
    let props_state = Arc::new(crate::resolver_core::ShallowFileState::from_analysis(
        Hash16::default(),
        Arc::clone(&props_analysis),
        Some(&props_env),
    ));
    host.seed_module_facts_for_test(
        "/types/props.ts",
        Hash16::default(),
        Arc::<str>::from(props_source),
        None,
        None,
        None,
        props_analysis,
        props_state,
        None,
        Some(Arc::<str>::from(props_source)),
        FxHashMap::default(),
    );

    let owner_source = r#"
import type { Props } from "./types"
export interface OwnerProps {
  child: Props['label']
}
"#;
    let owner_analysis = Arc::new(analyze_external_type_source(owner_source, &allocator));
    let owner_env = parse_and_build_env(owner_source);
    let owner_state = Arc::new(crate::resolver_core::ShallowFileState::from_analysis(
        Hash16::default(),
        Arc::clone(&owner_analysis),
        Some(&owner_env),
    ));
    host.seed_module_facts_for_test(
        "/owner.ts",
        Hash16::default(),
        Arc::<str>::from(owner_source),
        None,
        None,
        None,
        owner_analysis,
        owner_state,
        None,
        Some(Arc::<str>::from(owner_source)),
        FxHashMap::from_iter([(
            "./types".to_string(),
            crate::types::DependencyResolution {
                specifier: "./types".to_string(),
                resolved_canonical_id: Some("/types/index.ts".to_string()),
                possible_canonical_ids: vec!["/types/index.ts".to_string()],
            },
        )]),
    );

    let solver_host = SessionSolverHost::with_declaration_scope(&host, None, "/owner.ts");
    let root = solver_host
        .root_identity("", "Props")
        .expect("barrel import binding should resolve in declaration scope");

    assert_eq!(
        root.canonical_id, "/types/props.ts",
        "root_identity should canonicalize barrel import bindings to the defining file"
    );
    assert_eq!(root.symbol_name, "Props");
}

#[test]
fn prepared_value_decl_lookup_routes_barrel_targets_before_cache_lookup() {
    let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    ws.inject_file(
        "/theme/index.ts".to_string(),
        Arc::from("export { theme } from './theme'"),
    );
    ws.inject_file(
        "/theme/theme.ts".to_string(),
        Arc::from("export const theme: { color: string } = { color: 'blue' }"),
    );

    let host = VerterHost::new(crate::HostConfig::default(), ws);
    host.set_import_dependencies(
        "/theme/index.ts",
        vec![crate::types::DependencyResolution {
            specifier: "./theme".to_string(),
            resolved_canonical_id: Some("/theme/theme.ts".to_string()),
            possible_canonical_ids: vec!["/theme/theme.ts".to_string()],
        }],
    );

    let solver_host = SessionSolverHost::new(&host, None);
    let prepared = solver_host
        .resolve_prepared_value_decl(&ResolvedRootIdentity::new("/theme/index.ts", "theme"))
        .expect("barrel lookup should route to the defining prepared value decl");
    assert_eq!(prepared.root_identity.canonical_id, "/theme/theme.ts");
    assert_eq!(prepared.root_identity.symbol_name, "theme");
}

#[test]
fn member_projection_chases_generic_alias_slots_through_helper_context() {
    use verter_compiler::utils::oxc::vue::resolve_type::analyze_external_type_source;
    use verter_semantic::analysis::type_eval_build::parse_and_build_env;
    use verter_semantic::analysis::type_expr::{
        LiteralValue, ObjectMember, PrimitiveName, TypeExpr,
    };
    use verter_semantic::analysis::type_solver::solve::solve_type_with_trace;
    use verter_semantic::analysis::Hash16;

    let host = VerterHost::new_standalone(Default::default());
    let allocator = oxc_allocator::Allocator::new();

    let config_source = r#"
export type Id<T> = {} & { [P in keyof T]: T[P] }
export type Theme = {
  slots: {
    item: string
  }
}
export type Noise = {
  boom: string
}
export type ComponentSlots<T extends { slots?: Record<string, any> }> = Id<T['slots']>
export type ComponentConfig<T extends { slots?: Record<string, any> }> = {
  slots: ComponentSlots<T>
}
"#;
    let config_analysis = Arc::new(analyze_external_type_source(config_source, &allocator));
    let config_env = parse_and_build_env(config_source);
    let config_state = Arc::new(crate::resolver_core::ShallowFileState::from_analysis(
        Hash16::default(),
        Arc::clone(&config_analysis),
        Some(&config_env),
    ));
    host.seed_module_facts_for_test(
        "/types/config.ts",
        Hash16::default(),
        Arc::<str>::from(config_source),
        None,
        None,
        None,
        config_analysis,
        config_state,
        None,
        Some(Arc::<str>::from(config_source)),
        FxHashMap::default(),
    );

    let consumer_source = r#"
import type { ComponentConfig, Theme } from './config'
export type CheckboxGroup = ComponentConfig<Theme>
"#;
    let consumer_analysis = Arc::new(analyze_external_type_source(consumer_source, &allocator));
    let consumer_env = parse_and_build_env(consumer_source);
    let consumer_state = Arc::new(crate::resolver_core::ShallowFileState::from_analysis(
        Hash16::default(),
        Arc::clone(&consumer_analysis),
        Some(&consumer_env),
    ));
    host.seed_module_facts_for_test(
        "/types/consumer.ts",
        Hash16::default(),
        Arc::<str>::from(consumer_source),
        None,
        None,
        None,
        consumer_analysis,
        consumer_state,
        None,
        Some(Arc::<str>::from(consumer_source)),
        FxHashMap::from_iter([(
            "./config".to_string(),
            crate::types::DependencyResolution {
                specifier: "./config".to_string(),
                resolved_canonical_id: Some("/types/config.ts".to_string()),
                possible_canonical_ids: vec!["/types/config.ts".to_string()],
            },
        )]),
    );

    let solver_host = SessionSolverHost::with_declaration_scope(&host, None, "/types/consumer.ts");

    let (solved, trace) = solve_type_with_trace(
        &TypeExpr::IndexedAccess {
            object: Arc::new(TypeExpr::named("CheckboxGroup")),
            index: Arc::new(TypeExpr::Literal(LiteralValue::String("slots".to_string()))),
        },
        &solver_host,
    );

    let TypeExpr::Object(slots) = solved.value else {
        panic!("expected object slots projection, got {:?}", solved.value);
    };
    let item = slots
        .properties
        .iter()
        .find_map(|member| match member {
            ObjectMember::Property(prop) if prop.name == "item" => Some(prop),
            _ => None,
        })
        .expect("slots projection should contain item");
    assert!(
        !item.optional,
        "fixture keeps the projected slot member required"
    );
    assert!(matches!(
        item.ty,
        TypeExpr::Primitive(PrimitiveName::String)
    ));
    assert!(
        !trace.iter().any(|identity| {
            identity.canonical_id == "/types/config.ts" && identity.symbol_name == "Noise"
        }),
        "solving CheckboxGroup['slots'] should stay on-route and never visit Noise"
    );
}

#[test]
fn cross_scope_same_name_different_imports_resolve_independently() {
    let host = VerterHost::new_standalone(Default::default());

    let source_a = r#"import type { Props } from "./a_types"
export interface Wrapper { inner: Props }"#;
    seed_ts_file_with_routes(
        &host,
        "/file_a.ts",
        source_a,
        FxHashMap::from_iter([(
            "./a_types".to_string(),
            crate::types::DependencyResolution {
                specifier: "./a_types".to_string(),
                resolved_canonical_id: Some("/a_types.ts".to_string()),
                possible_canonical_ids: vec!["/a_types.ts".to_string()],
            },
        )]),
    );

    let source_b = r#"import type { Props } from "./b_types"
export interface Container { child: Props }"#;
    seed_ts_file_with_routes(
        &host,
        "/file_b.ts",
        source_b,
        FxHashMap::from_iter([(
            "./b_types".to_string(),
            crate::types::DependencyResolution {
                specifier: "./b_types".to_string(),
                resolved_canonical_id: Some("/b_types.ts".to_string()),
                possible_canonical_ids: vec!["/b_types.ts".to_string()],
            },
        )]),
    );

    let scope_a = SessionSolverHost::with_declaration_scope(&host, None, "/file_a.ts");
    let scope_b = SessionSolverHost::with_declaration_scope(&host, None, "/file_b.ts");

    let resolved_a = scope_a
        .root_identity("", "Props")
        .expect("scope A should resolve Props");
    let resolved_b = scope_b
        .root_identity("", "Props")
        .expect("scope B should resolve Props");

    assert_eq!(resolved_a.canonical_id, "/a_types.ts");
    assert_eq!(resolved_b.canonical_id, "/b_types.ts");
    assert_ne!(resolved_a.canonical_id, resolved_b.canonical_id);
}

#[test]
fn miss_in_one_scope_does_not_poison_another() {
    let host = VerterHost::new_standalone(Default::default());

    seed_ts_file(
        &host,
        "/file_a.ts",
        r#"export interface Alpha { value: number }"#,
    );
    seed_ts_file(
        &host,
        "/file_b.ts",
        r#"export interface Theme { color: string }"#,
    );

    let scope_a = SessionSolverHost::with_declaration_scope(&host, None, "/file_a.ts");
    let scope_b = SessionSolverHost::with_declaration_scope(&host, None, "/file_b.ts");

    assert!(
        scope_a.root_identity("/file_a.ts", "Theme").is_none(),
        "scope A should miss on Theme"
    );

    let hit = scope_b
        .root_identity("/file_b.ts", "Theme")
        .expect("scope B should still resolve Theme after A missed");
    assert_eq!(hit.canonical_id, "/file_b.ts");
}

#[test]
fn namespace_member_lookups_stay_scope_correct() {
    let host = VerterHost::new_standalone(Default::default());

    seed_ts_file(
        &host,
        "/ns_a.ts",
        r#"export interface Member { a: string }"#,
    );
    seed_ts_file(
        &host,
        "/ns_b.ts",
        r#"export interface Member { b: number }"#,
    );

    let source_a = r#"import * as Ns from "./ns_a"
export interface WrapA { child: Ns.Member }"#;
    seed_ts_file_with_routes(
        &host,
        "/file_a.ts",
        source_a,
        FxHashMap::from_iter([(
            "./ns_a".to_string(),
            crate::types::DependencyResolution {
                specifier: "./ns_a".to_string(),
                resolved_canonical_id: Some("/ns_a.ts".to_string()),
                possible_canonical_ids: vec!["/ns_a.ts".to_string()],
            },
        )]),
    );

    let source_b = r#"import * as Ns from "./ns_b"
export interface WrapB { child: Ns.Member }"#;
    seed_ts_file_with_routes(
        &host,
        "/file_b.ts",
        source_b,
        FxHashMap::from_iter([(
            "./ns_b".to_string(),
            crate::types::DependencyResolution {
                specifier: "./ns_b".to_string(),
                resolved_canonical_id: Some("/ns_b.ts".to_string()),
                possible_canonical_ids: vec!["/ns_b.ts".to_string()],
            },
        )]),
    );

    let scope_a = SessionSolverHost::with_declaration_scope(&host, None, "/file_a.ts");
    let scope_b = SessionSolverHost::with_declaration_scope(&host, None, "/file_b.ts");

    let resolved_a = scope_a
        .root_identity("", "Ns.Member")
        .expect("scope A should resolve Ns.Member via ns_a");
    assert_eq!(resolved_a.canonical_id, "/ns_a.ts");
    assert_eq!(resolved_a.symbol_name, "Member");

    let resolved_b = scope_b
        .root_identity("", "Ns.Member")
        .expect("scope B should resolve Ns.Member via ns_b");
    assert_eq!(resolved_b.canonical_id, "/ns_b.ts");
    assert_eq!(resolved_b.symbol_name, "Member");
}

#[test]
fn prepared_decl_bundle_reuses_warm_cache() {
    let host = VerterHost::new_standalone(Default::default());

    let source = r#"import type { Dep } from "./dep"
export interface Props { value: Dep }"#;
    seed_ts_file_with_routes(
        &host,
        "/owner.ts",
        source,
        FxHashMap::from_iter([(
            "./dep".to_string(),
            crate::types::DependencyResolution {
                specifier: "./dep".to_string(),
                resolved_canonical_id: Some("/dep.ts".to_string()),
                possible_canonical_ids: vec!["/dep.ts".to_string()],
            },
        )]),
    );

    let bundle1 = host
        .prepared_decl_bundle_in_view("/owner.ts", None)
        .expect("bundle should materialize");
    assert!(
        !bundle1.dep_edges.is_empty(),
        "bundle should have dep_edges"
    );
    assert!(
        !bundle1.import_bindings.is_empty(),
        "bundle should have import_bindings"
    );

    let bundle2 = host
        .prepared_decl_bundle_in_view("/owner.ts", None)
        .expect("bundle should hit cache");
    assert!(
        Arc::ptr_eq(&bundle1, &bundle2),
        "repeated retrieval should return the same Arc"
    );
}

#[test]
fn imported_file_solving_works_without_dependency_maps() {
    let host = VerterHost::new_standalone(Default::default());

    seed_ts_file(
        &host,
        "/dep.ts",
        r#"export interface Inner { field: string }"#,
    );

    let source = r#"import type { Inner } from "./dep"
export interface Props { child: Inner }"#;
    seed_ts_file_with_routes(
        &host,
        "/imported.ts",
        source,
        FxHashMap::from_iter([(
            "./dep".to_string(),
            crate::types::DependencyResolution {
                specifier: "./dep".to_string(),
                resolved_canonical_id: Some("/dep.ts".to_string()),
                possible_canonical_ids: vec!["/dep.ts".to_string()],
            },
        )]),
    );

    let solver_host = SessionSolverHost::with_declaration_scope(&host, None, "/imported.ts");

    let props = solver_host
        .root_identity("", "Props")
        .expect("Props should resolve in declaration scope");
    assert_eq!(props.canonical_id, "/imported.ts");

    let inner = solver_host
        .root_identity("", "Inner")
        .expect("Inner should resolve through import bindings");
    assert_eq!(inner.canonical_id, "/dep.ts");
    assert_eq!(inner.symbol_name, "Inner");

    let bundle = host
        .prepared_decl_bundle_in_view("/imported.ts", None)
        .expect("bundle should exist");
    let props_decl = bundle
        .prepared_type_decls
        .get("Props")
        .expect("Props should be in prepared decls");
    let inner_resolution = props_decl
        .name_resolution
        .get("Inner")
        .expect("Props should have name_resolution for Inner");
    assert_eq!(inner_resolution.canonical_id, "/dep.ts");
}

#[test]
fn dep_edges_track_canonical_dependencies_from_module_facts() {
    let host = VerterHost::new_standalone(Default::default());

    let source = r#"import type { A } from "./a"
import type { B } from "./b"
export interface Props { a: A, b: B }"#;
    seed_ts_file_with_routes(
        &host,
        "/component.ts",
        source,
        FxHashMap::from_iter([
            (
                "./a".to_string(),
                crate::types::DependencyResolution {
                    specifier: "./a".to_string(),
                    resolved_canonical_id: Some("/a.ts".to_string()),
                    possible_canonical_ids: vec!["/a.ts".to_string()],
                },
            ),
            (
                "./b".to_string(),
                crate::types::DependencyResolution {
                    specifier: "./b".to_string(),
                    resolved_canonical_id: Some("/b.ts".to_string()),
                    possible_canonical_ids: vec!["/b.ts".to_string()],
                },
            ),
        ]),
    );

    let bundle = host
        .prepared_decl_bundle_in_view("/component.ts", None)
        .expect("bundle should materialize");

    assert_eq!(
        bundle.dep_edges.get("./a").map(String::as_str),
        Some("/a.ts"),
        "dep_edges should track ./a -> /a.ts"
    );
    assert_eq!(
        bundle.dep_edges.get("./b").map(String::as_str),
        Some("/b.ts"),
        "dep_edges should track ./b -> /b.ts"
    );

    assert!(
        bundle.import_bindings.contains_key("A"),
        "import_bindings should contain A"
    );
    assert!(
        bundle.import_bindings.contains_key("B"),
        "import_bindings should contain B"
    );
    assert_eq!(
        bundle
            .import_bindings
            .get("A")
            .map(|binding| binding.canonical_id.as_str()),
        Some("/a.ts")
    );
    assert_eq!(
        bundle
            .import_bindings
            .get("B")
            .map(|binding| binding.canonical_id.as_str()),
        Some("/b.ts")
    );
}
