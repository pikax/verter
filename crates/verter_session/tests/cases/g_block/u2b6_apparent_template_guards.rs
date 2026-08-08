//! Guards for the `ApparentType` + `TemplateLiteralReduce` key surface.
//!
//! These tests pin the IDENTITY contract of the
//! [`SemanticQueryKey`] variants `ApparentType` and `TemplateLiteralReduce`,
//! their env-in-context dimensions (these keys have NO slot, so their R21
//! env dims ride INSIDE the context struct), the LIVE CALLABLE producer of
//! `ApparentType` (a call-signature-bearing base widens to the project's
//! registered ambient `Function`; every other base kind stays an honest
//! non-admitting `Miss`), and the LIVE concatenation producer of
//! `TemplateLiteralReduce` (which routes through the ONE shared deferred
//! evaluator, never a hand-rolled reducer).
//!
//! Identity is probed BEHAVIORALLY through the family memo exactly as the
//! sibling class/namespace/enum guards do: publishing a synthetic candidate under key `a`
//! and then reading `slot_candidate_count_for_tests(b)` is `> 0` iff `a` and
//! `b` project to the SAME `(FamilyKey, ModeSlot)`. A warm entry under one
//! identity is returned for another ONLY when they share a slot.

use std::sync::Arc;

use verter_session::for_tests::ReadSetSignature;
use verter_session::semantic_query::{
    ApparentDemandScope, ApparentTypeContext, LiteralValue, PrimitiveKind, QueryError, QueryResult,
    SemanticNodeData, SemanticNodeId, SemanticQueryKey, TemplateLiteralReduceContext,
};
use verter_session::{HostConfig, VerterHost};

fn host() -> VerterHost {
    VerterHost::new_standalone(HostConfig::default())
}

fn hash16(byte: u8) -> [u8; 16] {
    [byte; 16]
}

fn dummy_node() -> SemanticNodeId {
    SemanticNodeId(1)
}

/// Publish a synthetic candidate under `a`, then return the candidate
/// count `b` projects to. `> 0` ⟺ `a` and `b` share a `(FamilyKey, slot)`.
///
/// A FRESH host per call keeps every pair independent. Both new keys carry
/// no projection mode (`ModeSlot::Single`), so backfill — which fans out
/// only along the mode hierarchy — never muddies the probe.
fn count_for_b_after_publishing_a(a: &SemanticQueryKey, b: &SemanticQueryKey) -> usize {
    let host = host();
    let graph = host.project_type_store().semantic_graph();
    let node = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    graph.publish_with_carrier_dispatch_and_generation_for_tests(
        a.clone(),
        QueryResult::Value(node),
        ReadSetSignature::empty(),
        Arc::from(Vec::<Arc<str>>::new().into_boxed_slice()),
        Arc::from(Vec::new().into_boxed_slice()),
        100,
    );
    graph.slot_candidate_count_for_tests(b)
}

/// `a` and `b` are NON-equal keys AND project to DISTINCT `(FamilyKey,
/// slot)`. Also asserts the positive sanity (`a` reaches its own slot) so
/// the probe is not vacuously passing on a broken publish path.
fn assert_distinct_identity(a: &SemanticQueryKey, b: &SemanticQueryKey) {
    assert_ne!(a, b, "keys must be non-equal");
    assert_eq!(
        count_for_b_after_publishing_a(a, a),
        1,
        "sanity: publishing `a` must reach `a`'s own slot (count 1)"
    );
    assert_eq!(
        count_for_b_after_publishing_a(a, b),
        0,
        "a warm candidate published under `a` must NOT be reachable from \
         `b` — they must project to DISTINCT (FamilyKey, slot)"
    );
}

// ---------------------------------------------------------------------------
// Key constructors.
// ---------------------------------------------------------------------------

fn apparent_type_key(base: SemanticNodeId, t: u8, l: u8, j: u32) -> SemanticQueryKey {
    SemanticQueryKey::ApparentType {
        base,
        context: ApparentTypeContext {
            type_env_hash: hash16(t),
            lib_env_hash: hash16(l),
            project_identity: j,
            demand_scope: ApparentDemandScope::Anchored,
        },
    }
}

fn rootless_apparent_type_key(base: SemanticNodeId, demand_canonical: &str) -> SemanticQueryKey {
    SemanticQueryKey::ApparentType {
        base,
        context: ApparentTypeContext {
            type_env_hash: hash16(0),
            lib_env_hash: hash16(0),
            project_identity: 0,
            demand_scope: ApparentDemandScope::Rootless {
                canonical: Arc::from(demand_canonical),
            },
        },
    }
}

fn template_literal_reduce_key(
    pattern: &[&str],
    args: &[SemanticNodeId],
    r: u8,
    t: u8,
    l: u8,
    j: u32,
) -> SemanticQueryKey {
    let quasis: Arc<[Arc<str>]> = pattern.iter().map(|s| Arc::from(*s)).collect();
    let args: Arc<[SemanticNodeId]> = Arc::from(args.to_vec().into_boxed_slice());
    SemanticQueryKey::TemplateLiteralReduce {
        pattern: quasis,
        args,
        context: TemplateLiteralReduceContext {
            resolve_env_hash: hash16(r),
            type_env_hash: hash16(t),
            lib_env_hash: hash16(l),
            project_identity: j,
        },
    }
}

// ---------------------------------------------------------------------------
// (1) ApparentType identity covers L / T / J (carried IN the context, NOT a
//     slot) plus `base`.
// ---------------------------------------------------------------------------

#[test]
pub(crate) fn apparent_type_key_covers_lib_env_demand_and_context() {
    let base = apparent_type_key(dummy_node(), 0, 0, 0);

    // `lib_env_hash` (L) is part of identity — an apparent surface depends
    // on the lib-member index for primitive→wrapper / lib members.
    assert_distinct_identity(&base, &apparent_type_key(dummy_node(), 0, 9, 0));
    // `type_env_hash` (T) is part of identity.
    assert_distinct_identity(&base, &apparent_type_key(dummy_node(), 9, 0, 0));
    // `project_identity` (J) is part of identity.
    assert_distinct_identity(&base, &apparent_type_key(dummy_node(), 0, 0, 9));
    // `base` is part of identity.
    assert_distinct_identity(&base, &apparent_type_key(SemanticNodeId(2), 0, 0, 0));
    // The demand-scope witness is part of identity: an Anchored demand and
    // a Rootless demand over the SAME base are distinct.
    assert_distinct_identity(
        &base,
        &rootless_apparent_type_key(dummy_node(), "/a/main.ts"),
    );
    // Two Rootless demands from DIFFERENT canonicals are distinct — the
    // same interned rootless node can never cross-serve between projects.
    assert_distinct_identity(
        &rootless_apparent_type_key(dummy_node(), "/a/main.ts"),
        &rootless_apparent_type_key(dummy_node(), "/b/main.ts"),
    );
}

// ---------------------------------------------------------------------------
// (2) ApparentType do-not-warm-hit across a lib_env boundary.
// ---------------------------------------------------------------------------

#[test]
fn apparent_type_do_not_warm_hit() {
    let env_a = apparent_type_key(dummy_node(), 0, 0, 0);
    let env_b = apparent_type_key(dummy_node(), 0, 1, 0);
    assert_eq!(
        count_for_b_after_publishing_a(&env_a, &env_b),
        0,
        "ApparentType must not warm-hit across a lib_env boundary"
    );
}

// ---------------------------------------------------------------------------
// (3) The LIVE callable producer: a call-signature-bearing base widens to the
//     project's registered ambient `Function` surface, so `.call` / `.apply` /
//     `.bind` are reachable members carrying the AMBIENT canonical as their
//     declaration origin. Every non-callable base, and every project whose
//     ambient corpus registers no such interface, stays an honest
//     non-admitting `Miss` — the anti-fabrication half.
// ---------------------------------------------------------------------------

/// A callable-surface corpus: the members of the standard-library `Function`
/// interface a callable value exposes.
const AMBIENT_FUNCTION_LIB: &str = r#"
    interface Function {
        apply(this: Function, thisArg: any, argArray?: any): any;
        call(this: Function, thisArg: any, ...argArray: any[]): any;
        bind(this: Function, thisArg: any, ...argArray: any[]): any;
        readonly length: number;
    }
"#;

/// A file whose `Callable` type alias resolves to an authored call signature.
const CALLABLE_OWNER: &str = r#"
    export function greet(name: string): string {
        return name;
    }
    export type Callable = typeof greet;
"#;

const AMBIENT_LIB_ID: &str = "lib.function.d.ts";

/// Build a host over one configured project at `/ws`, optionally registering
/// the callable ambient corpus, with `/ws/owner.ts` loaded.
fn callable_host(register_ambient: bool) -> Arc<VerterHost> {
    use verter_workspace::{
        CanonicalPath, ConfiguredMembership, IdeProjectCompilerOptions, MemoryOptions,
        MemoryWorkspace, ProjectGraph, ProjectRank, VfsProjectConfig, WorkspaceAccess,
    };

    let workspace = Arc::new(MemoryWorkspace::new(MemoryOptions::default()));
    workspace.set_project_graph(ProjectGraph::from_configs(vec![VfsProjectConfig {
        root: "/ws".to_string(),
        rank: ProjectRank::Explicit,
        tsconfig_path: Some("/ws/tsconfig.json".to_string()),
        root_files: vec![],
        extensions: vec![".ts".into(), ".tsx".into(), ".d.ts".into()],
        workspace_root: "/ws".to_string(),
        workspace_aliases: vec![],
        compiler_options: IdeProjectCompilerOptions::default(),
        references: vec![],
        membership: ConfiguredMembership::match_all_under_root(&CanonicalPath::new("/ws")),
    }]));
    workspace.inject_file("/ws/owner.ts".to_string(), Arc::from(CALLABLE_OWNER));
    if register_ambient {
        workspace
            .register_ambient_lib(verter_workspace::AmbientLibSpec {
                project_id: None,
                canonical_id: Arc::from(AMBIENT_LIB_ID),
                source: Arc::from(AMBIENT_FUNCTION_LIB),
            })
            .expect("the callable ambient corpus MUST register against the configured project");
    }
    let access: Arc<dyn WorkspaceAccess> = workspace;
    Arc::new(VerterHost::new(HostConfig::default(), access))
}

/// Resolve `/ws/owner.ts`'s `Callable` alias to its authored call-signature
/// node — the base a real apparent-type demand starts from.
fn callable_base(host: &VerterHost) -> SemanticNodeId {
    let (outcome, _record) = host
        .resolve_named_symbol_wire_with_audit(
            "/ws/owner.ts",
            "Callable",
            &[],
            Some(verter_session::semantic_query::ProjectionMode::Expanded),
        )
        .into_parts();
    let node = outcome
        .ok()
        .flatten()
        .expect("`Callable` MUST resolve to a node");
    let graph = host.project_type_store().semantic_graph();
    // `typeof greet` resolves to the function's own surface; its single call
    // signature is the authored callable a member lookup starts from.
    let data = graph.node_data(node).expect("`Callable` node must exist");
    let signature = match &*data {
        SemanticNodeData::Object(surface) if surface.call_signatures.len() == 1 => {
            surface.call_signatures[0]
        }
        other => {
            panic!("`Callable` MUST resolve to a single-call-signature surface, got {other:?}")
        }
    };
    assert!(
        matches!(
            graph.node_data(signature).as_deref(),
            Some(SemanticNodeData::Signature { .. })
        ),
        "the guard's base MUST be an authored call signature, got {:?}",
        graph.node_data(signature)
    );
    signature
}

/// The `call` member names on `node`'s object surface, paired with the
/// canonical each was declared in.
fn call_member_origins(host: &VerterHost, node: SemanticNodeId) -> Vec<Option<Arc<str>>> {
    let graph = host.project_type_store().semantic_graph();
    let data = graph.node_data(node).expect("apparent surface must exist");
    let Some(SemanticNodeData::Object(surface)) = data.as_ref().into() else {
        panic!("the apparent surface of a callable MUST be an object surface, got {data:?}");
    };
    surface
        .positive_members()
        .iter()
        .filter(|member| {
            matches!(
                &member.key,
                verter_session::semantic_query::AuthoredPropertyKey::String(name)
                    if name.as_ref() == "call"
            )
        })
        .map(|member| member.declaration_origin.clone())
        .collect()
}

#[test]
fn apparent_type_widens_a_callable_to_the_registered_ambient_function_surface() {
    let host = callable_host(true);
    let base = callable_base(&host);
    let key = apparent_type_key(base, 0, 0, 0);

    let result =
        verter_session::for_tests::dispatch_execute_type_node_for_tests(&host, key.clone());
    let node = match result {
        QueryResult::Value(output) => output.value,
        other => panic!(
            "a callable base MUST widen to the ambient callable surface, got {other:?}. \
             This FAILS against a non-producing ApparentType arm."
        ),
    };
    // The widened surface carries `call`, and it is declared in the AMBIENT
    // canonical — the registry route, not a fabricated member.
    let origins = call_member_origins(&host, node);
    assert_eq!(
        origins.len(),
        1,
        "the ambient `Function` surface declares exactly one `call` member, got {origins:?}"
    );
    let origin = origins[0]
        .as_deref()
        .expect("the widened `call` member MUST carry its declaring canonical");
    let expected = host
        .workspace_read()
        .project_stable_key(verter_workspace::ProjectId(0))
        .map(|key| verter_workspace::ambient_virtual_canonical_id(key, AMBIENT_LIB_ID))
        .expect("the configured project MUST have a stable key");
    assert_eq!(
        origin,
        expected.as_ref(),
        "the widened member MUST be declared in the registered ambient lib's virtual canonical"
    );
}

#[test]
fn apparent_type_misses_without_a_registered_ambient_callable_surface() {
    // Same callable base, same project — only the ambient registration is
    // absent. The producer NEVER fabricates a surface it cannot prove.
    let host = callable_host(false);
    let base = callable_base(&host);
    let key = apparent_type_key(base, 0, 0, 0);

    let result =
        verter_session::for_tests::dispatch_execute_type_node_for_tests(&host, key.clone());
    assert!(
        matches!(result, QueryResult::Error(QueryError::Miss)),
        "an unregistered ambient corpus MUST produce Error(Miss), got {result:?}"
    );
    let graph = host.project_type_store().semantic_graph();
    assert_eq!(
        graph.slot_candidate_count_for_tests(&key),
        0,
        "a missed ApparentType build must admit NOTHING into the shared memo"
    );
}

#[test]
fn apparent_type_misses_for_a_non_callable_base() {
    // The primitive-to-wrapper widening (`string` → `String`) reads a
    // lib-member index that does not exist. Even with the callable corpus
    // registered, a primitive base is an honest, non-admitting Miss.
    let host = callable_host(true);
    let graph = host.project_type_store().semantic_graph();
    let base = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let key = apparent_type_key(base, 0, 0, 0);

    let result =
        verter_session::for_tests::dispatch_execute_type_node_for_tests(&host, key.clone());
    assert!(
        matches!(result, QueryResult::Error(QueryError::Miss)),
        "a non-callable base MUST produce Error(Miss), got {result:?}"
    );
    assert_eq!(
        graph.slot_candidate_count_for_tests(&key),
        0,
        "a non-callable ApparentType build must admit NOTHING into the shared memo"
    );
}

// ---------------------------------------------------------------------------
// (4) TemplateLiteralReduce identity covers R / T / L / J + pattern + args,
//     and args ORDER is significant (concatenation order matters).
// ---------------------------------------------------------------------------

#[test]
pub(crate) fn template_literal_reduce_key_covers_context() {
    let a = dummy_node();
    let b = SemanticNodeId(2);
    let base = template_literal_reduce_key(&["", "-", ""], &[a, b], 0, 0, 0, 0);

    // resolve_env_hash (R) is part of identity.
    assert_distinct_identity(
        &base,
        &template_literal_reduce_key(&["", "-", ""], &[a, b], 9, 0, 0, 0),
    );
    // type_env_hash (T).
    assert_distinct_identity(
        &base,
        &template_literal_reduce_key(&["", "-", ""], &[a, b], 0, 9, 0, 0),
    );
    // lib_env_hash (L).
    assert_distinct_identity(
        &base,
        &template_literal_reduce_key(&["", "-", ""], &[a, b], 0, 0, 9, 0),
    );
    // project_identity (J).
    assert_distinct_identity(
        &base,
        &template_literal_reduce_key(&["", "-", ""], &[a, b], 0, 0, 0, 9),
    );
    // pattern (quasis) is part of identity.
    assert_distinct_identity(
        &base,
        &template_literal_reduce_key(&["", "_", ""], &[a, b], 0, 0, 0, 0),
    );
    // args are part of identity.
    assert_distinct_identity(
        &base,
        &template_literal_reduce_key(&["", "-", ""], &[a], 0, 0, 0, 0),
    );

    // NEGATIVE: arg ORDER matters — `${a}-${b}` and `${b}-${a}` are
    // DISTINCT concatenations and MUST NOT collide. A reorder/sort applied
    // to `args` (as NormalizeUnion does for its order-insensitive members)
    // would make these share a slot — `assert_distinct_identity` then sees
    // count 1 and FAILS. This is the discriminating negative.
    assert_distinct_identity(
        &base,
        &template_literal_reduce_key(&["", "-", ""], &[b, a], 0, 0, 0, 0),
    );
}

// ---------------------------------------------------------------------------
// (5) TemplateLiteralReduce do-not-warm-hit across a resolve_env boundary.
// ---------------------------------------------------------------------------

#[test]
fn template_literal_reduce_do_not_warm_hit() {
    let a = dummy_node();
    let b = SemanticNodeId(2);
    let env_a = template_literal_reduce_key(&["", "-", ""], &[a, b], 0, 0, 0, 0);
    let env_b = template_literal_reduce_key(&["", "-", ""], &[a, b], 1, 0, 0, 0);
    assert_eq!(
        count_for_b_after_publishing_a(&env_a, &env_b),
        0,
        "TemplateLiteralReduce must not warm-hit across a resolve_env boundary"
    );
}

// ---------------------------------------------------------------------------
// (6) PRODUCE discriminator — the LIVE concatenation producer folds an
//     all-literal template via the ONE shared deferred evaluator, and
//     carrier-stops (returns the shell) when an expression is non-literal.
// ---------------------------------------------------------------------------

#[test]
fn template_literal_reduce_reduces_concrete_concatenation() {
    let host = host();
    let graph = host.project_type_store().semantic_graph();

    // `${"a"}-${"b"}` ⇒ pattern ["", "-", ""], args [Literal("a"), Literal("b")].
    let lit_a = graph.intern_node(SemanticNodeData::Literal(LiteralValue::String(
        "a".to_string(),
    )));
    let lit_b = graph.intern_node(SemanticNodeData::Literal(LiteralValue::String(
        "b".to_string(),
    )));
    let key = template_literal_reduce_key(&["", "-", ""], &[lit_a, lit_b], 0, 0, 0, 0);

    let result = verter_session::for_tests::dispatch_execute_type_node_for_tests(&host, key);
    let node = match result {
        QueryResult::Value(out) => out.value,
        other => panic!("TemplateLiteralReduce(all-literal) must produce a Value, got {other:?}"),
    };
    // The fold MUST be the concrete concatenation "a-b" — FAILS against a
    // non-producing (Miss) impl AND against a hand-rolled reducer that
    // concatenates wrongly.
    match graph.node_data(node).as_deref() {
        Some(SemanticNodeData::Literal(LiteralValue::String(s))) => {
            assert_eq!(
                s.as_str(),
                "a-b",
                "all-literal template must fold to the concatenated string literal"
            );
        }
        other => panic!("expected Literal(String(\"a-b\")), got {other:?}"),
    }

    // Carrier-stop NEGATIVE: with one NON-literal arg (a `Primitive(String)`
    // node), the template cannot fold — the result is the deferred
    // `TemplateLiteral` shell, NOT a fabricated literal.
    let prim = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let key2 = template_literal_reduce_key(&["", "-", ""], &[lit_a, prim], 0, 0, 0, 0);
    let result2 = verter_session::for_tests::dispatch_execute_type_node_for_tests(&host, key2);
    let node2 = match result2 {
        QueryResult::Value(out) => out.value,
        QueryResult::Recursive(n) => n,
        other => panic!("TemplateLiteralReduce(non-literal arg) must return a node, got {other:?}"),
    };
    match graph.node_data(node2).as_deref() {
        Some(SemanticNodeData::TemplateLiteral { .. }) => {}
        other => panic!(
            "non-literal arg must carrier-stop to the TemplateLiteral shell, \
             NOT a fabricated literal; got {other:?}"
        ),
    }
}
