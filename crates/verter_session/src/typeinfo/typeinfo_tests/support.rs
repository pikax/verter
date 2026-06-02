//! @ai-generated - Shared host setup and assertions for synthetic
//! component-shaped typeinfo tests.

use std::collections::BTreeMap;

pub(super) use std::sync::Arc;
pub(super) use verter_audit::ProjectionModeTag;
use verter_audit::RequestKindPayload;
use verter_type_expr::{FunctionExpr, IndexSignature, LiteralValue, ObjectMember, ObjectProperty};
pub(super) use verter_type_expr::{PrimitiveName, TypeExpr};

pub(super) use super::super::types::{EvaluateTypeExpressionRequest, ImportSpec, NamedImport};
pub(super) use crate::semantic_query::ProjectionMode;
use crate::types::{FileKind, HostConfig, UpsertRequest};
use crate::VerterHost;

pub(super) const COMPONENT_TYPES: &str = include_str!("fixtures/component_types.ts");
pub(super) const SCOPE_TYPES: &str = include_str!("fixtures/scope.ts");
pub(super) const FOOTPRINT_NEEDED: &str = include_str!("fixtures/footprint_needed.ts");
pub(super) const FOOTPRINT_OWNER: &str = include_str!("fixtures/footprint_owner.ts");
pub(super) const FOOTPRINT_UNUSED: &str = include_str!("fixtures/footprint_unused.ts");
pub(super) const DEEP_PATH: &str = include_str!("fixtures/deep_path.ts");
pub(super) const MENU_LIKE: &str = include_str!("fixtures/menu_like.ts");
pub(super) const MESSAGE_LIST_LIKE: &str = include_str!("fixtures/message_list_like.ts");
pub(super) const TABLE_LIKE: &str = include_str!("fixtures/table_like.ts");
pub(super) const CONDITIONAL_INFER: &str = include_str!("fixtures/conditional_infer.ts");
pub(super) const CROSS_FILE_BARREL: &str = include_str!("fixtures/cross_file_barrel.ts");
pub(super) const CROSS_FILE_CONSUMER: &str = include_str!("fixtures/cross_file_consumer.ts");
pub(super) const CROSS_FILE_LEAF: &str = include_str!("fixtures/cross_file_leaf.ts");
pub(super) const CROSS_FILE_UNUSED: &str = include_str!("fixtures/cross_file_unused.ts");
pub(super) const DEMAND_BARREL: &str = include_str!("fixtures/demand_barrel.ts");
pub(super) const DEMAND_NEEDED: &str = include_str!("fixtures/demand_needed.ts");
pub(super) const DEMAND_OWNER: &str = include_str!("fixtures/demand_owner.ts");
pub(super) const DEMAND_UNUSED: &str = include_str!("fixtures/demand_unused.ts");
pub(super) const GENERIC_DEFAULTS: &str = include_str!("fixtures/generic_defaults.ts");
pub(super) const INDEXED_UTILITIES: &str = include_str!("fixtures/indexed_utilities.ts");
pub(super) const MAPPED_TEMPLATE: &str = include_str!("fixtures/mapped_template.ts");
pub(super) const RECURSIVE_UNION: &str = include_str!("fixtures/recursive_union.ts");
pub(super) const UTILITY_COMPOSITION: &str = include_str!("fixtures/utility_composition.ts");
pub(super) const WIDE_DEEP: &str = include_str!("fixtures/wide_deep.ts");
pub(super) const TYPESCRIPT_RULES: &str = include_str!("fixtures/typescript_rules.ts");
pub(super) const EXPANSION_OWNER: &str = include_str!("fixtures/expansion_owner.ts");
pub(super) const EXPANSION_SELECTED: &str = include_str!("fixtures/expansion_selected.ts");
pub(super) const EXPANSION_UNSELECTED: &str = include_str!("fixtures/expansion_unselected.ts");
pub(super) const VALUE_INFERENCE: &str = include_str!("fixtures/value_inference.ts");
pub(super) const FLOW_RETURN_CATALOG: &str = include_str!("fixtures/flow_return_catalog.ts");
pub(super) const FLOW_RETURN_EDGE_CATALOG: &str =
    include_str!("fixtures/flow_return_edge_catalog.ts");
pub(super) const FLOW_RETURN_EDGE_CROSS: &str = include_str!("fixtures/flow_return_edge_cross.ts");
pub(super) const FLOW_RETURN_PATH_OWNER: &str = include_str!("fixtures/flow_return_path_owner.ts");
pub(super) const FLOW_RETURN_PATH_BARREL: &str =
    include_str!("fixtures/flow_return_path_barrel.ts");
pub(super) const FLOW_RETURN_PATH_SELECTED: &str =
    include_str!("fixtures/flow_return_path_selected.ts");
pub(super) const FLOW_RETURN_PATH_ALTERNATE: &str =
    include_str!("fixtures/flow_return_path_alternate.ts");
pub(super) const FLOW_RETURN_PATH_UNUSED: &str =
    include_str!("fixtures/flow_return_path_unused.ts");
pub(super) const FLOW_RETURN_PARITY_CATALOG: &str =
    include_str!("fixtures/flow_return_parity_catalog.ts");
pub(super) const FLOW_RETURN_PARITY_AUG_OWNER: &str =
    include_str!("fixtures/flow_return_parity_aug_owner.ts");
pub(super) const FLOW_RETURN_PARITY_AUG_BARREL: &str =
    include_str!("fixtures/flow_return_parity_aug_barrel.ts");
pub(super) const FLOW_RETURN_PARITY_AUG_BASE: &str =
    include_str!("fixtures/flow_return_parity_aug_base.ts");
pub(super) const FLOW_RETURN_PARITY_AUG_PATCH: &str =
    include_str!("fixtures/flow_return_parity_aug_patch.ts");
pub(super) const FLOW_RETURN_PARITY_AUG_UNUSED: &str =
    include_str!("fixtures/flow_return_parity_aug_unused.ts");
pub(super) const FLOW_RETURN_CROSS_TYPES: &str =
    include_str!("fixtures/flow_return_cross_types.ts");
pub(super) const FLOW_RETURN_CROSS_FACTORY: &str =
    include_str!("fixtures/flow_return_cross_factory.ts");
pub(super) const FLOW_RETURN_CROSS_GUARDS: &str =
    include_str!("fixtures/flow_return_cross_guards.ts");
pub(super) const FLOW_RETURN_CROSS_SOURCE: &str =
    include_str!("fixtures/flow_return_cross_source.ts");
pub(super) const FLOW_RETURN_CROSS_INDEX: &str =
    include_str!("fixtures/flow_return_cross_index.ts");
pub(super) const FLOW_RETURN_CROSS_MAIN: &str = include_str!("fixtures/flow_return_cross_main.ts");
pub(super) const FLOW_RETURN_CROSS_PACKAGE_MAIN: &str =
    include_str!("fixtures/flow_return_cross_package_main.ts");
pub(super) const FLOW_RETURN_PACKAGE_DECLARATIONS: &str =
    include_str!("fixtures/flow_return_package_declarations.ts");
pub(super) const FLOW_RETURN_EDGE_PACKAGE_DECLARATIONS: &str =
    include_str!("fixtures/flow_return_edge_package_declarations.ts");

pub(super) fn make_host_with_footprint() -> Arc<VerterHost> {
    Arc::new(VerterHost::new_standalone(HostConfig {
        audit_enabled: true,
        footprint_capture: true,
        ..HostConfig::default()
    }))
}

pub(super) fn make_host_with_workspace_files_footprint(files: &[(&str, &str)]) -> Arc<VerterHost> {
    let workspace = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    for (path, source) in files {
        workspace.inject_file((*path).to_string(), Arc::from(*source));
    }
    let host = Arc::new(VerterHost::new(
        HostConfig {
            audit_enabled: true,
            footprint_capture: true,
            ..HostConfig::default()
        },
        workspace,
    ));
    host.configure_projects(vec![
        verter_semantic::analysis::project_resolver::IdeProjectConfig::new(
            "/workspace".to_string(),
            "/workspace".to_string(),
            Some("/workspace/tsconfig.json".to_string()),
        ),
    ]);
    host
}

pub(super) fn upsert_ts(host: &VerterHost, canonical_id: &str, source: &str) {
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some(canonical_id.to_string()),
        input_id: canonical_id.to_string(),
        source: Arc::from(source),
        file_kind: FileKind::from_path(canonical_id),
        aliases: Vec::new(),
    });
}

pub(super) fn resolve_expr(
    host: &VerterHost,
    canonical_id: &str,
    name: &str,
    type_args: &[Arc<TypeExpr>],
    mode: ProjectionMode,
) -> (TypeExpr, verter_audit::RequestAuditRecord) {
    let (node, record) =
        host.resolve_named_symbol_with_audit(canonical_id, name, type_args, Some(mode));
    let expr = host
        .project_node_to_type_expr(node.unwrap_or_else(|| panic!("{name} must resolve")))
        .unwrap_or_else(|| panic!("{name} resolved node must project to TypeExpr"));
    (expr, record.expect("typeinfo request emits audit"))
}

/// Resolve `name` to its carrier node (in `Navigate`, so heritage /
/// intersection arms stay un-merged carriers) and then run the EMPTY-PATH
/// `Shallow` `ProjectPath` terminal-surface synthesiser on it, projecting the
/// synthesised one-level surface to a [`TypeExpr`].
///
/// This is the typeinfo-primary exercise for the empty-path Shallow
/// projection (`expand_empty_path_shallow_terminal_surface`) — the load-bearing
/// path the unification stage makes carry the COMPLETE surface fact set and the
/// heritage-vs-authored merge. `resolve_named_symbol` alone dispatches
/// `Instantiate` (not an empty-path `ProjectPath`), so it does NOT exercise the
/// synthesiser; this helper does.
pub(super) fn shallow_surface_expr(host: &VerterHost, canonical_id: &str, name: &str) -> TypeExpr {
    use crate::project_semantic_dispatch::ProjectSemanticDispatch;
    use crate::semantic_query::{
        ProjectionReductionContext, ResolveDeclKey, ScopeId, SemanticQueryApi, SemanticQueryKey,
    };

    let store_view = host.resolver_store_view();
    let overlay = Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
    let host_ctx = crate::resolver_core::HostResolverContext::new(host, &store_view, overlay);
    let dispatch = ProjectSemanticDispatch::new(&host_ctx);

    // Base = the declaration CARRIER (a `DeclPlaceholder`), NOT a
    // pre-instantiated body. The empty-path Shallow synthesiser's decl-root
    // unwrap re-establishes the consuming declaration's KIND (interface/class
    // vs alias) and classifies its heritage arms — exactly the base shape the
    // U3 `resolve_surface_view` replacement consumes. Resolving via
    // `ResolveDecl` (rather than `resolve_named_symbol`, which instantiates)
    // keeps that carrier intact.
    let base = match dispatch.execute(SemanticQueryKey::ResolveDecl(ResolveDeclKey {
        scope: ScopeId {
            canonical_id: Arc::from(canonical_id),
            local_scope: None,
        },
        name: Arc::from(name),
    })) {
        crate::semantic_query::QueryResult::Value(node) => node,
        other => panic!("{name} must resolve to a declaration carrier: {other:?}"),
    };

    let terminal = match dispatch.execute(SemanticQueryKey::ProjectPath {
        base,
        path: Arc::from(Vec::new().into_boxed_slice()),
        context: ProjectionReductionContext::published(ProjectionMode::Shallow),
    }) {
        crate::semantic_query::QueryResult::Value(node) => node,
        other => panic!("empty-path Shallow projection of {name} failed: {other:?}"),
    };
    dispatch
        .raise_node_to_type_expr(terminal)
        .unwrap_or_else(|| panic!("{name} empty-path Shallow surface must project to TypeExpr"))
}

pub(super) fn evaluate_expr(
    host: &VerterHost,
    scope: &str,
    expression: &str,
    mode: ProjectionMode,
) -> (TypeExpr, verter_audit::RequestAuditRecord) {
    let (node, record) = host.evaluate_type_expression_with_audit(EvaluateTypeExpressionRequest {
        scope: scope.to_string(),
        expression: expression.to_string(),
        extra_imports: Vec::new(),
        mode,
        cacheable: false,
    });
    let expr = host
        .project_node_to_type_expr(node.unwrap_or_else(|| panic!("{expression} must resolve")))
        .unwrap_or_else(|| panic!("{expression} resolved node must project to TypeExpr"));
    (expr, record.expect("typeinfo request emits audit"))
}

pub(super) fn assert_query_mode(
    record: &verter_audit::RequestAuditRecord,
    mode: ProjectionModeTag,
) {
    match &record.kind_payload {
        RequestKindPayload::TypeResolution(payload) => {
            assert_eq!(payload.query_mode, mode);
        }
        other => panic!("expected TypeResolution payload, got {other:?}"),
    }
}

pub(super) fn object_props(expr: &TypeExpr) -> BTreeMap<String, ObjectProperty> {
    let mut props = BTreeMap::new();
    collect_object_props(expr, &mut props);
    props
}

fn collect_object_props(expr: &TypeExpr, props: &mut BTreeMap<String, ObjectProperty>) {
    match expr {
        TypeExpr::Object(object) => {
            for member in &object.properties {
                match member {
                    ObjectMember::Property(prop) => {
                        props.insert(prop.name.clone(), prop.clone());
                    }
                    ObjectMember::Method(method) => {
                        props.insert(
                            method.name.clone(),
                            ObjectProperty::synthetic_public(
                                method.name.clone(),
                                TypeExpr::Function(Arc::new(method.function.clone())),
                                method.optional,
                                false,
                            ),
                        );
                    }
                    ObjectMember::IndexSignature(_)
                    | ObjectMember::CallSignature(_)
                    | ObjectMember::ConstructSignature(_) => {}
                }
            }
        }
        TypeExpr::Intersection(parts) => {
            for part in parts.iter() {
                collect_object_props(part, props);
            }
        }
        other => panic!("expected object type, got {other:?}"),
    }
}

pub(super) fn prop_names(props: &BTreeMap<String, ObjectProperty>) -> Vec<&str> {
    props.keys().map(String::as_str).collect()
}

pub(super) fn object_index_signatures(expr: &TypeExpr) -> Vec<IndexSignature> {
    let TypeExpr::Object(object) = expr else {
        panic!("expected object type, got {expr:?}");
    };
    object
        .properties
        .iter()
        .filter_map(|member| match member {
            ObjectMember::IndexSignature(sig) => Some(sig.clone()),
            _ => None,
        })
        .collect()
}

/// Call signatures (`(args): ret`) carried on an object surface. Discriminates
/// the empty-path Shallow projection's call-signature carriage — the prior
/// projection dropped these.
pub(super) fn object_call_signatures(expr: &TypeExpr) -> Vec<FunctionExpr> {
    let TypeExpr::Object(object) = expr else {
        panic!("expected object type, got {expr:?}");
    };
    object
        .properties
        .iter()
        .filter_map(|member| match member {
            ObjectMember::CallSignature(function) => Some(function.clone()),
            _ => None,
        })
        .collect()
}

/// Construct signatures (`new (args): ret`) carried on an object surface.
pub(super) fn object_construct_signatures(expr: &TypeExpr) -> Vec<FunctionExpr> {
    let TypeExpr::Object(object) = expr else {
        panic!("expected object type, got {expr:?}");
    };
    object
        .properties
        .iter()
        .filter_map(|member| match member {
            ObjectMember::ConstructSignature(function) => Some(function.clone()),
            _ => None,
        })
        .collect()
}

pub(super) fn array_element(expr: &TypeExpr) -> &TypeExpr {
    match expr {
        TypeExpr::Array { element, .. } => element,
        other => panic!("expected array type, got {other:?}"),
    }
}

pub(super) fn assert_primitive(expr: &TypeExpr, expected: PrimitiveName) {
    assert_eq!(expr, &TypeExpr::Primitive(expected));
}

pub(super) fn assert_ref(expr: &TypeExpr, expected_name: &str) {
    match expr {
        TypeExpr::Ref {
            name,
            type_arguments: _,
        } => {
            assert_eq!(name.as_ref(), expected_name);
        }
        TypeExpr::RecursiveRef {
            name,
            type_arguments: _,
            conditional_context: _,
        } => {
            assert_eq!(name.as_ref(), expected_name);
        }
        other => panic!("expected ref {expected_name}, got {other:?}"),
    }
}

pub(super) fn assert_array_of_primitive(expr: &TypeExpr, expected: PrimitiveName) {
    match expr {
        TypeExpr::Array { element, .. } => assert_primitive(element, expected),
        other => panic!("expected array of {expected:?}, got {other:?}"),
    }
}

pub(super) fn assert_array_of_ref(expr: &TypeExpr, expected_name: &str) {
    match expr {
        TypeExpr::Array { element, .. } => assert_ref(element, expected_name),
        other => panic!("expected array of ref {expected_name}, got {other:?}"),
    }
}

pub(super) fn assert_recursive_ref(expr: &TypeExpr, expected_name: &str) {
    match expr {
        TypeExpr::RecursiveRef {
            name,
            type_arguments: _,
            conditional_context: _,
        } => {
            assert_eq!(name.as_ref(), expected_name);
        }
        other => panic!("expected RecursiveRef {expected_name}, got {other:?}"),
    }
}

pub(super) fn assert_array_of_recursive_ref(expr: &TypeExpr, expected_name: &str) {
    match expr {
        TypeExpr::Array { element, .. } => assert_recursive_ref(element, expected_name),
        other => panic!("expected array of RecursiveRef {expected_name}, got {other:?}"),
    }
}

pub(super) fn assert_literal_union(expr: &TypeExpr, expected: &[&str]) {
    let TypeExpr::Union(types) = expr else {
        panic!("expected literal union, got {expr:?}");
    };
    let mut actual: Vec<&str> = types
        .iter()
        .map(|ty| match ty {
            TypeExpr::Literal(LiteralValue::String(value)) => value.as_str(),
            other => panic!("expected string literal arm, got {other:?}"),
        })
        .collect();
    actual.sort_unstable();
    let mut expected = expected.to_vec();
    expected.sort_unstable();
    assert_eq!(actual, expected);
}

pub(super) fn assert_union_has_object_arm(expr: &TypeExpr, expected_props: &[&str]) {
    let TypeExpr::Union(types) = expr else {
        panic!("expected union with object arm, got {expr:?}");
    };
    let mut expected = expected_props.to_vec();
    expected.sort_unstable();
    let found = types.iter().any(|ty| {
        let (TypeExpr::Object(_) | TypeExpr::Intersection(_)) = ty else {
            return false;
        };
        prop_names(&object_props(ty)) == expected
    });
    assert!(
        found,
        "expected union {expr:?} to contain object arm with props {expected:?}"
    );
}

pub(super) fn assert_number_literal_union(expr: &TypeExpr, expected: &[f64]) {
    let TypeExpr::Union(types) = expr else {
        panic!("expected number literal union, got {expr:?}");
    };
    let mut actual: Vec<u64> = types
        .iter()
        .map(|ty| match ty {
            TypeExpr::Literal(LiteralValue::Number(value)) => value.to_bits(),
            other => panic!("expected number literal arm, got {other:?}"),
        })
        .collect();
    actual.sort_unstable();
    let mut expected: Vec<u64> = expected.iter().map(|value| value.to_bits()).collect();
    expected.sort_unstable();
    assert_eq!(actual, expected);
}

pub(super) fn assert_union_contains_primitive(expr: &TypeExpr, expected: PrimitiveName) {
    let TypeExpr::Union(types) = expr else {
        panic!("expected union containing {expected:?}, got {expr:?}");
    };
    assert!(
        types
            .iter()
            .any(|ty| matches!(ty, TypeExpr::Primitive(name) if *name == expected)),
        "expected union {expr:?} to contain {expected:?}"
    );
}

pub(super) fn assert_expr_contains_primitive(expr: &TypeExpr, expected: PrimitiveName) {
    let contains = match expr {
        TypeExpr::Primitive(name) => *name == expected,
        TypeExpr::Union(types) | TypeExpr::Intersection(types) => types
            .iter()
            .any(|ty| matches!(ty, TypeExpr::Primitive(name) if *name == expected)),
        _ => false,
    };
    assert!(
        contains,
        "expected expression {expr:?} to contain primitive {expected:?}"
    );
}

pub(super) fn assert_string_literal(expr: &TypeExpr, expected: &str) {
    match expr {
        TypeExpr::Literal(LiteralValue::String(value)) => assert_eq!(value, expected),
        other => panic!("expected string literal {expected:?}, got {other:?}"),
    }
}

pub(super) fn assert_number_literal(expr: &TypeExpr, expected: f64) {
    match expr {
        TypeExpr::Literal(LiteralValue::Number(value)) => {
            assert_eq!(value.to_bits(), expected.to_bits())
        }
        other => panic!("expected number literal {expected:?}, got {other:?}"),
    }
}

pub(super) fn assert_boolean_literal(expr: &TypeExpr, expected: bool) {
    match expr {
        TypeExpr::Literal(LiteralValue::Boolean(value)) => assert_eq!(*value, expected),
        other => panic!("expected boolean literal {expected:?}, got {other:?}"),
    }
}

pub(super) fn function_type(expr: &TypeExpr) -> &FunctionExpr {
    match expr {
        TypeExpr::Function(function) => function,
        other => panic!("expected function type, got {other:?}"),
    }
}

pub(super) fn loaded_file_names(host: &VerterHost) -> Vec<String> {
    host.audit()
        .loaded_files()
        .iter()
        .map(|path| path.to_string())
        .collect()
}

pub(super) fn request_loaded_file_names(record: &verter_audit::RequestAuditRecord) -> Vec<String> {
    record
        .footprint
        .as_ref()
        .expect("typeinfo requests must attach a footprint when footprint_capture=true")
        .loaded_files()
        .iter()
        .map(|path| path.to_string())
        .collect()
}

pub(super) fn declared_dependency_file_names(
    record: &verter_audit::RequestAuditRecord,
) -> Vec<String> {
    record
        .footprint
        .as_ref()
        .expect("typeinfo requests must attach a footprint when footprint_capture=true")
        .declared_dependency_files()
        .iter()
        .map(|path| path.to_string())
        .collect()
}

pub(super) fn assert_declared_dependency_includes(
    record: &verter_audit::RequestAuditRecord,
    canonical_id: &str,
) {
    let declared = declared_dependency_file_names(record);
    assert!(
        declared.iter().any(|path| path == canonical_id),
        "{canonical_id} must enter the typeinfo dependency footprint; got {declared:?}"
    );
}

pub(super) fn assert_declared_dependency_excludes(
    record: &verter_audit::RequestAuditRecord,
    canonical_id: &str,
) {
    let declared = declared_dependency_file_names(record);
    assert!(
        !declared.iter().any(|path| path == canonical_id),
        "{canonical_id} must not enter the typeinfo dependency footprint; got {declared:?}"
    );
}

pub(super) fn assert_loaded_files_include(host: &VerterHost, canonical_id: &str) {
    let loaded = loaded_file_names(host);
    assert!(
        loaded.iter().any(|path| path == canonical_id),
        "{canonical_id} must be loaded by the request; got {loaded:?}"
    );
}

pub(super) fn assert_loaded_files_exclude(host: &VerterHost, canonical_id: &str) {
    let loaded = loaded_file_names(host);
    assert!(
        !loaded.iter().any(|path| path == canonical_id),
        "{canonical_id} must not be loaded by the request; got {loaded:?}"
    );
}

pub(super) fn assert_request_loaded_files_exclude(
    record: &verter_audit::RequestAuditRecord,
    canonical_id: &str,
) {
    let loaded = request_loaded_file_names(record);
    assert!(
        !loaded.iter().any(|path| path == canonical_id),
        "{canonical_id} must not be loaded by the request; got {loaded:?}"
    );
}

pub(super) fn assert_no_fresh_source_loading(record: &verter_audit::RequestAuditRecord) {
    let footprint = record
        .footprint
        .as_ref()
        .expect("typeinfo requests must attach a footprint when footprint_capture=true");
    assert!(
        footprint.vfs_reads.is_empty(),
        "warm typeinfo rerun must not trigger VFS reads; got {:?}",
        footprint.vfs_reads
    );
    assert!(
        footprint.indexed_ready_builds.is_empty(),
        "warm typeinfo rerun must not rebuild IndexedReady entries; got {:?}",
        footprint.indexed_ready_builds
    );
    assert!(
        footprint.shared_load_reuses.is_empty(),
        "single-threaded warm typeinfo rerun must not join in-flight source loads; got {:?}",
        footprint.shared_load_reuses
    );
}

pub(super) fn assert_no_route_misses(record: &verter_audit::RequestAuditRecord) {
    let layers = &record.store.cache_layers;
    assert_eq!(
        layers.route_db.misses, 0,
        "warm typeinfo rerun must not perform cold RouteDb work"
    );
    assert_eq!(
        layers.route_owned_shallow.misses, 0,
        "warm typeinfo rerun must not perform cold route-owned shallow work"
    );
    assert_eq!(
        layers.owner_import.misses, 0,
        "warm typeinfo rerun must not perform cold owner-import route work"
    );
}
