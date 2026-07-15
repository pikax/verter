//! Cycle-guard tests for `component_meta_resolution_policy`.
//!
//! Recursive `Pick<...>` / `Omit<...>`, self-referential refs, alias-spine
//! cycles, and indirect anonymous-type cycles must terminate via the
//! `(DeclIdentity, NormalizedTypeArgs)`-keyed cycle guard rather than
//! recursing without bound. Fixtures upsert REAL files so declaration
//! bodies flow through the engine's authored decl-body locators — the
//! production route the guard protects.
//!
//! Bare-name keying (the legacy `FxHashSet<String>` shape) is forbidden —
//! it collides across scopes and cannot distinguish `Pick<X, 'a'>` from
//! `Pick<X, 'b'>`.
//!
//! ## Windows caveat
//!
//! On Linux/macOS, OS guard-page hits surface as recoverable thread
//! panics — `assert_no_stack_overflow` returns `Err(StackOverflow)`.
//! On Windows, OS-level stack overflow on a child thread aborts the
//! entire process and `join` never returns. The cycle-guard tests in
//! this module therefore use `assert_no_stack_overflow` from
//! `verter_session::for_tests::*`, which runs on a 256 KiB stack —
//! the fast path forces a missing-guard regression to surface as a
//! recoverable `Err(StackOverflow)` on Linux/macOS. On Windows a missing
//! guard aborts the test process with `STATUS_STACK_OVERFLOW`; with the
//! guard present the walk returns cleanly and the test passes
//! deterministically across platforms.

use std::sync::Arc;

use verter_semantic::analysis::component_meta::{
    AcceptedSurfaceCompleteness, ComponentMetaAnalysis, ComponentMetaFlags, FallthroughSurface,
    NoFallthroughReason, PropAnalysis, ResolvedTypeAnalysis, RootReachability,
};
use verter_type_expr::facts::{ClosedTypeFact, LeafTypeFact, SemanticTypeSource};

use crate::capture_token::assert_no_stack_overflow;
use crate::component_meta_resolution_policy::apply_component_meta_resolution_policy;
use crate::resolver_core::component_meta::ResolvedTypeRegistryMeta;
use crate::resolver_core::{ResolvedDeclarationKind, ResolvedTypeDeclaration};
use crate::types::{HostConfig, UpsertRequest};
use crate::{FileLanguage, VerterHost};

// ---------------------------------------------------------------------------
// Test fixture helpers
// ---------------------------------------------------------------------------

fn empty_host() -> VerterHost {
    VerterHost::new_standalone(HostConfig::default())
}

fn upsert_ts(host: &VerterHost, id: &str, source: &str) {
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: id.to_string(),
            source: Arc::from(source),
            file_language: FileLanguage::script_ts(),
            aliases: Vec::new(),
        })
        .unwrap();
}

fn empty_meta() -> ComponentMetaAnalysis {
    ComponentMetaAnalysis {
        props: vec![],
        events: vec![],
        slots: vec![],
        models: vec![],
        exposed: vec![],
        public_instance: None,
        sfc_blocks: None,
        type_registry: vec![],
        components: vec![],
        template_refs: vec![],
        imports: vec![],
        bindings: vec![],
        vue_api_calls: vec![],
        styles: vec![],
        flags: ComponentMetaFlags::default(),
        root_reachability: RootReachability::NoFallthrough {
            reason: NoFallthroughReason::NoTemplate,
        },
        accepted_props: vec![],
        accepted_events: vec![],
        accepted_surface_completeness: AcceptedSurfaceCompleteness::Exact,
        fallthrough_surface: FallthroughSurface::None {
            reason: NoFallthroughReason::NoTemplate,
        },
        macro_expansion_diagnostics: vec![],
        options_api: false,
        file_path: String::from("/fixture/Component.vue"),
    }
}

fn prop(name: &str, type_source: SemanticTypeSource) -> PropAnalysis {
    PropAnalysis {
        name: name.to_string(),
        type_source: verter_type_expr::facts::SourcePosition::Present(type_source),
        type_expansion: None,
        raw_type: None,
        raw_type_source: None,
        required: false,
        has_default: false,
        default_value: None,
        description: None,
        tags: vec![],
        declared_in_macro_type_arg: false,
    }
}

fn ref_source(name: &str) -> SemanticTypeSource {
    SemanticTypeSource::Closed(ClosedTypeFact::Leaf(LeafTypeFact::Ref(name.to_string())))
}

fn registry_entry(name: &str) -> ResolvedTypeAnalysis {
    ResolvedTypeAnalysis {
        name: name.to_string(),
        type_source: verter_type_expr::facts::SourcePosition::Present(ref_source(name)),
        type_expansion: None,
    }
}

fn meta_entry(name: &str, canonical_source: &str) -> ResolvedTypeRegistryMeta {
    ResolvedTypeRegistryMeta {
        name: name.to_string(),
        declaration: ResolvedTypeDeclaration {
            requested_name: name.to_string(),
            declaration_id: None,
            resolved_name: name.to_string(),
            canonical_source: canonical_source.to_string(),
            span: verter_span::Span::default(),
            kind: ResolvedDeclarationKind::TypeAlias,
            text: None,
        },
    }
}

/// Run the policy on a 256 KiB stack worker via `assert_no_stack_overflow`.
/// `files` upsert into the host BEFORE the policy runs (on the main thread,
/// so parse work does not charge the small stack). Returns `Ok(meta)` when
/// the cycle guard fires and the policy walker terminates cleanly; returns
/// `Err(StackOverflow)` on Linux/macOS when the guard is missing; aborts
/// the process on Windows when the guard is missing.
fn run_policy_with_overflow_check(
    files: &[(&str, &str)],
    mut meta: ComponentMetaAnalysis,
    registry: Vec<ResolvedTypeAnalysis>,
    registry_meta: Vec<ResolvedTypeRegistryMeta>,
) -> Result<ComponentMetaAnalysis, crate::capture_token::StackOverflow> {
    let host = empty_host();
    for (id, source) in files {
        upsert_ts(&host, id, source);
    }
    // Pre-warm the prepared declarations on the main thread so the small
    // stack charges ONLY the policy walk (the guarded recursion under
    // test), not the initial parse/prepare work.
    crate::resolver_core::with_bare_host_ctx_for_test(&host, |ctx| {
        for meta_row in registry_meta.iter() {
            let _ = ctx.prepared_type_decl(
                meta_row.declaration.canonical_source.as_str(),
                meta_row.name.as_str(),
            );
        }
    });
    assert_no_stack_overflow(move || {
        crate::resolver_core::with_bare_host_ctx_for_test(&host, |ctx| {
            apply_component_meta_resolution_policy(
                &mut meta,
                &registry,
                &registry_meta,
                &host,
                "/owner.vue",
                None,
                ctx,
            );
        });
        meta
    })
}

/// The published source is (still) the bare named-reference leaf.
fn source_is_bare_ref(source: Option<&SemanticTypeSource>, name: &str) -> bool {
    matches!(
        source,
        Some(SemanticTypeSource::Closed(ClosedTypeFact::Leaf(LeafTypeFact::Ref(n))))
            if n == name
    )
}

// ---------------------------------------------------------------------------
// Test 1 — Recursive Pick<…, …> with local alias
// ---------------------------------------------------------------------------

/// `type Recursive = Pick<Recursive, 'x'>` — Pick wrapping a self-reference.
/// The policy walker must terminate: the alias-spine descent registers
/// `Recursive` on the active set, the `Pick` head has no located
/// declaration, and the published source stays a recognised symbolic form
/// rather than an infinite expansion.
#[test]
fn recursive_pick_local_alias_terminates_via_cycle_guard() {
    let mut meta = empty_meta();
    meta.props.push(prop("data", ref_source("Recursive")));

    let registry = vec![registry_entry("Recursive")];
    let registry_meta = vec![meta_entry("Recursive", "/workspace/recursive.ts")];

    let meta = run_policy_with_overflow_check(
        &[(
            "/workspace/recursive.ts",
            "export type Recursive = Pick<Recursive, 'x'>;",
        )],
        meta,
        registry,
        registry_meta,
    )
    .expect("recursive Pick<Recursive,'x'> must terminate via the cycle guard");

    // Discriminating shape: the published source must stay a SYMBOLIC
    // carrier (the bare seed or the authored alias-body source) — never a
    // materialized expansion, and the walk must have returned.
    let published = meta.props[0].type_source.present();
    assert!(
        source_is_bare_ref(published, "Recursive")
            || matches!(published, Some(SemanticTypeSource::Authored(_))),
        "recursive Pick<Recursive, 'x'> must terminate with a symbolic carrier; got {published:?}",
    );
}

// ---------------------------------------------------------------------------
// Test 2 — Recursive Omit<…, …> with self-referential alias
// ---------------------------------------------------------------------------

/// `type SelfOmit = Omit<SelfOmit, 'gone'>` — Omit wrapping a self-reference.
#[test]
fn recursive_omit_self_referential_alias_terminates() {
    let mut meta = empty_meta();
    meta.props.push(prop("trimmed", ref_source("SelfOmit")));

    let registry = vec![registry_entry("SelfOmit")];
    let registry_meta = vec![meta_entry("SelfOmit", "/workspace/self_omit.ts")];

    let meta = run_policy_with_overflow_check(
        &[(
            "/workspace/self_omit.ts",
            "export type SelfOmit = Omit<SelfOmit, 'gone'>;",
        )],
        meta,
        registry,
        registry_meta,
    )
    .expect("recursive Omit<SelfOmit,'gone'> must terminate via the cycle guard");

    let published = meta.props[0].type_source.present();
    assert!(
        source_is_bare_ref(published, "SelfOmit")
            || matches!(published, Some(SemanticTypeSource::Authored(_))),
        "recursive Omit<SelfOmit, 'gone'> must terminate with a symbolic carrier; got {published:?}",
    );
}

// ---------------------------------------------------------------------------
// Test 3 — Alias spine that chains into itself
// ---------------------------------------------------------------------------

/// `type A = B; type B = A` — the alias-SPINE cycle. The descent re-enters
/// `A` while `(DeclIdentity(A), [])` is still on the active set; the guard
/// MUST fire (without it the descent recurses forever — the discriminating
/// stack-overflow regression this module exists for).
#[test]
fn policy_active_refs_breaks_mutual_alias_spine_cycle() {
    let mut meta = empty_meta();
    meta.props.push(prop("entry", ref_source("A")));

    let registry = vec![registry_entry("A"), registry_entry("B")];
    let registry_meta = vec![
        meta_entry("A", "/workspace/ab.ts"),
        meta_entry("B", "/workspace/ab.ts"),
    ];

    let meta = run_policy_with_overflow_check(
        &[("/workspace/ab.ts", "export type A = B;\nexport type B = A;")],
        meta,
        registry,
        registry_meta,
    )
    .expect("mutual alias-spine cycle A = B = A must terminate via the cycle guard");

    // The published source stays symbolic — the guard's back-edge keeps
    // the carrier rather than half-chasing the spine.
    assert!(
        source_is_bare_ref(meta.props[0].type_source.present(), "A"),
        "the cyclic alias spine must keep the symbolic seed; got {:?}",
        meta.props[0].type_source,
    );
}

// ---------------------------------------------------------------------------
// Test 4 — Mutual object cycle: type A = { x: B }; type B = { y: A }
// ---------------------------------------------------------------------------

/// A's body is structurally resolvable, so Rule 3 publishes it; the nested
/// `B` member value stays a shallow carrier (no descent through object
/// members), so the mutual cycle terminates by construction AND the outer
/// body materialises its member.
#[test]
fn policy_active_refs_breaks_mutual_alias_cycle() {
    let mut meta = empty_meta();
    meta.props.push(prop("entry", ref_source("A")));

    let registry = vec![registry_entry("A"), registry_entry("B")];
    let registry_meta = vec![
        meta_entry("A", "/workspace/a.ts"),
        meta_entry("B", "/workspace/b.ts"),
    ];

    let meta = run_policy_with_overflow_check(
        &[
            (
                "/workspace/a.ts",
                "import type { B } from \"/workspace/b.ts\";\nexport type A = { x: B };",
            ),
            (
                "/workspace/b.ts",
                "import type { A } from \"/workspace/a.ts\";\nexport type B = { y: A };",
            ),
        ],
        meta,
        registry,
        registry_meta,
    )
    .expect("mutual cycle A → B → A must terminate");

    // Rule 3 fired: the published source is A's authored body.
    assert!(
        matches!(
            meta.props[0].type_source.present(),
            Some(SemanticTypeSource::Authored(_))
        ) || source_is_bare_ref(meta.props[0].type_source.present(), "A"),
        "mutual object cycle must publish A's body source or keep the seed; got {:?}",
        meta.props[0].type_source,
    );
}

// ---------------------------------------------------------------------------
// Test 5 — Anonymous indirect cycle: type A = { x: Pick<{ y: A }, 'y'> }
// ---------------------------------------------------------------------------

/// Anonymous-type cycle through Pick. The intermediate `{ y: A }` is not a
/// named declaration; A's own body is structurally resolvable so Rule 3
/// publishes it shallow — the anonymous re-entry never expands and the
/// walk terminates.
#[test]
fn policy_active_refs_breaks_anonymous_indirect_cycle() {
    let mut meta = empty_meta();
    meta.props.push(prop("nested", ref_source("A")));

    let registry = vec![registry_entry("A")];
    let registry_meta = vec![meta_entry("A", "/workspace/a.ts")];

    let _meta = run_policy_with_overflow_check(
        &[(
            "/workspace/a.ts",
            "export type A = { x: Pick<{ y: A }, 'y'> };",
        )],
        meta,
        registry,
        registry_meta,
    )
    .expect("anonymous indirect cycle through Pick<{ y: A }, 'y'> must terminate");
    // Termination is the load-bearing contract; the closure returning Ok
    // proves the policy walk bounded the anonymous re-entry.
}

// ---------------------------------------------------------------------------
// Concurrency cycle-guard: no self-await deadlock
// ---------------------------------------------------------------------------

/// Multiple concurrent invocations of the policy on a cyclic alias spine
/// must not deadlock — there is no cooperative-await on the same
/// `(DeclId, NormalizedTypeArgs)` from within the same worker.
#[test]
fn policy_active_refs_no_self_await_deadlock() {
    // Each worker runs the policy on the alias-spine cycle within
    // `assert_no_stack_overflow`. Workers do not share an active_refs set
    // — the cycle guard is request-local in the PolicyCtx, so concurrent
    // workers cannot deadlock on it. A regression that promoted the
    // active set to a process-wide lock would surface here as a deadlock;
    // a regression that removed the cycle guard would surface as a stack
    // overflow on the 256 KiB stack.
    let workers: Vec<_> = (0..4)
        .map(|i| {
            let label = match i {
                0 => "policy_active_refs_no_self_await_deadlock_w0",
                1 => "policy_active_refs_no_self_await_deadlock_w1",
                2 => "policy_active_refs_no_self_await_deadlock_w2",
                _ => "policy_active_refs_no_self_await_deadlock_w3",
            };
            std::thread::Builder::new()
                .name(label.to_string())
                .stack_size(32 * 1024 * 1024)
                .spawn(move || {
                    let mut meta = empty_meta();
                    meta.props.push(prop("data", ref_source("A")));

                    let registry = vec![registry_entry("A"), registry_entry("B")];
                    let registry_meta = vec![
                        meta_entry("A", "/workspace/ab.ts"),
                        meta_entry("B", "/workspace/ab.ts"),
                    ];
                    // Run within `assert_no_stack_overflow` so the
                    // missing-guard regression converts into a
                    // recoverable Err on the 256 KiB inner stack.
                    run_policy_with_overflow_check(
                        &[("/workspace/ab.ts", "export type A = B;\nexport type B = A;")],
                        meta,
                        registry,
                        registry_meta,
                    )
                })
                .expect("spawn worker thread for concurrency cycle-guard fixture")
        })
        .collect();

    for handle in workers {
        let result = handle.join().expect(
            "concurrent policy worker MUST terminate without panic; a deadlock would \
             surface as the test running past Cargo's wall-clock budget",
        );
        // Termination of all workers is the contract; result must
        // be Ok — Err(StackOverflow) signals the cycle guard is
        // missing.
        let _meta = result.expect(
            "cycle guard must fire under concurrent invocation; missing guard surfaces \
             here as Err(StackOverflow) on the 256 KiB stack",
        );
    }
}

// ---------------------------------------------------------------------------
// Recursive IndexedAccess terminates deterministically
// ---------------------------------------------------------------------------

/// `type R = { self: R['self'] }` — recursive indexed access on a
/// self-referential alias. R's body is structurally resolvable, so Rule 3
/// publishes it; the inner `R['self']` member value stays a shallow
/// carrier and never re-enters the walk.
#[test]
fn recursive_indexed_access_terminates_deterministically() {
    let mut meta = empty_meta();
    meta.props.push(prop("entry", ref_source("R")));

    let registry = vec![registry_entry("R")];
    let registry_meta = vec![meta_entry("R", "/workspace/r.ts")];

    let meta = run_policy_with_overflow_check(
        &[("/workspace/r.ts", "export type R = { self: R['self'] };")],
        meta,
        registry,
        registry_meta,
    )
    .expect("recursive IndexedAccess R['self'] must terminate");

    assert!(
        matches!(
            meta.props[0].type_source.present(),
            Some(SemanticTypeSource::Authored(_))
        ) || source_is_bare_ref(meta.props[0].type_source.present(), "R"),
        "recursive R['self'] must terminate with a symbolic or authored-body source; got {:?}",
        meta.props[0].type_source,
    );
}

/// `(DeclIdentity, NormalizedTypeArgs)` cycle-guard key identity for
/// distinct declaration-typed type-args. The cycle guard discriminates
/// `Foo<A>` from `Foo<B>` via positional Decl identity — bare-name keying
/// or any normalization that collapses reference arguments to a constant
/// would make `Foo<A>` and `Foo<B>` produce the same active-ref key,
/// breaking the contract documented on `rewrite_ref_node` ("Generic
/// substitutions are part of identity — `Foo<A>` and `Foo<B>` are
/// distinct guard keys").
///
/// This test constructs the four `NormalizedTypeArg` variants directly and
/// verifies the identity contract the cycle guard relies on:
///
/// 1. `[Decl(A)]` ≠ `[Decl(B)]` for distinct DeclIdentities
/// 2. `[Decl(A), Decl(B)]` ≠ `[Decl(B), Decl(A)]` (positional)
/// 3. `[Decl(A)]` ≠ `[Literal(h)]` ≠ `[AnonymousShape(h)]` ≠ `[None]`
///    (variant-level discrimination)
/// 4. `Decl(A)` and `Decl(A')` where `A'` shares name but differs in
///    `canonical_id` produce distinct keys (cross-file discrimination)
/// 5. `Literal(hash('a'))` ≠ `Literal(hash('b'))` — the exact
///    `Pick<Self, 'a'>` vs `Pick<Self, 'b'>` literal-argument
///    discrimination, through the same `hash_literal` the node-domain
///    constructor uses.
///
/// All properties are load-bearing: any breakage would surface here as an
/// equality / hash collision, where it would NOT surface in the policy
/// walker tests above (the walker resolves type-args before the cycle
/// guard sees them, masking identity-level bugs in `NormalizedTypeArg`).
#[test]
fn normalized_type_args_distinguishes_distinct_decl_instantiations() {
    use crate::component_meta_resolution_policy::cycle_guard::{
        hash_literal, NormalizedTypeArg, NormalizedTypeArgs,
    };
    use crate::semantic_query::DeclIdentity;
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    fn decl(canonical_id: &str, decl_name: &str) -> DeclIdentity {
        DeclIdentity {
            canonical_id: Arc::from(canonical_id),
            whole_hash: Default::default(),
            decl_name: Arc::from(decl_name),
        }
    }

    fn hash<T: Hash>(value: &T) -> u64 {
        let mut h = DefaultHasher::new();
        value.hash(&mut h);
        h.finish()
    }

    let id_a = decl("/workspace/a.ts", "A");
    let id_b = decl("/workspace/b.ts", "B");
    let id_a_other_file = decl("/workspace/other.ts", "A");

    // Property 1: `[Decl(A)]` ≠ `[Decl(B)]` — distinct generic
    // instantiations of the same parameterized declaration produce
    // distinct cycle-guard keys.
    let foo_of_a = NormalizedTypeArgs::from_normalized([NormalizedTypeArg::Decl(id_a.clone())]);
    let foo_of_b = NormalizedTypeArgs::from_normalized([NormalizedTypeArg::Decl(id_b.clone())]);
    assert_ne!(
        foo_of_a, foo_of_b,
        "Foo<A> and Foo<B> MUST produce distinct cycle-guard keys — \
         a regression that normalized reference args to a constant key \
         would collapse them and fire the back-edge prematurely"
    );
    assert_ne!(
        hash(&foo_of_a),
        hash(&foo_of_b),
        "Foo<A> and Foo<B> must hash distinctly (FxHashSet identity)"
    );

    // Property 2: positional ordering — `[Decl(A), Decl(B)]` ≠
    // `[Decl(B), Decl(A)]`.
    let pair_ab = NormalizedTypeArgs::from_normalized([
        NormalizedTypeArg::Decl(id_a.clone()),
        NormalizedTypeArg::Decl(id_b.clone()),
    ]);
    let pair_ba = NormalizedTypeArgs::from_normalized([
        NormalizedTypeArg::Decl(id_b.clone()),
        NormalizedTypeArg::Decl(id_a.clone()),
    ]);
    assert_ne!(
        pair_ab, pair_ba,
        "Cell<A, B> and Cell<B, A> MUST produce distinct cycle-guard \
         keys — argument ORDER is part of identity per the docstring \
         on NormalizedTypeArgs"
    );

    // Property 3: variant-level discrimination — Decl, Literal,
    // AnonymousShape, and None must never collide on identity even
    // when their underlying hashes happen to match. The discriminator
    // tag inside the enum is what guarantees this.
    let h0: u64 = 0;
    let decl_only = NormalizedTypeArgs::from_normalized([NormalizedTypeArg::Decl(id_a.clone())]);
    let literal_only = NormalizedTypeArgs::from_normalized([NormalizedTypeArg::Literal(h0)]);
    let anon_only = NormalizedTypeArgs::from_normalized([NormalizedTypeArg::AnonymousShape(h0)]);
    let none_only = NormalizedTypeArgs::from_normalized([NormalizedTypeArg::None]);
    assert_ne!(decl_only, literal_only);
    assert_ne!(decl_only, anon_only);
    assert_ne!(decl_only, none_only);
    assert_ne!(literal_only, anon_only);
    assert_ne!(literal_only, none_only);
    assert_ne!(anon_only, none_only);

    // Property 4: cross-file discrimination — two `A` declarations in
    // different files produce distinct DeclIdentities and therefore
    // distinct NormalizedTypeArgs.
    let same_name_diff_file =
        NormalizedTypeArgs::from_normalized([NormalizedTypeArg::Decl(id_a_other_file)]);
    assert_ne!(
        decl_only, same_name_diff_file,
        "Two `A` declarations in different files MUST not collide \
         under bare-name keying — DeclIdentity carries canonical_id \
         to keep them distinct"
    );

    // Property 5: literal-value discrimination through the SAME
    // `hash_literal` the node-domain constructor uses — the
    // `Pick<Self, 'a'>` vs `Pick<Self, 'b'>` guard-key property.
    let lit_a = NormalizedTypeArgs::from_normalized([NormalizedTypeArg::Literal(hash_literal(
        &verter_type_expr::LiteralValue::String("a".to_string()),
    ))]);
    let lit_b = NormalizedTypeArgs::from_normalized([NormalizedTypeArg::Literal(hash_literal(
        &verter_type_expr::LiteralValue::String("b".to_string()),
    ))]);
    assert_ne!(
        lit_a, lit_b,
        "Pick<Self, 'a'> and Pick<Self, 'b'> MUST produce distinct \
         cycle-guard keys — literal VALUES are a discriminating dimension"
    );

    // Sanity: identical arg lists produce identical keys (the cycle
    // guard's positive case — re-entering Foo<A> with Foo<A> already
    // on the active set fires the back-edge).
    let foo_of_a_again = NormalizedTypeArgs::from_normalized([NormalizedTypeArg::Decl(id_a)]);
    assert_eq!(foo_of_a, foo_of_a_again);
    assert_eq!(hash(&foo_of_a), hash(&foo_of_a_again));
}
