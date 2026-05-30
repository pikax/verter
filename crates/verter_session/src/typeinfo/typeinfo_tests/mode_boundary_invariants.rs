//! @ai-generated - Mode-boundary invariant regression tests sourced from
//! the tsgo-audit fixture
//! `benchmarks/tsgo-audit/large-fixture/src/large-types.ts`
//! (`LargeKeys_499`, 500-level dependent chain). The benchmark run
//! observed `recursion_limit_reached=true`, `hops=2503`,
//! `expansions=1002`, and `depth_high_water=501` for a query that
//! semantically only needs the 4 shallow member names of
//! `LargeRecord_499`.
//!
//! Each test here pins ONE mode-boundary contract from the type-resolution
//! SKILL (`/Users/carlosrodrigues/Documents/dev/verter/.claude/skills/type-resolution/SKILL.md`):
//!
//! 1. `Identity` returns declaration identity only — no body read,
//!    no graph walk, no result shape materialisation.
//! 2. `Shallow` exposes one shell level — object member names plus
//!    lazy per-member refs, no recursive member-body expansion.
//! 3. `Expanded` `keyof` is path/key-surface precise — only the
//!    object's member names enter the resolved emission. Member-body
//!    expansion (Pick<parent>, payload.value chains) is forbidden.
//! 4. Multi-file `export { Foo } from "./next"` re-export chains
//!    must resolve `Foo` to its terminal-leaf body — no unresolved
//!    `Ref { name: "Foo" }` shells survive.
//! 5. `keyof T` for `T = Foo & { a: 1 }` across a re-export chain
//!    must enumerate keys from both intersection arms.
//!
//! All 5 are currently TDD-red. The bounded-audit-counter discriminator
//! avoids relying on resolver-internal shape choices: when the rules
//! are followed, the per-request `hops` / `expansions` /
//! `depth_high_water` stay small regardless of chain depth.

use super::support::*;
use verter_audit::RequestKindPayload;

const MODE_BOUNDARY_CHAIN: &str = include_str!("fixtures/mode_boundary_chain.ts");
const MODE_BOUNDARY_REEXPORT_PRINCIPAL: &str =
    include_str!("fixtures/mode_boundary_reexport_principal.ts");
const MODE_BOUNDARY_REEXPORT_LINK_1: &str =
    include_str!("fixtures/mode_boundary_reexport_link_1.ts");
const MODE_BOUNDARY_REEXPORT_LINK_2: &str =
    include_str!("fixtures/mode_boundary_reexport_link_2.ts");
const MODE_BOUNDARY_REEXPORT_LINK_3: &str =
    include_str!("fixtures/mode_boundary_reexport_link_3.ts");
const MODE_BOUNDARY_REEXPORT_LINK_4: &str =
    include_str!("fixtures/mode_boundary_reexport_link_4.ts");
const MODE_BOUNDARY_REEXPORT_LINK_5: &str =
    include_str!("fixtures/mode_boundary_reexport_link_5.ts");
const MODE_BOUNDARY_REEXPORT_LINK_6: &str =
    include_str!("fixtures/mode_boundary_reexport_link_6.ts");
const MODE_BOUNDARY_REEXPORT_BARREL: &str =
    include_str!("fixtures/mode_boundary_reexport_barrel.ts");
const MODE_BOUNDARY_REEXPORT_LEAF: &str = include_str!("fixtures/mode_boundary_reexport_leaf.ts");

const CHAIN_FILE: &str = "/fixtures/mode_boundary_chain.ts";
const REEXPORT_PRINCIPAL_FILE: &str = "/fixtures/mode_boundary_reexport_principal.ts";

fn upsert_chain(host: &crate::VerterHost) {
    upsert_ts(host, CHAIN_FILE, MODE_BOUNDARY_CHAIN);
}

fn upsert_reexport_chain(host: &crate::VerterHost) {
    upsert_ts(
        host,
        "/fixtures/mode_boundary_reexport_leaf.ts",
        MODE_BOUNDARY_REEXPORT_LEAF,
    );
    upsert_ts(
        host,
        "/fixtures/mode_boundary_reexport_barrel.ts",
        MODE_BOUNDARY_REEXPORT_BARREL,
    );
    upsert_ts(
        host,
        "/fixtures/mode_boundary_reexport_link_6.ts",
        MODE_BOUNDARY_REEXPORT_LINK_6,
    );
    upsert_ts(
        host,
        "/fixtures/mode_boundary_reexport_link_5.ts",
        MODE_BOUNDARY_REEXPORT_LINK_5,
    );
    upsert_ts(
        host,
        "/fixtures/mode_boundary_reexport_link_4.ts",
        MODE_BOUNDARY_REEXPORT_LINK_4,
    );
    upsert_ts(
        host,
        "/fixtures/mode_boundary_reexport_link_3.ts",
        MODE_BOUNDARY_REEXPORT_LINK_3,
    );
    upsert_ts(
        host,
        "/fixtures/mode_boundary_reexport_link_2.ts",
        MODE_BOUNDARY_REEXPORT_LINK_2,
    );
    upsert_ts(
        host,
        "/fixtures/mode_boundary_reexport_link_1.ts",
        MODE_BOUNDARY_REEXPORT_LINK_1,
    );
    upsert_ts(
        host,
        REEXPORT_PRINCIPAL_FILE,
        MODE_BOUNDARY_REEXPORT_PRINCIPAL,
    );
}

fn resolve_with_mode(
    host: &crate::VerterHost,
    canonical: &str,
    name: &str,
    mode: ProjectionMode,
) -> (TypeExpr, verter_audit::RequestAuditRecord) {
    resolve_expr(host, canonical, name, &[], mode)
}

fn type_resolution_payload(
    record: &verter_audit::RequestAuditRecord,
) -> &verter_audit::TypeResolutionPayload {
    match &record.kind_payload {
        RequestKindPayload::TypeResolution(payload) => payload,
        other => panic!("expected TypeResolution payload, got {other:?}"),
    }
}

// =====================================================================
// 1) keyof on a 12-level dependent chain must stay bounded in Expanded
// =====================================================================
//
// SKILL contract: `Expanded` `keyof T` on an object literal/interface
// only needs the SHALLOW member name surface of T — the resolver must
// NOT recurse into `parent`/`payload` member bodies just to enumerate
// the outer keyspace. Tsgo-audit benchmark probe at 500 levels:
// `recursion_limit_reached=true`, hops=2503, expansions=1002, depth=501.
//
// TS7 emission: `keyof LargeRecord_11` = `"id" | "tag" | "parent" | "payload"`.
#[test]
#[ignore = "verter currently returns `Unknown { raw: \"semanticMiss\" }` for `Expanded(LargeKeys_11)` at the 12-level scale (payload: hops=3, expansions=1, depth=1) — the resolver aborts before producing the keyspace. The tsgo-audit probe on the 500-level fixture showed a different failure mode at scale: hops=2503, expansions=1002, depth=501, recursion_limit_reached=true. Both modes violate the /type-resolution SKILL.md contract: Expanded `keyof T` on an object literal/interface must emit the literal-union of T's member names. The correct implementation only needs T's SHALLOW member-name surface — member bodies (`parent: Pick<LargeRecord_N-1, ...>`, `payload: { value: LargeValue_N-1 }`) must not enter the keyspace enumeration. Suspect call sites for the at-scale chain-walk regression: enumerate.rs:169 and evaluate.rs:176 (`DeclPlaceholder -> Instantiate { body_mode: Expanded }` shortcut) plus lower.rs:596 (object lowering propagates caller mode). Keep as the future keyof-bounded-on-deep-chain contract."]
fn mode_boundary_keyof_deep_chain_is_bounded_in_expanded() {
    let host = make_host_with_footprint();
    upsert_chain(&host);

    let (expr, record) =
        resolve_with_mode(&host, CHAIN_FILE, "LargeKeys_11", ProjectionMode::Expanded);
    assert_query_mode(&record, ProjectionModeTag::Expanded);

    // Correctness: result is exactly the 4 expected keys.
    // Discriminates against the current `Unknown { raw: "semanticMiss" }`
    // emission AND against any future implementation that returns the
    // wrong key set.
    assert_literal_union(&expr, &["id", "tag", "parent", "payload"]);

    // Boundedness: secondary discriminator against the chain-walk
    // regression observed under the tsgo-audit probe. Bound calibration:
    // a correct implementation
    // needs T's shallow surface (~5 allocations) + keyspace projection
    // (~5 allocations) = ~10 expansions, depth ~3-5. Bug behavior at the
    // 12-level scale would be ~2 allocations per level = ~24 expansions
    // and depth ~12. Bound `<= 16` discriminates between correct (~10) and
    // bug (~24); `recursion_limit_reached` must never fire.
    let payload = type_resolution_payload(&record);
    assert!(
        !payload.recursion_limit_reached,
        "Expanded keyof on a 12-level chain must not hit the recursion limit; got payload={payload:?}"
    );
    assert!(
        payload.expansions <= 16,
        "Expanded keyof on a 12-level chain must enumerate only the OUTER member names — member bodies (Pick<parent>, payload.value) must not contribute expansions; got expansions={} (depth={}, hops={})",
        payload.expansions,
        payload.depth_high_water,
        payload.hops
    );
    assert!(
        u32::from(payload.depth_high_water) <= 8,
        "Expanded keyof on a 12-level chain must stay near-shallow in depth; got depth={} (expansions={}, hops={})",
        payload.depth_high_water,
        payload.expansions,
        payload.hops
    );
}

// =====================================================================
// 2) Identity mode must not materialize the alias body
// =====================================================================
//
// SKILL contract: `Identity` returns declaration identity. The resolver
// must NOT read the alias body, must NOT walk the dependent chain, and
// must NOT enumerate the keyspace.
//
// tsgo-audit benchmark probe observation: `resolveSymbolWithAudit(...,
// "identity")` on `LargeKeys_499` returned the concrete `"even" | "id"
// | "parent" | "payload" | "tag"` union after 2503 hops and depth 501.
#[test]
#[ignore = "verter currently returns `Unknown { raw: \"semanticMiss\" }` for `resolveSymbolWithAudit(file, \"LargeKeys_11\", null, Identity)` instead of the alias declaration identity. The /type-resolution SKILL.md contract is that `Identity(X)` returns X's declaration identity (a `Ref { name: \"LargeKeys_11\" }` or `RecursiveRef`), NOT X's body and NOT a semantic miss. At small chain scale the resolver's miss happens to avoid the body-materialisation hazard observed by the tsgo-audit probe (hops=2503, depth=501 at 500 levels), but a miss is not a substitute for the contracted identity shape — clients that wire dependency edges from Identity get no signal. Keep as the future identity-mode-returns-alias-decl-identity contract."]
fn mode_boundary_identity_does_not_materialize_alias_body() {
    let host = make_host_with_footprint();
    upsert_chain(&host);

    let (expr, record) =
        resolve_with_mode(&host, CHAIN_FILE, "LargeKeys_11", ProjectionMode::Identity);
    assert_query_mode(&record, ProjectionModeTag::Identity);

    // RESULT-SHAPE assertion: Identity must surface the alias
    // declaration identity (a `Ref` / `RecursiveRef` named
    // `LargeKeys_11`). It MUST NOT:
    //   (a) reduce to the concrete `"id" | "tag" | "parent" | "payload"`
    //       literal union — that is the Expanded emission and would mean
    //       the resolver walked the keyof operator and the chain;
    //   (b) return `Unknown { raw: "semanticMiss" }` — a miss is not a
    //       substitute for declaration identity. The SKILL says Identity
    //       returns "canonical file + symbol name + optional substitution
    //       environment" — that maps to a Ref-shaped projection, not a miss.
    let is_alias_identity = matches!(
        &expr,
        TypeExpr::Ref { name, .. } | TypeExpr::RecursiveRef { name, .. } if name.as_ref() == "LargeKeys_11"
    );
    assert!(
        is_alias_identity,
        "Identity(LargeKeys_11) must surface the alias declaration identity as `Ref {{ name: \"LargeKeys_11\" }}` or `RecursiveRef {{ name: \"LargeKeys_11\" }}` — NOT a fully reduced literal union AND NOT `Unknown {{ raw: \"semanticMiss\" }}`. Got: {expr:?}"
    );

    let payload = type_resolution_payload(&record);
    assert!(
        !payload.recursion_limit_reached,
        "Identity mode must never hit the recursion limit; got payload={payload:?}"
    );
    assert!(
        payload.expansions == 0,
        "Identity mode must not expand any nodes; got expansions={} (hops={}, depth={})",
        payload.expansions,
        payload.hops,
        payload.depth_high_water
    );
    assert!(
        u32::from(payload.depth_high_water) <= 2,
        "Identity mode must stay at the declaration identity depth; got depth={} (expansions={}, hops={})",
        payload.depth_high_water,
        payload.expansions,
        payload.hops
    );
    assert!(
        payload.hops <= 4,
        "Identity mode hops must be dominated by alias lookup, not chain traversal; got hops={} (expansions={}, depth={})",
        payload.hops,
        payload.expansions,
        payload.depth_high_water
    );
}

// =====================================================================
// 3) Shallow mode must not expand member bodies
// =====================================================================
//
// SKILL contract: `Shallow` exposes one shell level. For an object,
// member names plus lazy per-member refs — not recursive member-body
// expansion.
//
// tsgo-audit benchmark probe observation: `shallow` on `LargeKeys_499`
// behaved like `expanded`: concrete key union, hops=2503,
// expansions=1002, depth=501.
//
// This test probes `LargeRecord_11` (the interface, not the keyof
// alias) so the contract is concretely visible: members `id`, `tag`,
// `parent`, `payload` must appear at the surface, but `parent`'s body
// (`Pick<LargeRecord_10, "id" | "tag">`) and `payload.value`'s body
// (`LargeValue_10 | "value_11"`) must remain SHALLOW — not full
// 12-level dependent-chain expansions.
#[test]
#[ignore = "verter currently treats Shallow mode as Expanded for the `parent` member — it materialises `parent: Pick<LargeRecord_10, \"id\" | \"tag\">` into the full `{ id: 10, tag: \"tag_10\" }` Object shape (probe at 12-level scale: expansions=23, depth=12, hops=47; tsgo-audit probe on 500-level fixture: hops=2503, expansions=1002, depth=501). The /type-resolution SKILL.md contract is that Shallow exposes ONE shell level — member names plus per-member reference nodes, no recursive member-body expansion. For an interface member typed as `Pick<...>` (an operator carrier), the per-member shell must remain a Ref / unevaluated operator, NOT a fully reduced Object. Suspect call site: lower.rs:596 (`build_instantiate` advertises one shell level but the object lowering path lowers every property value with the caller's current mode). Keep as the future shallow-mode-bounded-object-lowering contract."]
fn mode_boundary_shallow_does_not_expand_member_bodies() {
    let host = make_host_with_footprint();
    upsert_chain(&host);

    let (expr, record) =
        resolve_with_mode(&host, CHAIN_FILE, "LargeRecord_11", ProjectionMode::Shallow);
    assert_query_mode(&record, ProjectionModeTag::Shallow);

    // Correctness: the outer member-name surface is present.
    let props = object_props(&expr);
    assert_eq!(prop_names(&props), vec!["id", "parent", "payload", "tag"]);

    // STRUCTURAL discriminator: `parent` is declared as
    // `Pick<LargeRecord_10, "id" | "tag">` (an operator carrier).
    // Shallow must NOT reduce that to the full
    // `{ id: 10; tag: "tag_10" }` Object shape — that requires reading
    // LargeRecord_10's body. The per-member shell at the Shallow layer
    // must remain a Ref / RecursiveRef / unevaluated operator
    // representation. A reduced Object on `parent.ty` is the bug.
    let parent_ty = &props["parent"].ty;
    let parent_is_object = matches!(parent_ty, TypeExpr::Object(_));
    assert!(
        !parent_is_object,
        "Shallow `LargeRecord_11.parent` (declared `Pick<LargeRecord_10, \"id\" | \"tag\">`) must NOT be reduced to a concrete `{{ id; tag }}` Object at the Shallow layer — that requires reading LargeRecord_10's body and violates the one-shell-level contract. Got parent.ty = {parent_ty:?}"
    );

    // Boundedness: secondary discriminator against the chain-walk
    // regression. Bound calibration: a correct Shallow implementation
    // needs 1 outer Object + 4 member-refs + at most 1 anonymous-inline
    // shell for `payload` = ~6-10 expansions; depth ~2-3. Bug behavior
    // at 12 levels = ~23 expansions, depth 12. Bound `<= 12` and `<= 6`
    // discriminate cleanly; `recursion_limit_reached` must never fire.
    let payload = type_resolution_payload(&record);
    assert!(
        !payload.recursion_limit_reached,
        "Shallow mode must never hit the recursion limit; got payload={payload:?}"
    );
    assert!(
        payload.expansions <= 12,
        "Shallow mode must allocate at most one shell-level of nodes — 1 outer Object + 4 member refs + small headroom for anonymous-inline payload; got expansions={} (hops={}, depth={})",
        payload.expansions,
        payload.hops,
        payload.depth_high_water
    );
    assert!(
        u32::from(payload.depth_high_water) <= 6,
        "Shallow mode must stay near one-shell-level deep; got depth={} (expansions={}, hops={})",
        payload.depth_high_water,
        payload.expansions,
        payload.hops
    );
}

// =====================================================================
// 4) Multi-file re-export chain must resolve `Foo` to its leaf body
// =====================================================================
//
// SKILL contract: `export { Foo } from "./next"` is a typed re-export
// that must transit through every intermediate hop without leaving an
// unresolved `Ref { name: "Foo" }` shell. The terminal leaf's body
// (`type Foo = { b: 1 }`) must surface at the principal consumer.
//
// Observed divergence from tsgo: principal `WantedType = Foo & { a: 1 }`
// currently resolves to `object{a:1} & ref:Foo` (unresolved Ref); tsgo
// emits `Foo & { a: 1; }` (where Foo's body is structurally `{ b: 1 }`).
//
// TS7 emission verified via IsExactly probe:
//   IsExactly<WantedType, { b: 1 } & { a: 1 }> = true
#[test]
#[ignore = "verter currently leaves `Foo` as an unresolved `Ref { name: \"Foo\" }` after a 7-hop `export { Foo } from \"./next\"` re-export chain that culminates in `export * from \"./barrel\"` then `export { Foo } from \"./leaf\"` (observed divergence from tsgo: produces `object{a:1} & ref:Foo` where tsgo emits `Foo & { a: 1 }` with `Foo = { b: 1 }`). The /type-resolution SKILL.md contract is that typed re-exports must transit through every intermediate hop without leaving unresolved Ref shells. Keep as the future reexport-chain-resolves-imported-alias contract."]
fn mode_boundary_reexport_chain_resolves_imported_alias() {
    let host = make_host_with_footprint();
    upsert_reexport_chain(&host);

    let (expr, _record) = resolve_with_mode(
        &host,
        REEXPORT_PRINCIPAL_FILE,
        "WantedType",
        ProjectionMode::Expanded,
    );

    // Per /type-resolution SKILL.md, Expanded unwraps aliases
    // structurally. `WantedType = Foo & { a: 1 }` should surface as
    // EITHER (a) an Intersection of two Object arms `{ b: 1 } & { a: 1 }`
    // (preserving the Intersection form, mirroring tsgo's textual
    // display) OR (b) a single merged Object `{ b: 1; a: 1 }` (after
    // structural normalisation of an intersection of disjoint object
    // types). Both are semantically equivalent for this fixture; both
    // satisfy the contract that `Foo`'s body must surface, NOT remain as
    // `Ref { name: "Foo" }`.
    //
    // Reject the current bug shape FIRST so the failure message points
    // at the unresolved-Ref defect rather than at a downstream
    // structural assertion.
    let has_unresolved_foo_ref = match &expr {
        TypeExpr::Intersection(parts) => parts
            .iter()
            .any(|p| matches!(p, TypeExpr::Ref { name, .. } if name.as_ref() == "Foo")),
        TypeExpr::Ref { name, .. } => name.as_ref() == "Foo",
        _ => false,
    };
    assert!(
        !has_unresolved_foo_ref,
        "`WantedType` (= `Foo & {{ a: 1 }}` via a 7-hop re-export chain) must NOT surface `Foo` as an unresolved `Ref {{ name: \"Foo\" }}` — the re-export chain must transit to the terminal body `{{ b: 1 }}`. Got {expr:?}"
    );

    // The expression must surface as an Object or an Intersection of
    // Objects. Anything else (Union, Ref, Unknown, primitive) indicates
    // the alias chain did not deliver a structural body.
    let is_object_shape = matches!(&expr, TypeExpr::Object(_) | TypeExpr::Intersection(_));
    assert!(
        is_object_shape,
        "`WantedType` must surface as an Object or an Intersection of Objects after the alias chain is resolved; got {expr:?}"
    );

    // Collect ALL top-level property bindings reachable through the
    // top-level expression — traversing Intersection arms, but NOT
    // descending into property values (so we don't flatten arbitrary
    // nested objects).
    fn collect_top_level_props(
        expr: &TypeExpr,
        out: &mut std::collections::BTreeMap<String, TypeExpr>,
    ) {
        match expr {
            TypeExpr::Object(obj) => {
                for member in &obj.properties {
                    if let verter_type_expr::ObjectMember::Property(prop) = member {
                        out.insert(prop.name.clone(), prop.ty.clone());
                    }
                }
            }
            TypeExpr::Intersection(parts) => {
                for part in parts.iter() {
                    collect_top_level_props(part, out);
                }
            }
            _ => {}
        }
    }
    let mut binding = std::collections::BTreeMap::new();
    collect_top_level_props(&expr, &mut binding);

    let names: Vec<&str> = binding.keys().map(String::as_str).collect();
    assert_eq!(
        names,
        vec!["a", "b"],
        "`WantedType` must contain exactly two top-level bindings `a` and `b` (from `Foo & {{ a: 1 }}` where `Foo = {{ b: 1 }}`); got top-level binding names {names:?} on expr {expr:?}"
    );
    assert!(
        matches!(
            binding.get("a"),
            Some(TypeExpr::Literal(verter_type_expr::LiteralValue::Number(n))) if *n == 1.0
        ),
        "`a` must bind to the literal `1`; got binding={:?}",
        binding.get("a")
    );
    assert!(
        matches!(
            binding.get("b"),
            Some(TypeExpr::Literal(verter_type_expr::LiteralValue::Number(n))) if *n == 1.0
        ),
        "`b` must bind to the literal `1` (the resolved `Foo` body); got binding={:?}. An absent or non-literal `b` indicates the `Foo` re-export chain failed to surface its terminal body.",
        binding.get("b")
    );
}

// =====================================================================
// 5) keyof across the re-export chain enumerates keys from both arms
// =====================================================================
//
// SKILL contract: `keyof (A & B)` enumerates the union of keys from
// both arms. When A or B is an imported alias, the keyspace
// enumeration must follow the re-export chain to the terminal body —
// not stop at an unresolved Ref.
//
// Observed divergence from tsgo: `WantedKeys = keyof WantedType`
// currently returns `unknown` (because `Foo` was unresolved); TS7
// emits `"a" | "b"`.
//
// TS7 emission verified via IsExactly probe:
//   IsExactly<WantedKeys, "a" | "b"> = true
#[test]
#[ignore = "verter currently returns `unknown` for `keyof WantedType` where `WantedType = Foo & { a: 1 }` and `Foo` is reached via a 7-hop re-export chain (observed divergence from tsgo: keyspace enumeration cannot proceed past the unresolved `Ref { name: \"Foo\" }`). The /type-resolution SKILL.md contract is that `keyof (A & B)` enumerates the union of keys from both arms after fully resolving each arm — re-export chains must transit cleanly to the terminal body. Keep as the future keyof-across-reexport-chain contract."]
fn mode_boundary_keyof_across_reexport_chain_resolves_all_keys() {
    let host = make_host_with_footprint();
    upsert_reexport_chain(&host);

    let (expr, _record) = resolve_with_mode(
        &host,
        REEXPORT_PRINCIPAL_FILE,
        "WantedKeys",
        ProjectionMode::Expanded,
    );

    // TS7: WantedKeys = "a" | "b". Discriminates against the current
    // `Unknown { raw: "semanticMiss" }` emission (where keyspace
    // enumeration aborts because Foo remained an unresolved Ref) AND
    // against any future implementation that yields an incorrect key
    // set.
    assert_literal_union(&expr, &["a", "b"]);
}
