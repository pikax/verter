//! The Shallow terminal of a NON-EMPTY projected path: the member's own
//! type is the answer, and a one-level surface is synthesised only for a
//! terminal that has one.
//!
//! Both Shallow demands ride the same walker terminal — the published
//! `Shallow` projection and the relation engine's `structural_transit()`
//! context — so every shape is pinned under each. The expected answers
//! are TypeScript 7.0.2's, measured through the corpus's two-step wrapper
//! (`declare const v: <probe>; export const s: null = v;`, `--strict`):
//! `{ a: 1 }['a']` is `1`, `{ a: string }['a']` is `string`,
//! `{ a: string[] }['a']` is `string[]`, `{ a: [1, 2] }['a']` is
//! `[1, 2]`, `{ a: { b: 1 } | 1 }['a']` is `1 | { b: 1; }`, and
//! `{ a: () => void }['a']` is `() => void` — never the empty surface
//! `{}` a synthesis over a surfaceless terminal produces.

use std::sync::Arc;

use super::{resolve_decl_key, ProjectSemanticDispatch};
use crate::semantic_query::{
    LiteralValue, PathSegment, PrimitiveKind, ProjectionMode, ProjectionReductionContext,
    PropertyKey, QueryResult, SemanticNodeData, SemanticNodeId, SemanticQueryApi, SemanticQueryKey,
    SemanticQueryOutput,
};
use crate::{CompileErrorPolicy, FileLanguage, HostConfig, UpsertRequest, VerterHost};

const CANONICAL: &str = "/w/projected_terminal.ts";

const SOURCE: &str = "\
type S = string;
export type Box = {
    lit: 1;
    str: string;
    arr: string[];
    tup: [1, 2];
    mixed: { b: 1 } | 1;
    fn: () => void;
    obj: { b: 1 };
    alias: S;
};
";

fn host_with_box() -> VerterHost {
    let host = VerterHost::new_standalone(HostConfig {
        dev_mode: false,
        compile_error_policy: CompileErrorPolicy::StrictError,
        ..HostConfig::default()
    });
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: CANONICAL.to_string(),
            source: Arc::from(SOURCE),
            file_language: FileLanguage::script_ts(),
            aliases: Vec::new(),
        })
        .expect("upsert the fixture");
    host
}

/// `Box[member]` under `context`, through the shared `ProjectPath` query.
fn project_member(
    dispatch: &ProjectSemanticDispatch<'_>,
    member: &str,
    context: ProjectionReductionContext,
) -> SemanticNodeId {
    let base = match dispatch.execute_type_node(SemanticQueryKey::ResolveDecl(resolve_decl_key(
        CANONICAL,
        verter_type_expr::TopLevelOwnerId::ordinary_file(),
        "Box",
    ))) {
        QueryResult::Value(SemanticQueryOutput { value, .. }) => value,
        other => panic!("Box resolves, got {other:?}"),
    };
    match dispatch.execute_type_node(SemanticQueryKey::ProjectPath {
        base,
        path: Arc::from(vec![PathSegment::Member(PropertyKey::identifier(member))]),
        context,
    }) {
        QueryResult::Value(SemanticQueryOutput { value, .. }) => value,
        other => panic!("Box['{member}'] projects, got {other:?}"),
    }
}

fn shallow_demands() -> [(&'static str, ProjectionReductionContext); 2] {
    [
        (
            "published(Shallow)",
            ProjectionReductionContext::published(ProjectionMode::Shallow),
        ),
        (
            "structural_transit()",
            ProjectionReductionContext::structural_transit(),
        ),
    ]
}

/// Every surfaceless terminal is its own answer under both Shallow
/// demands; none collapses to an empty `Object`.
#[test]
fn a_projected_surfaceless_terminal_is_the_member_type_itself() {
    let host = host_with_box();
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = host.project_type_store().semantic_graph();
    for (demand, context) in shallow_demands() {
        let data = |member: &str| {
            graph
                .node_data(project_member(&dispatch, member, context))
                .unwrap_or_else(|| panic!("{demand}: Box['{member}'] has node data"))
        };
        assert!(
            matches!(
                data("lit").as_ref(),
                SemanticNodeData::Literal(LiteralValue::Number(n)) if *n == 1.0
            ),
            "{demand}: Box['lit'] is the literal 1, got {:?}",
            data("lit")
        );
        assert!(
            matches!(
                data("str").as_ref(),
                SemanticNodeData::Primitive(PrimitiveKind::String)
            ),
            "{demand}: Box['str'] is string, got {:?}",
            data("str")
        );
        assert!(
            matches!(data("arr").as_ref(), SemanticNodeData::Array { .. }),
            "{demand}: Box['arr'] is string[], got {:?}",
            data("arr")
        );
        assert!(
            matches!(data("tup").as_ref(), SemanticNodeData::Tuple { .. }),
            "{demand}: Box['tup'] is [1, 2], got {:?}",
            data("tup")
        );
        assert!(
            matches!(data("mixed").as_ref(), SemanticNodeData::Union(arms) if arms.len() == 2),
            "{demand}: Box['mixed'] is the union {{ b: 1 }} | 1, got {:?}",
            data("mixed")
        );
        assert!(
            matches!(data("fn").as_ref(), SemanticNodeData::Signature { .. }),
            "{demand}: Box['fn'] is the callable () => void, got {:?}",
            data("fn")
        );
    }
}

/// A terminal that HAS a surface keeps its one-level synthesis: an
/// object member projects to its `Object` surface, and a reference
/// whose body is a scalar keeps the reference rather than an empty
/// surface.
#[test]
fn a_projected_terminal_with_a_surface_still_synthesises_it() {
    let host = host_with_box();
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = host.project_type_store().semantic_graph();
    for (demand, context) in shallow_demands() {
        let obj = project_member(&dispatch, "obj", context);
        match graph.node_data(obj).as_deref() {
            Some(SemanticNodeData::Object(view)) => {
                let names: Vec<_> = view
                    .positive_members()
                    .iter()
                    .filter_map(|member| member.string_name().map(str::to_owned))
                    .collect();
                assert_eq!(names, ["b"], "{demand}: Box['obj'] surfaces `b`");
            }
            other => panic!("{demand}: Box['obj'] is the {{ b: 1 }} surface, got {other:?}"),
        }
        let alias = project_member(&dispatch, "alias", context);
        assert!(
            !matches!(
                graph.node_data(alias).as_deref(),
                Some(SemanticNodeData::Object(_))
            ),
            "{demand}: Box['alias'] over `type S = string` is never an empty surface, got {:?}",
            graph.node_data(alias)
        );
    }
}
