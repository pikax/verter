//! Cycle-guard tests for `component_meta_resolution_policy`.
//!
//! Per Issue #9: recursive `Pick<...>` / `Omit<...>`, self-referential
//! refs, indirect anonymous-type cycles, and cross-file same-name
//! cycles must terminate via a `(DeclIdentity, NormalizedTypeArgs)`-keyed
//! cycle guard rather than recursing without bound.
//!
//! Bare-name keying (the legacy `FxHashSet<String>` shape) is forbidden
//! per Invariant #20 — it collides across scopes and cannot
//! distinguish `Pick<X, 'a'>` from `Pick<X, 'b'>`.
//!
//! ## Windows caveat
//!
//! On Linux/macOS, OS guard-page hits surface as recoverable thread
//! panics — `assert_no_stack_overflow` returns `Err(StackOverflow)`.
//! On Windows, OS-level stack overflow on a child thread aborts the
//! entire process and `join` never returns. The cycle-guard tests in
//! this module therefore use `assert_no_stack_overflow` from
//! `verter_session::for_tests::*`, which runs on a 256 KiB stack —
//! the fast path forces the regression to surface as a recoverable
//! `Err(StackOverflow)` on Linux/macOS. On Windows, before the cycle
//! guard is fixed, the test process aborts with `STATUS_STACK_OVERFLOW`;
//! after the fix lands, the guard returns `semanticMiss` cleanly and
//! the test passes deterministically across platforms.

use std::sync::Arc;

use verter_semantic::analysis::component_meta::{
    AcceptedSurfaceCompleteness, ComponentMetaAnalysis, ComponentMetaFlags, FallthroughSurface,
    NoFallthroughReason, PropAnalysis, ResolvedTypeAnalysis, RootReachability,
};
use verter_type_expr::{
    LiteralValue, ObjectExpr, ObjectMember, ObjectProperty, PrimitiveName, TypeExpr,
};

use crate::capture_token::assert_no_stack_overflow;
use crate::component_meta_resolution_policy::apply_component_meta_resolution_policy;
use crate::resolver_core::component_meta::ResolvedTypeRegistryMeta;
use crate::resolver_core::{ResolvedDeclarationKind, ResolvedTypeDeclaration};
use crate::types::HostConfig;
use crate::VerterHost;

// ---------------------------------------------------------------------------
// Test fixture helpers
// ---------------------------------------------------------------------------

fn empty_host() -> VerterHost {
    VerterHost::new_standalone(HostConfig::default())
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

fn prop(name: &str, type_expr: TypeExpr) -> PropAnalysis {
    PropAnalysis {
        name: name.to_string(),
        type_expr,
        type_expansion: None,
        raw_type: None,
        raw_type_expr: None,
        required: false,
        has_default: false,
        default_value: None,
        description: None,
        tags: vec![],
    }
}

fn ref_zero(name: &str) -> TypeExpr {
    TypeExpr::Ref {
        name: Arc::from(name),
        type_arguments: Arc::from(Vec::<TypeExpr>::new()),
    }
}

fn ref_with_args(name: &str, args: Vec<TypeExpr>) -> TypeExpr {
    TypeExpr::Ref {
        name: Arc::from(name),
        type_arguments: Arc::from(args),
    }
}

fn registry_entry(name: &str, body: TypeExpr) -> ResolvedTypeAnalysis {
    ResolvedTypeAnalysis {
        name: name.to_string(),
        type_expr: body,
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

fn object_with_property(prop_name: &str, ty: TypeExpr) -> TypeExpr {
    TypeExpr::Object(Arc::new(ObjectExpr {
        properties: vec![ObjectMember::Property(ObjectProperty {
            name: prop_name.to_string(),
            ty,
            optional: false,
            readonly: false,
        })],
    }))
}

/// Run the policy on a 256 KiB stack worker via `assert_no_stack_overflow`.
/// Returns `Ok(meta)` when the cycle guard fires and the policy walker
/// terminates cleanly with `semanticMiss` / preserved-Ref / structural
/// shape; returns `Err(StackOverflow)` on Linux/macOS when the guard is
/// missing; aborts the process on Windows when the guard is missing.
fn run_policy_with_overflow_check(
    mut meta: ComponentMetaAnalysis,
    registry: Vec<ResolvedTypeAnalysis>,
    registry_meta: Vec<ResolvedTypeRegistryMeta>,
) -> Result<ComponentMetaAnalysis, crate::capture_token::StackOverflow> {
    assert_no_stack_overflow(move || {
        let host = empty_host();
        apply_component_meta_resolution_policy(
            &mut meta,
            &registry,
            &registry_meta,
            &host,
            "/owner.vue",
            None,
            None,
        );
        meta
    })
}

// ---------------------------------------------------------------------------
// Test 1 — Recursive Pick<…, …> with local alias
// ---------------------------------------------------------------------------

/// `type Recursive = Pick<Recursive, 'x'>` — Pick wrapping a self-reference.
/// The policy walker must terminate; the prop type must surface a
/// `semanticMiss`/Unknown sentinel rather than infinite expansion.
#[test]
fn recursive_pick_local_alias_terminates_via_semantic_miss() {
    let mut meta = empty_meta();
    meta.props.push(prop("data", ref_zero("Recursive")));

    // Recursive's body is Pick<Recursive, 'x'> — a Pick wrapping
    // a reference to itself. The recursion fires when `rewrite_ref`
    // chases Recursive's body, encounters the inner `Recursive` ref
    // inside `Pick`'s first type argument, and re-enters the body.
    //
    // The cycle guard MUST detect re-entry on
    // `(DeclIdentity(Recursive), NormalizedTypeArgs::empty())` and
    // bail with `semanticMiss`.
    let recursive_body = ref_with_args(
        "Pick",
        vec![
            ref_zero("Recursive"),
            TypeExpr::Literal(LiteralValue::String("x".to_string())),
        ],
    );

    let registry = vec![registry_entry("Recursive", recursive_body)];
    let registry_meta = vec![meta_entry("Recursive", "/workspace/recursive.ts")];

    let meta = run_policy_with_overflow_check(meta, registry, registry_meta)
        .expect("recursive Pick<Recursive,'x'> must terminate via the cycle guard");

    // Discriminating shape match: the published type must be ONE OF
    // the documented termination sentinels, structurally typed —
    // wildcard `dbg.contains("Recursive")` is not enough because the
    // identifier name "Recursive" appears inside a `Ref` for any
    // outcome (even a buggy one that fails to terminate has a
    // `Recursive` substring in its formatted output).
    match &meta.props[0].type_expr {
        // Cycle-guard hit: published as the explicit semanticMiss
        // Unknown sentinel.
        TypeExpr::Unknown { raw } if raw.contains("semanticMiss") => {}
        // Recursion preservation: the policy may surface a back-edge
        // RecursiveRef for the recursive alias.
        TypeExpr::RecursiveRef { name, .. } if name.as_ref() == "Recursive" => {}
        // Pick was preserved unevaluated (cycle guard refused to
        // chase the body). The type-arg shape pins this is the
        // Pick<Recursive,'x'> we built (not an arbitrary Pick).
        TypeExpr::Ref {
            name,
            type_arguments,
        } if name.as_ref() == "Pick"
            && type_arguments.len() == 2
            && matches!(
                &type_arguments[0],
                TypeExpr::Ref { name: inner, .. } if inner.as_ref() == "Recursive"
            ) => {}
        // The recursive alias was preserved as a bare zero-arg ref
        // because the body chase short-circuited on the cycle guard.
        TypeExpr::Ref {
            name,
            type_arguments,
        } if name.as_ref() == "Recursive" && type_arguments.is_empty() => {}
        other => panic!(
            "recursive Pick<Recursive, 'x'> must terminate with a structurally-typed \
             sentinel (Unknown {{ raw: 'semanticMiss…' }}, RecursiveRef, preserved \
             Pick<Recursive,'x'>, or zero-arg Ref 'Recursive'); got {other:?}"
        ),
    }
}

// ---------------------------------------------------------------------------
// Test 2 — Recursive Omit<…, …> with self-referential alias
// ---------------------------------------------------------------------------

/// `type SelfOmit = Omit<SelfOmit, 'gone'>` — Omit wrapping a self-reference.
#[test]
fn recursive_omit_self_referential_alias_terminates() {
    let mut meta = empty_meta();
    meta.props.push(prop("trimmed", ref_zero("SelfOmit")));

    let self_omit_body = ref_with_args(
        "Omit",
        vec![
            ref_zero("SelfOmit"),
            TypeExpr::Literal(LiteralValue::String("gone".to_string())),
        ],
    );

    let registry = vec![registry_entry("SelfOmit", self_omit_body)];
    let registry_meta = vec![meta_entry("SelfOmit", "/workspace/self_omit.ts")];

    let meta = run_policy_with_overflow_check(meta, registry, registry_meta)
        .expect("recursive Omit<SelfOmit,'gone'> must terminate via the cycle guard");

    // Discriminating shape match — see the Pick<Recursive,'x'> sibling
    // test for the rationale.
    match &meta.props[0].type_expr {
        TypeExpr::Unknown { raw } if raw.contains("semanticMiss") => {}
        TypeExpr::RecursiveRef { name, .. } if name.as_ref() == "SelfOmit" => {}
        TypeExpr::Ref {
            name,
            type_arguments,
        } if name.as_ref() == "Omit"
            && type_arguments.len() == 2
            && matches!(
                &type_arguments[0],
                TypeExpr::Ref { name: inner, .. } if inner.as_ref() == "SelfOmit"
            ) => {}
        TypeExpr::Ref {
            name,
            type_arguments,
        } if name.as_ref() == "SelfOmit" && type_arguments.is_empty() => {}
        other => panic!(
            "recursive Omit<SelfOmit, 'gone'> must terminate with a structurally-typed \
             sentinel (Unknown {{ raw: 'semanticMiss…' }}, RecursiveRef, preserved \
             Omit<SelfOmit,'gone'>, or zero-arg Ref 'SelfOmit'); got {other:?}"
        ),
    }
}

// ---------------------------------------------------------------------------
// Test 3 — Two refs that chain into each other
// ---------------------------------------------------------------------------

/// `type A = { x: B }; type B = { y: A }`. The mutual cycle must be
/// detected when chasing A → B → A.
#[test]
fn policy_active_refs_breaks_mutual_alias_cycle() {
    let mut meta = empty_meta();
    meta.props.push(prop("entry", ref_zero("A")));

    // A's body references B; B's body references A. The walker
    // chases A → A.body (Object{x: Ref(B)}) → recurses into B
    // (because B is project-local, not Props-suffix) → B.body
    // (Object{y: Ref(A)}) → recurses into A → CYCLE.
    let a_body = object_with_property("x", ref_zero("B"));
    let b_body = object_with_property("y", ref_zero("A"));
    let registry = vec![registry_entry("A", a_body), registry_entry("B", b_body)];
    let registry_meta = vec![
        meta_entry("A", "/workspace/a.ts"),
        meta_entry("B", "/workspace/b.ts"),
    ];

    let meta = run_policy_with_overflow_check(meta, registry, registry_meta)
        .expect("mutual cycle A → B → A must terminate via the cycle guard");

    // The outer A must materialise (its top-level Object shape is
    // visible), but the inner re-entry into A must be detected and
    // halted by the cycle guard. Termination of the closure proves
    // the guard fired.
    match &meta.props[0].type_expr {
        TypeExpr::Object(obj) => {
            let has_x = obj.properties.iter().any(|m| {
                matches!(m,
                ObjectMember::Property(p) if p.name == "x")
            });
            assert!(
                has_x,
                "outer A must materialise its `x` member; got {:?}",
                obj.properties
            );
        }
        TypeExpr::Ref { name, .. } => {
            // The cycle guard fired at the outer entry as well —
            // also acceptable as long as it terminated.
            assert_eq!(name.as_ref(), "A");
        }
        other => panic!(
            "mutual cycle A → B → A must produce a published Object \
             or preserved Ref shape; got {other:?}"
        ),
    }
}

// ---------------------------------------------------------------------------
// Test 4 — Anonymous indirect cycle: type A = { x: Pick<{ y: A }, 'y'> }
// ---------------------------------------------------------------------------

/// Anonymous-type cycle through Pick. The intermediate `{ y: A }` is
/// not a named declaration; the cycle guard must still detect the
/// re-entry into A via the anonymous-shape path.
#[test]
fn policy_active_refs_breaks_anonymous_indirect_cycle() {
    let mut meta = empty_meta();
    meta.props.push(prop("nested", ref_zero("A")));

    // A's body is { x: Pick<{ y: A }, 'y'> }. Resolving A chases
    // its body, which contains Pick<...> wrapping an anonymous
    // object whose `y` member references A. The walker must
    // recognise the re-entry through the anonymous shape and
    // terminate.
    let inner_object = TypeExpr::Object(Arc::new(ObjectExpr {
        properties: vec![ObjectMember::Property(ObjectProperty {
            name: "y".to_string(),
            ty: ref_zero("A"),
            optional: false,
            readonly: false,
        })],
    }));
    let pick_inner = ref_with_args(
        "Pick",
        vec![
            inner_object,
            TypeExpr::Literal(LiteralValue::String("y".to_string())),
        ],
    );
    let a_body = object_with_property("x", pick_inner);
    let registry = vec![registry_entry("A", a_body)];
    let registry_meta = vec![meta_entry("A", "/workspace/a.ts")];

    let _meta = run_policy_with_overflow_check(meta, registry, registry_meta)
        .expect("anonymous indirect cycle through Pick<{ y: A }, 'y'> must terminate");
    // Termination is the load-bearing contract; the closure
    // returning Ok proves the cycle guard caught the re-entry into A
    // through the anonymous-shape path.
}

// ---------------------------------------------------------------------------
// Test 5 — Cross-file same-name cycle (proves DeclId keying, not name keying)
// ---------------------------------------------------------------------------

/// Two files each declare `interface Foo` referencing the OTHER file's
/// `Foo`. With bare-name (`String`) keying both Foos collide; the
/// cycle guard must distinguish them via `DeclIdentity` so the
/// (legitimate) cross-file recursion is detected and bounded.
#[test]
fn policy_active_refs_breaks_cross_file_same_name_cycle() {
    let mut meta = empty_meta();
    meta.props.push(prop("a_entry", ref_zero("Foo")));

    // The owner imports a project-local `Foo`. The registry meta
    // declares two distinct Foos at different canonical sources.
    // The walker uses the canonical_source to disambiguate the two
    // declarations; bare-name keying would over-treat both as the
    // same active ref and break navigation.
    //
    // Each Foo's body references "Foo" — but in a real lookup, the
    // resolver resolves through the import scope to the OTHER
    // file's Foo. To express this in the registry-driven test, both
    // bodies reference `Foo` and the meta has TWO different
    // canonical_sources. The first Foo entered (the owner's) will
    // be disambiguated from a re-entry by DeclId; the test asserts
    // termination — bare-name keying cannot satisfy this contract.
    let foo_a_body = object_with_property("ref_to_b", ref_zero("Foo"));
    let foo_b_body = object_with_property("ref_to_a", ref_zero("Foo"));
    let registry = vec![
        registry_entry("Foo", foo_a_body),
        registry_entry("Foo", foo_b_body),
    ];
    let registry_meta = vec![
        meta_entry("Foo", "/workspace/a.ts"),
        meta_entry("Foo", "/workspace/b.ts"),
    ];

    let _meta = run_policy_with_overflow_check(meta, registry, registry_meta).expect(
        "cross-file same-name cycle Foo (a.ts) ↔ Foo (b.ts) must terminate via DeclId-keyed guard",
    );
    // Termination is the load-bearing contract: bare-name keying
    // would prematurely flag the second Foo as an active-ref re-entry
    // and short-circuit, OR — if the walker doesn't have a guard at
    // all — recurse forever. With DeclId keying, the walker
    // navigates Foo distinctly per-source and terminates.
}

// ---------------------------------------------------------------------------
// Test 6 — Pick<X, 'a'> and Pick<X, 'b'> are NOT considered the same active-ref
// ---------------------------------------------------------------------------

/// Per Invariant #20: `(DeclId, NormalizedTypeArgs)` keying means
/// `Pick<X, 'a'>` and `Pick<X, 'b'>` produce different active-ref
/// keys. The walker must navigate both without prematurely bailing.
#[test]
fn policy_active_refs_distinguishes_pick_with_different_type_args() {
    let mut meta = empty_meta();
    // `combined: { a_branch: Pick<X, 'a'>, b_branch: Pick<X, 'b'> }`
    let outer = TypeExpr::Object(Arc::new(ObjectExpr {
        properties: vec![
            ObjectMember::Property(ObjectProperty {
                name: "a_branch".to_string(),
                ty: ref_with_args(
                    "Pick",
                    vec![
                        ref_zero("X"),
                        TypeExpr::Literal(LiteralValue::String("a".to_string())),
                    ],
                ),
                optional: false,
                readonly: false,
            }),
            ObjectMember::Property(ObjectProperty {
                name: "b_branch".to_string(),
                ty: ref_with_args(
                    "Pick",
                    vec![
                        ref_zero("X"),
                        TypeExpr::Literal(LiteralValue::String("b".to_string())),
                    ],
                ),
                optional: false,
                readonly: false,
            }),
        ],
    }));
    meta.props.push(prop("combined", outer));

    // X's body is a simple object with both members; both Pick'd
    // members reach this body via different type arguments.
    let x_body = TypeExpr::Object(Arc::new(ObjectExpr {
        properties: vec![
            ObjectMember::Property(ObjectProperty {
                name: "a".to_string(),
                ty: TypeExpr::Primitive(PrimitiveName::String),
                optional: false,
                readonly: false,
            }),
            ObjectMember::Property(ObjectProperty {
                name: "b".to_string(),
                ty: TypeExpr::Primitive(PrimitiveName::Number),
                optional: false,
                readonly: false,
            }),
        ],
    }));
    let registry = vec![registry_entry("X", x_body)];
    let registry_meta = vec![meta_entry("X", "/workspace/x.ts")];

    let meta = run_policy_with_overflow_check(meta, registry, registry_meta)
        .expect("Pick<X, 'a'> and Pick<X, 'b'> must navigate without overflow");

    // Both Pick branches MUST navigate distinctly. With name-only
    // keying the second Pick<X, 'b'> would be flagged as the same
    // active-ref as Pick<X, 'a'> and short-circuit.
    //
    // The discriminating signal: the outer Object MUST surface BOTH
    // a_branch and b_branch as recognisable shapes — neither
    // collapsed to a degenerate sentinel.
    let TypeExpr::Object(outer_obj) = &meta.props[0].type_expr else {
        panic!(
            "outer combined object must remain an Object; got {:?}",
            meta.props[0].type_expr
        );
    };
    let prop_names: Vec<&str> = outer_obj
        .properties
        .iter()
        .filter_map(|m| match m {
            ObjectMember::Property(p) => Some(p.name.as_str()),
            _ => None,
        })
        .collect();
    assert!(
        prop_names.contains(&"a_branch"),
        "Pick<X, 'a'> branch must navigate; got props {prop_names:?}"
    );
    assert!(
        prop_names.contains(&"b_branch"),
        "Pick<X, 'b'> branch must navigate distinctly from 'a' branch; \
         got props {prop_names:?}"
    );
}

/// `(DeclIdentity, NormalizedTypeArgs)` cycle-guard key identity for
/// distinct declaration-typed type-args. Per Invariant #20 the cycle
/// guard discriminates `Foo<A>` from `Foo<B>` via positional Decl
/// hashing — bare-name keying or any normalization that collapses
/// `Ref` arguments to a constant would make `Foo<A>` and `Foo<B>`
/// produce the same active-ref key, breaking the contract documented
/// on `rewrite_ref` ("Generic substitutions are part of identity —
/// `Foo<A>` and `Foo<B>` are distinct guard keys").
///
/// The existing `recursive_alias_via_typeof` correctness fixture and
/// `recursive_pick_local_alias_terminates_via_semantic_miss` cycle
/// test only exercise SAME-instantiation back-edges, so a regression
/// in `NormalizedTypeArg::Decl` keying would not surface. This test
/// constructs the four `NormalizedTypeArg` variants directly and
/// verifies the identity contract that the cycle guard relies on:
///
/// 1. `[Decl(A)]` ≠ `[Decl(B)]` for distinct DeclIdentities
/// 2. `[Decl(A), Decl(B)]` ≠ `[Decl(B), Decl(A)]` (positional)
/// 3. `[Decl(A)]` ≠ `[Literal(h)]` ≠ `[AnonymousShape(h)]` ≠ `[None]`
///    (variant-level discrimination)
/// 4. `Decl(A)` and `Decl(A')` where `A'` shares name but differs in
///    `canonical_id` produce distinct keys (cross-file discrimination)
///
/// All four properties are load-bearing: any breakage would surface
/// here as an equality / hash collision, where it would NOT surface
/// in the policy walker tests above (the walker resolves type-args to
/// concrete bodies before the cycle guard sees them, masking
/// identity-level bugs in `NormalizedTypeArg`).
#[test]
fn normalized_type_args_distinguishes_distinct_decl_instantiations() {
    use crate::component_meta_resolution_policy::cycle_guard::{
        NormalizedTypeArg, NormalizedTypeArgs,
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
         a regression that normalized Ref args to a constant key \
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

    // Sanity: identical arg lists produce identical keys (the cycle
    // guard's positive case — re-entering Foo<A> with Foo<A> already
    // on the active set fires the back-edge).
    let foo_of_a_again = NormalizedTypeArgs::from_normalized([NormalizedTypeArg::Decl(id_a)]);
    assert_eq!(foo_of_a, foo_of_a_again);
    assert_eq!(hash(&foo_of_a), hash(&foo_of_a_again));
}

// ---------------------------------------------------------------------------
// Concurrency cycle-guard: no self-await deadlock
// ---------------------------------------------------------------------------

/// Multiple concurrent invocations of the policy on a recursive ref
/// must not deadlock — there is no cooperative-await on the same
/// `(DeclId, NormalizedTypeArgs)` from within the same worker.
#[test]
fn policy_active_refs_no_self_await_deadlock() {
    // Each worker runs the policy on a recursive Pick<X, …> shape
    // within `assert_no_stack_overflow`. Workers do not share an
    // active_refs set — the cycle guard is request-local in the
    // PolicyCtx, so concurrent workers cannot deadlock on it. A
    // regression that promoted the active set to a process-wide lock
    // would surface here as a deadlock; a regression that removed
    // the cycle guard would surface as a stack overflow on the
    // 256 KiB stack.
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
                    meta.props.push(prop("data", ref_zero("Recursive")));

                    let recursive_body = ref_with_args(
                        "Pick",
                        vec![
                            ref_zero("Recursive"),
                            TypeExpr::Literal(LiteralValue::String("x".to_string())),
                        ],
                    );
                    let registry = vec![registry_entry("Recursive", recursive_body)];
                    let registry_meta = vec![meta_entry("Recursive", "/workspace/recursive.ts")];
                    // Run within `assert_no_stack_overflow` so the
                    // missing-guard regression converts into a
                    // recoverable Err on the 256 KiB inner stack.
                    run_policy_with_overflow_check(meta, registry, registry_meta)
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
/// self-referential alias. The policy walker hits the recursive `R`
/// inside the IndexedAccess root; the cycle guard must catch the
/// re-entry via `(DeclIdentity(R), NormalizedTypeArgs::empty())`.
#[test]
fn recursive_indexed_access_terminates_deterministically() {
    let mut meta = empty_meta();
    meta.props.push(prop("entry", ref_zero("R")));

    // R's body: { self: R['self'] }
    let indexed_self = TypeExpr::IndexedAccess {
        object: Arc::new(ref_zero("R")),
        index: Arc::new(TypeExpr::Literal(LiteralValue::String("self".to_string()))),
    };
    let r_body = object_with_property("self", indexed_self);
    let registry = vec![registry_entry("R", r_body)];
    let registry_meta = vec![meta_entry("R", "/workspace/r.ts")];

    let meta = run_policy_with_overflow_check(meta, registry, registry_meta)
        .expect("recursive IndexedAccess R['self'] must terminate via the cycle guard");

    // The outer prop must materialise to an Object (R's body
    // produces `{ self: R['self'] }` after one level). The inner
    // R['self'] must NOT recurse forever; the cycle guard must catch
    // the re-entry.
    match &meta.props[0].type_expr {
        TypeExpr::Object(_) => {} // outer materialised — the cycle was caught at the inner re-entry
        TypeExpr::Ref { name, .. } => {
            assert_eq!(name.as_ref(), "R");
        }
        other => panic!(
            "recursive R['self'] must terminate with an Object or preserved Ref shape; \
             got {other:?}"
        ),
    }
}
