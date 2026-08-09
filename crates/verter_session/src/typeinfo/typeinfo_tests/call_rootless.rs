//! @ai-generated - Rootless-callable `.call` public-boundary rows.
//!
//! A function-typed parameter and a local arrow are ROOTLESS callables:
//! their signatures carry no authored occurrence, so no declaring canonical
//! anchors the ambient lookup. `.call` on them scopes the ambient `Function`
//! registry by the LEXICAL DEMAND canonical — the file containing the
//! member-access/call site — and rebases onto the extracted callable, so the
//! call returns the callable's own declared return, never `NotCallable`.
//!
//! Verified via tsc 7.0.2 `--strict`:
//! `IsExactly<ParamCallResult, 1>` and `IsExactly<LocalArrowCallResult, 1>`
//! both hold for this fixture's aliases.

use super::support::*;
use crate::VerterHost;

const CALL_ROOTLESS: &str = include_str!("fixtures/call_rootless.ts");
const CALL_ROOTLESS_CANONICAL: &str = "/fixtures/call_rootless.ts";

fn upsert(host: &VerterHost) {
    upsert_ts(host, CALL_ROOTLESS_CANONICAL, CALL_ROOTLESS);
}

/// `fn.call(undefined, "x")` on a function-typed PARAMETER
/// `fn: (x: string) => 1` returns the callable's declared `1`.
#[test]
fn call_rootless_function_typed_parameter_call_returns_declared_return() {
    let host = make_host_with_footprint();
    upsert(&host);

    let (expr, record) = resolve_expr(
        &host,
        CALL_ROOTLESS_CANONICAL,
        "ParamCallResult",
        &[],
        ProjectionMode::Expanded,
    );

    assert_number_literal(&expr, 1.0);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

/// `local.call(undefined, "x")` on a LOCAL arrow
/// `const local = (x: string): 1 => 1` returns the callable's declared `1`.
#[test]
fn call_rootless_local_arrow_call_returns_declared_return() {
    let host = make_host_with_footprint();
    upsert(&host);

    let (expr, record) = resolve_expr(
        &host,
        CALL_ROOTLESS_CANONICAL,
        "LocalArrowCallResult",
        &[],
        ProjectionMode::Expanded,
    );

    assert_number_literal(&expr, 1.0);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

/// The SAME project layout with NO registered ambient corpus stays
/// FAIL-CLOSED: `.call` needs the active project's ambient `Function`
/// occurrence, so without one the rootless call degrades to the typed
/// `unmodeledPosition` marker — the substrate's one fail-closed spelling
/// for an unresolvable receiver (the same marker the `this`-receiver rows
/// pin), never `1`, never `any`.
#[test]
fn call_rootless_without_registered_ambient_stays_fail_closed() {
    // The standalone fixture project WITHOUT its ambient callable corpus.
    let workspace = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    workspace.set_project_graph(verter_workspace::ProjectGraph::from_configs(vec![
        verter_workspace::VfsProjectConfig {
            root: "/fixtures".to_string(),
            rank: verter_workspace::ProjectRank::Explicit,
            tsconfig_path: Some("/fixtures/tsconfig.json".to_string()),
            root_files: vec![],
            extensions: vec![".ts".into(), ".tsx".into(), ".d.ts".into()],
            workspace_root: "/fixtures".to_string(),
            workspace_aliases: vec![],
            compiler_options: verter_workspace::IdeProjectCompilerOptions::default(),
            references: vec![],
            membership: verter_workspace::ConfiguredMembership::match_all_under_root(
                &verter_workspace::CanonicalPath::new("/fixtures"),
            ),
        },
    ]));
    let access: Arc<dyn verter_workspace::WorkspaceAccess> = workspace;
    let host = Arc::new(VerterHost::new(
        crate::types::HostConfig {
            audit_enabled: true,
            footprint_capture: true,
            ..crate::types::HostConfig::default()
        },
        access,
    ));
    upsert(&host);

    for alias in ["ParamCallResult", "LocalArrowCallResult"] {
        let (expr, _record) = resolve_expr(
            &host,
            CALL_ROOTLESS_CANONICAL,
            alias,
            &[],
            ProjectionMode::Expanded,
        );
        match &expr {
            TypeExpr::Unknown(unknown) => assert_eq!(
                unknown.raw(),
                "unmodeledPosition",
                "{alias} must stay the typed fail-closed marker without ambient proof"
            ),
            other => panic!(
                "{alias} must fail closed without a registered ambient `Function`, got {other:?}"
            ),
        }
    }
}
