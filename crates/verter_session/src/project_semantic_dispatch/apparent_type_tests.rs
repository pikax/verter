//! @ai-generated - Rootless-callable apparent-type scoping and admission
//! tests.
//!
//! A ROOTLESS callable (a parameter annotation, a local arrow — a
//! `Signature` with no authored occurrence) has no declaring canonical, so
//! its ambient `Function` lookup is scoped by the LEXICAL DEMAND canonical
//! carried in the `ApparentType` key's demand-scope witness. These tests
//! pin the three admission facts of that arm:
//!
//! 1. project-correct resolution — the same interned rootless node widens
//!    to DIFFERENT ambient surfaces under two projects' demand scopes, and
//!    neither serves the other's value;
//! 2. shared-cache suppression — a rootless apparent value, and every
//!    enclosing member projection that read it, is `cache_suppress` and
//!    never enters the family memo;
//! 3. fail-closed without ambient proof — no registered ambient corpus
//!    means `Miss`, never a fabricated surface.

use std::sync::Arc;

use super::ProjectSemanticDispatch;
use crate::semantic_query::{
    ApparentDemandScope, FunctionParam, PrimitiveKind, ProjectionMode, QueryError, QueryResult,
    SemanticNodeData, SemanticNodeId, SemanticQueryKey, SignatureKind, SignatureReturnCarrier,
};
use crate::types::{HostConfig, UpsertRequest};
use crate::VerterHost;

/// A `Function` ambient corpus whose surface carries a project-A-only
/// marker member.
const AMBIENT_LIB_A: &str = r#"
interface Function {
  call(this: Function, thisArg: any, ...argArray: any[]): any;
  onlyA(): "a";
}
"#;

/// A `Function` ambient corpus whose surface carries a project-B-only
/// marker member.
const AMBIENT_LIB_B: &str = r#"
interface Function {
  call(this: Function, thisArg: any, ...argArray: any[]): any;
  onlyB(): "b";
}
"#;

fn project_config(root: &str) -> verter_workspace::VfsProjectConfig {
    verter_workspace::VfsProjectConfig {
        root: root.to_string(),
        rank: verter_workspace::ProjectRank::Explicit,
        tsconfig_path: Some(format!("{root}/tsconfig.json")),
        root_files: vec![],
        extensions: vec![".ts".into(), ".d.ts".into()],
        workspace_root: root.to_string(),
        workspace_aliases: vec![],
        compiler_options: verter_workspace::IdeProjectCompilerOptions::default(),
        references: vec![],
        membership: verter_workspace::ConfiguredMembership::match_all_under_root(
            &verter_workspace::CanonicalPath::new(root),
        ),
    }
}

fn upsert_ts(host: &VerterHost, canonical: &str, source: &str) {
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: Some(canonical.to_string()),
            input_id: canonical.to_string(),
            source: Arc::from(source),
            file_language: crate::FileLanguage::script_ts(),
            aliases: Vec::new(),
        })
        .expect("the fixture serves");
}

fn upsert_ambient(host: &VerterHost, virtual_id: &Arc<str>, source: &str) {
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: virtual_id.to_string(),
            source: Arc::from(source),
            file_language: crate::FileLanguage::script_ts(),
            aliases: Vec::new(),
        })
        .expect("the ambient lib serves");
}

/// Two configured projects (`/a`, `/b`), each with its OWN ambient
/// `Function` corpus, plus one plain source file per project so the demand
/// canonicals resolve to their projects.
fn two_project_ambient_host() -> Arc<VerterHost> {
    let workspace = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    workspace.set_project_graph(verter_workspace::ProjectGraph::from_configs(vec![
        project_config("/a"),
        project_config("/b"),
    ]));
    let mut virtual_ids = Vec::new();
    for (ordinal, (lib_id, lib)) in [("lib.a.d.ts", AMBIENT_LIB_A), ("lib.b.d.ts", AMBIENT_LIB_B)]
        .into_iter()
        .enumerate()
    {
        verter_workspace::WorkspaceAccess::register_ambient_lib(
            workspace.as_ref(),
            verter_workspace::AmbientLibSpec {
                project_id: Some(verter_workspace::workspace_snapshot::ProjectId(
                    ordinal as u32,
                )),
                canonical_id: Arc::from(lib_id),
                source: Arc::from(lib),
            },
        )
        .expect("the ambient corpus registers against its project");
        let key = verter_workspace::WorkspaceRead::project_stable_key(
            workspace.as_ref(),
            verter_workspace::workspace_snapshot::ProjectId(ordinal as u32),
        )
        .expect("project key");
        virtual_ids.push((
            verter_workspace::ambient_virtual_canonical_id(key, lib_id),
            lib,
        ));
    }
    let access: Arc<dyn verter_workspace::WorkspaceAccess> = workspace;
    let host = Arc::new(VerterHost::new(HostConfig::default(), access));
    for (virtual_id, lib) in &virtual_ids {
        upsert_ambient(&host, virtual_id, lib);
    }
    upsert_ts(&host, "/a/main.ts", "export const a = 1;\n");
    upsert_ts(&host, "/b/main.ts", "export const b = 2;\n");
    host
}

/// One configured project (`/ws`) with NO ambient corpus registered.
fn no_ambient_host() -> Arc<VerterHost> {
    let workspace = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    workspace.set_project_graph(verter_workspace::ProjectGraph::from_configs(vec![
        project_config("/ws"),
    ]));
    let access: Arc<dyn verter_workspace::WorkspaceAccess> = workspace;
    let host = Arc::new(VerterHost::new(HostConfig::default(), access));
    upsert_ts(&host, "/ws/main.ts", "export const w = 1;\n");
    host
}

/// An anonymous `(x: string) => "rootless"` call signature — no authored
/// occurrence, so its anchor classification is rootless.
fn rootless_signature(dispatch: &ProjectSemanticDispatch<'_>) -> SemanticNodeId {
    let graph = dispatch.graph();
    let string = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let return_type = graph.intern_node(SemanticNodeData::Literal(
        verter_type_expr::LiteralValue::String("rootless".into()),
    ));
    graph.intern_node(SemanticNodeData::Signature {
        kind: SignatureKind::Call,
        params: Arc::from(
            vec![FunctionParam::synthetic(None, string, false, false)].into_boxed_slice(),
        ),
        return_type,
        type_parameters: Arc::from(Vec::new().into_boxed_slice()),
        occurrence: None,
        return_carrier: SignatureReturnCarrier::Declared(return_type),
        signature_span: None,
        return_type_span: None,
    })
}

/// The property names of `node`'s object surface, via the test projection.
fn surface_member_names(host: &VerterHost, node: SemanticNodeId) -> Vec<String> {
    let expr = host
        .project_node_to_type_expr_for_test(node)
        .expect("the apparent surface projects to a TypeExpr");
    let verter_type_expr::TypeExpr::Object(object) = &expr else {
        panic!("expected an object surface, got {expr:?}");
    };
    object
        .properties
        .iter()
        .filter_map(|member| match member {
            verter_type_expr::ObjectMember::Property(prop) => {
                prop.key.as_string().map(str::to_owned)
            }
            verter_type_expr::ObjectMember::Method(method) => {
                method.key.as_string().map(str::to_owned)
            }
            _ => None,
        })
        .collect()
}

/// The rootless `ApparentType` key `apparent_type_of` mints for
/// `(base, demand_canonical)` — built through the SAME private context
/// constructor, so the probe key and the walker key agree.
fn rootless_key(
    dispatch: &ProjectSemanticDispatch<'_>,
    base: SemanticNodeId,
    demand_canonical: &str,
) -> SemanticQueryKey {
    SemanticQueryKey::ApparentType {
        base,
        context: dispatch.apparent_type_context_scoped(
            demand_canonical,
            ApparentDemandScope::Rootless {
                canonical: Arc::from(demand_canonical),
            },
        ),
    }
}

/// Resolve the rootless apparent surface of `base` under `demand_canonical`
/// through the walker-side entry (`apparent_type_of` behind a lexical
/// demand-scope frame).
fn apparent_under_demand_scope(
    dispatch: &ProjectSemanticDispatch<'_>,
    base: SemanticNodeId,
    demand_canonical: &str,
) -> Option<SemanticNodeId> {
    let _scope = super::super::LexicalDemandScopeGuard::push(
        &dispatch.lexical_demand_scope,
        Arc::from(demand_canonical),
    );
    dispatch.apparent_type_of(base)
}

/// One interned rootless signature node, demanded from TWO projects with
/// DIFFERENT ambient `Function` corpora: each demand resolves its OWN
/// project's surface, and neither serves the other's (the A-scoped surface
/// carries `onlyA` and no `onlyB`; the B-scoped surface the reverse). Both
/// rootless values are cache-suppressed and hold NO family-memo candidate,
/// so a warm cross-project hit is structurally impossible.
///
/// Mutation recipe: scope the rootless lookup by a fixed canonical (or
/// serve the first resolved surface for both demands) and the B-scoped
/// member assertions fail; admit the rootless build (drop its
/// `cache_suppress`) and the zero-candidate assertions fail.
#[test]
fn rootless_apparent_surface_is_project_scoped_and_never_cached() {
    let host = two_project_ambient_host();
    let store_view = host.resolver_store_view_read().into_owned_view();
    let overlay = Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
    let host_ctx = crate::resolver_core::HostResolverContext::new(&host, &store_view, overlay);
    let dispatch = ProjectSemanticDispatch::new(&host_ctx);
    let base = rootless_signature(&dispatch);

    let apparent_a = apparent_under_demand_scope(&dispatch, base, "/a/main.ts")
        .expect("the A-scoped rootless apparent surface resolves");
    let names_a = surface_member_names(&host, apparent_a);
    assert!(names_a.iter().any(|n| n == "onlyA"), "got {names_a:?}");
    assert!(names_a.iter().any(|n| n == "call"), "got {names_a:?}");
    assert!(
        !names_a.iter().any(|n| n == "onlyB"),
        "the A-scoped surface must not carry project B's marker: {names_a:?}"
    );

    // The SAME interned node demanded from project B resolves B's surface —
    // the A result was not served across the project boundary.
    let apparent_b = apparent_under_demand_scope(&dispatch, base, "/b/main.ts")
        .expect("the B-scoped rootless apparent surface resolves");
    let names_b = surface_member_names(&host, apparent_b);
    assert!(names_b.iter().any(|n| n == "onlyB"), "got {names_b:?}");
    assert!(names_b.iter().any(|n| n == "call"), "got {names_b:?}");
    assert!(
        !names_b.iter().any(|n| n == "onlyA"),
        "the B-scoped surface must not carry project A's marker: {names_b:?}"
    );
    assert_ne!(
        apparent_a, apparent_b,
        "two projects' ambient surfaces are distinct nodes"
    );

    // Neither rootless value entered the shared family memo.
    let graph = host.project_type_store().semantic_graph();
    for canonical in ["/a/main.ts", "/b/main.ts"] {
        assert_eq!(
            graph.slot_candidate_count_for_tests(&rootless_key(&dispatch, base, canonical)),
            0,
            "a rootless apparent value must never be admitted ({canonical})"
        );
    }
}

/// The rootless apparent read is `cache_suppress` at the read boundary, and
/// the ENCLOSING member projection that consumed it inherits the taint: the
/// `.call` member of a rootless callable resolves (value flows) but neither
/// the `ApparentType` value nor the enclosing `ProjectMember`/`ProjectPath`
/// projection holds a family-memo candidate afterwards.
///
/// Mutation recipe: drop the producer's rootless `cache_suppress` and the
/// suppress/zero-candidate assertions fail (the enclosing projection would
/// be admitted warm).
#[test]
fn rootless_apparent_taint_propagates_through_enclosing_member_projection() {
    let host = two_project_ambient_host();
    let store_view = host.resolver_store_view_read().into_owned_view();
    let overlay = Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
    let host_ctx = crate::resolver_core::HostResolverContext::new(&host, &store_view, overlay);
    let dispatch = ProjectSemanticDispatch::new(&host_ctx);
    let base = rootless_signature(&dispatch);

    // The rootless ApparentType read itself: value + suppress.
    let key = rootless_key(&dispatch, base, "/a/main.ts");
    let read = dispatch.execute_read(key.clone());
    assert!(
        matches!(read.value, QueryResult::Value(_)),
        "the rootless apparent surface resolves, got {:?}",
        read.value
    );
    assert!(
        read.cache_suppress,
        "a rootless apparent value must be cache-suppressed"
    );

    // The ENCLOSING member projection (`<rootless>.call` through the
    // walker's apparent hop) resolves and inherits the suppress taint.
    let member_key = SemanticQueryKey::ProjectMember {
        base,
        member: Arc::from("call"),
        mode: ProjectionMode::Navigate,
    };
    let member_read = {
        let _scope = super::super::LexicalDemandScopeGuard::push(
            &dispatch.lexical_demand_scope,
            Arc::from("/a/main.ts"),
        );
        dispatch.execute_read(member_key.clone())
    };
    assert!(
        matches!(member_read.value, QueryResult::Value(_)),
        "`.call` on a rootless callable resolves through the apparent hop, got {:?}",
        member_read.value
    );
    assert!(
        member_read.cache_suppress,
        "the enclosing member projection must inherit the rootless suppress taint"
    );

    // Nothing entered the shared family memo: not the rootless apparent
    // value, not the enclosing projection (probed under BOTH its API form
    // and its canonicalized ProjectPath form).
    let graph = host.project_type_store().semantic_graph();
    assert_eq!(graph.slot_candidate_count_for_tests(&key), 0);
    assert_eq!(graph.slot_candidate_count_for_tests(&member_key), 0);
    let path_key = SemanticQueryKey::ProjectPath {
        base,
        path: Arc::from(
            vec![crate::semantic_query::PathSegment::Member(
                crate::semantic_query::PropertyKey::identifier("call"),
            )]
            .into_boxed_slice(),
        ),
        context: crate::semantic_query::ProjectionReductionContext::published(
            ProjectionMode::Navigate,
        ),
    };
    assert_eq!(graph.slot_candidate_count_for_tests(&path_key), 0);
}

/// Without a registered ambient corpus the rootless demand FAILS CLOSED:
/// no fabricated surface, an honest `Miss`, and nothing admitted. The
/// demand-scope witness alone is not ambient proof.
///
/// Mutation recipe: synthesize a callable surface when the registry lookup
/// misses and this test fails.
#[test]
fn rootless_apparent_without_registered_ambient_fails_closed() {
    let host = no_ambient_host();
    let store_view = host.resolver_store_view_read().into_owned_view();
    let overlay = Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
    let host_ctx = crate::resolver_core::HostResolverContext::new(&host, &store_view, overlay);
    let dispatch = ProjectSemanticDispatch::new(&host_ctx);
    let base = rootless_signature(&dispatch);

    assert_eq!(
        apparent_under_demand_scope(&dispatch, base, "/ws/main.ts"),
        None,
        "no registered ambient corpus ⇒ no apparent surface"
    );
    let read = dispatch.execute_read(rootless_key(&dispatch, base, "/ws/main.ts"));
    assert!(
        matches!(read.value, QueryResult::Error(QueryError::Miss)),
        "the producer misses honestly, got {:?}",
        read.value
    );
}

/// A rootless base with NO lexical demand site on the stack fails closed at
/// the walker entry: no demand canonical, no key, no lookup.
#[test]
fn rootless_apparent_without_demand_site_fails_closed() {
    let host = two_project_ambient_host();
    let store_view = host.resolver_store_view_read().into_owned_view();
    let overlay = Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
    let host_ctx = crate::resolver_core::HostResolverContext::new(&host, &store_view, overlay);
    let dispatch = ProjectSemanticDispatch::new(&host_ctx);
    let base = rootless_signature(&dispatch);

    assert_eq!(
        dispatch.apparent_type_of(base),
        None,
        "a rootless callable with no member-access/call site on the stack has no scope"
    );
}
