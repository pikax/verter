//! @ai-generated - Synthetic TypeScript enum typeinfo contracts.
//!
//! These tests describe the TS7 expected projection for declarative numeric
//! enums, declarative string enums, `const enum`, the `${Enum}` template
//! expansion, `keyof typeof Enum`, and an enum-member discriminant.
//!
//! The Enums reducer is COMPLETE: every assertion below PASSES (run with
//! `--ignored` to verify) — Verter produces the TS7-correct projection. The
//! rows stay `#[ignore]`d because they are NOT oracle-liftable: tsgo's hover
//! displays the enum SHAPES as nominal qualified names (`Enum.Member`) or
//! unexpanded operator origins (`${Enum}`, `keyof typeof Enum`, indexed
//! access), which the closed positive-allowlist oracle admission gate rejects
//! (the `#[ignore]` reasons record the exact reject per row). Lifting requires
//! oracle-infra extension (escalated) — see the gate-discrimination negatives
//! below, which DO run, for the resolver-side regression coverage.

use super::support::*;

fn upsert_enum_fixture(host: &crate::VerterHost) {
    upsert_ts(host, "/fixtures/enums.ts", ENUMS);
}

const ENUMS: &str = include_str!("fixtures/enums.ts");

#[test]
#[ignore = "Enums reducer complete: Verter resolves `Color.Red` to the literal number `0` (verified). NOT oracle-liftable — tsgo hover displays the numeric-enum member NOMINALLY as `Color.Red`, which cannot carry the structural literal (oracle admission Reject(EnumMemberOrQualified)); lift pending oracle-infra for nominal enum-member display"]
fn enum_numeric_member_resolves_to_branded_literal_zero() {
    // TS7 contract: `Color.Red` is a branded numeric-enum member type whose
    // value-side numeric literal is `0`. The published surface for the type
    // alias `ColorRed = Color.Red` is the literal `0` (TS treats numeric enum
    // members as assignable to/from the corresponding number literal at the
    // type level).
    let host = make_host_with_footprint();
    upsert_enum_fixture(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/enums.ts",
        "ColorRed",
        &[],
        ProjectionMode::Expanded,
    );

    assert_number_literal(&expr, 0.0);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
#[ignore = "Enums reducer complete: Verter resolves `Status.Idle` to the string literal `\"idle\"` (verified). NOT oracle-liftable — tsgo hover displays the string-enum member NOMINALLY as `Status.Idle` (oracle admission Reject(EnumMemberOrQualified)); lift pending oracle-infra for nominal enum-member display"]
fn enum_string_member_resolves_to_branded_string_literal() {
    // TS7 contract: `Status.Idle` is a branded string-enum member. At the
    // structural type level it surfaces as the string literal `"idle"`
    // (Verter publishes the literal value; the brand identity is a TS-only
    // nominal-typing trick that has no runtime structure).
    let host = make_host_with_footprint();
    upsert_enum_fixture(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/enums.ts",
        "StatusIdle",
        &[],
        ProjectionMode::Expanded,
    );

    assert_string_literal(&expr, "idle");
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
#[ignore = "Enums reducer complete: Verter expands `${Status}` to `\"active\" | \"done\" | \"idle\"` via TemplateLiteralReduce (verified). NOT oracle-liftable — tsgo hover displays the template-literal ORIGIN `${Status}`, not the expansion (oracle admission Reject(DeferredConstruct(template-literal))); lift pending a template-literal expansion probe"]
fn enum_template_literal_over_string_enum_produces_value_union() {
    // TS7 contract: `${Status}` is a template-literal type that expands the
    // string-enum value union, producing `"idle" | "active" | "done"`.
    let host = make_host_with_footprint();
    upsert_enum_fixture(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/enums.ts",
        "StatusValueUnion",
        &[],
        ProjectionMode::Expanded,
    );

    assert_literal_union(&expr, &["active", "done", "idle"]);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
#[ignore = "Enums reducer complete: Verter projects `keyof typeof Color` to `\"Blue\" | \"Green\" | \"Red\"` via KeyOf over the enum `typeof` object (verified). NOT oracle-liftable — `keyof typeof Enum` is not a recognized source-walk carve-out shape (oracle admission Reject(SourceUnresolvedOrCyclic)); lift pending a keyof-typeof-of-enum carve-out + distributive-identity probe"]
fn enum_keyof_typeof_numeric_yields_member_name_union() {
    // TS7 contract: `keyof typeof Color` is the union of the enum's declared
    // member names, NOT the reverse-mapped numeric keys. So
    // `ColorKeyUnion = "Red" | "Green" | "Blue"`. (For numeric enums TS also
    // exposes a numeric index signature on the typeof Enum value, but that
    // doesn't appear in the keyof.)
    let host = make_host_with_footprint();
    upsert_enum_fixture(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/enums.ts",
        "ColorKeyUnion",
        &[],
        ProjectionMode::Expanded,
    );

    assert_literal_union(&expr, &["Blue", "Green", "Red"]);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
#[ignore = "Enums reducer complete: Verter projects `keyof typeof Status` to `\"Active\" | \"Done\" | \"Idle\"` (verified). NOT oracle-liftable — `keyof typeof Enum` is not a recognized source-walk carve-out shape (oracle admission Reject(SourceUnresolvedOrCyclic)); lift pending a keyof-typeof-of-enum carve-out + distributive-identity probe"]
fn enum_keyof_typeof_string_yields_member_name_union() {
    // TS7 contract: `keyof typeof Status` = `"Idle" | "Active" | "Done"`.
    let host = make_host_with_footprint();
    upsert_enum_fixture(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/enums.ts",
        "StatusKeyUnion",
        &[],
        ProjectionMode::Expanded,
    );

    assert_literal_union(&expr, &["Active", "Done", "Idle"]);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
#[ignore = "Enums reducer complete: Verter resolves the const-enum member `Direction.Up` to the string literal `\"UP\"` (verified). NOT oracle-liftable — tsgo hover displays the const-enum member NOMINALLY as `Direction.Up` (oracle admission Reject(EnumMemberOrQualified)); lift pending oracle-infra for nominal enum-member display"]
fn enum_const_enum_member_resolves_to_inlined_string_literal() {
    // TS7 contract: `Direction.Up` from a `const enum` produces the string
    // literal `"UP"` (const enums inline at use sites; the type-level
    // projection equals the assigned literal).
    let host = make_host_with_footprint();
    upsert_enum_fixture(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/enums.ts",
        "DirectionUp",
        &[],
        ProjectionMode::Expanded,
    );

    assert_string_literal(&expr, "UP");
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
#[ignore = "Enums reducer complete: Verter selects the `Status.Idle` arm and projects its payload `{ hint: string }` (verified) — `Status.Idle` lowers to the SAME literal on both the union arm and the Extract probe, so the shared object relation + indexed access pick the right arm. NOT oracle-liftable — tsgo hover displays the indexed-access ORIGIN, not the resolved object (oracle admission Reject(DeferredConstruct(indexed-access))); lift pending an indexed-access expansion probe"]
fn enum_discriminant_extract_projects_matching_arm_payload() {
    // TS7 contract: `Extract<StatefulNode, { status: Status.Idle }>["payload"]`
    // selects the `Status.Idle` arm and projects its payload object:
    //   { hint: string }
    let host = make_host_with_footprint();
    upsert_enum_fixture(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/enums.ts",
        "IdleNodePayload",
        &[],
        ProjectionMode::Expanded,
    );

    let props = object_props(&expr);
    assert_eq!(prop_names(&props), vec!["hint"]);
    assert!(!props["hint"].optional);
    assert_primitive(&props["hint"].ty, PrimitiveName::String);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

// ---------------------------------------------------------------------------
// Gate-discrimination negatives — the `Enum.Member` projection is a TYPED,
// GATED fallback, NOT a generic dotted-name heuristic. These run (not ignored).
// ---------------------------------------------------------------------------

#[test]
fn enum_member_projection_is_gated_to_declared_members() {
    // DISCRIMINATING (member-existence gate): a DECLARED member projects to
    // its value literal, but an UNDECLARED member name must NOT be minted into
    // a bogus literal — it stays a semantic miss. Removing the
    // `enum_members.get(member)` gate would project `Color.Blue` to garbage.
    let host = make_host_with_footprint();
    upsert_ts(
        &host,
        "/fixtures/enum_gate.ts",
        "export enum Color { Red, Green }\n\
         export type RealMember = Color.Red;\n\
         export type GhostMember = Color.Blue;\n",
    );

    let (real, _) = resolve_expr(
        &host,
        "/fixtures/enum_gate.ts",
        "RealMember",
        &[],
        ProjectionMode::Expanded,
    );
    assert_number_literal(&real, 0.0);

    let (ghost, _) = resolve_expr(
        &host,
        "/fixtures/enum_gate.ts",
        "GhostMember",
        &[],
        ProjectionMode::Expanded,
    );
    assert!(
        ghost.is_unknown(),
        "an UNDECLARED enum member must stay a semantic miss, never a minted \
         literal: {ghost:?}"
    );
}

#[test]
fn non_enum_dotted_namespace_member_is_not_enum_projected() {
    // NON-REGRESSION / resolution-ordering guard (NOT a reducer discriminator):
    // a `Namespace.Type` dotted reference is NOT an enum member and must
    // resolve through the existing namespace-member path to the namespace's
    // declared type. This passes on the BASE tree too — there was no enum hook
    // to hijack it — so it does NOT go red pre-change and is NOT proof the enum
    // reducer landed; it guards that the enum-member hook, once added, does not
    // OVER-BROADLY intercept a non-enum dotted ref. A sibling enum keeps the
    // enum machinery live; the hook keys on the typed `ValueDeclKind::Enum`
    // fact, so the non-enum namespace prefix falls through unaffected.
    let host = make_host_with_footprint();
    upsert_ts(
        &host,
        "/fixtures/enum_ns_gate.ts",
        "export enum Color { Red }\n\
         export namespace Shapes {\n  export type Circle = { radius: number };\n}\n\
         export type C = Shapes.Circle;\n",
    );

    let (expr, _) = resolve_expr(
        &host,
        "/fixtures/enum_ns_gate.ts",
        "C",
        &[],
        ProjectionMode::Expanded,
    );
    let props = object_props(&expr);
    assert_eq!(prop_names(&props), vec!["radius"]);
    assert_primitive(&props["radius"].ty, PrimitiveName::Number);
}

// ---------------------------------------------------------------------------
// Operator-origin composition regression fences — the 4 operator-origin parity
// rows above (`${Enum}`, `keyof typeof Enum` ×2, the `Extract<…>[…]`
// discriminant) are `#[ignore]`d for ORACLE reasons only (tsgo hover shows the
// operator ORIGIN, not the expansion), so their RESOLVER correctness is checked
// only under `--ignored`. These siblings pin the resolver composition DIRECTLY,
// un-ignored, over a LOCAL fixture (NOT a parity row) — a permanent fence that
// goes red if `${Enum}` (TemplateLiteralReduce) or `keyof typeof Enum` (KeyOf
// over the enum `typeof` object) regresses.
// ---------------------------------------------------------------------------

#[test]
fn enum_template_literal_value_union_resolver_fence() {
    // `${Status}` over a STRING enum expands, via the real TemplateLiteralReduce
    // path, to the union of the member VALUES: `"active" | "done" | "idle"`.
    let host = make_host_with_footprint();
    upsert_ts(
        &host,
        "/fixtures/enum_template_fence.ts",
        "export enum Status { Idle = \"idle\", Active = \"active\", Done = \"done\" }\n\
         export type StatusValues = `${Status}`;\n",
    );

    let (expr, _) = resolve_expr(
        &host,
        "/fixtures/enum_template_fence.ts",
        "StatusValues",
        &[],
        ProjectionMode::Expanded,
    );

    assert_literal_union(&expr, &["active", "done", "idle"]);
}

#[test]
fn enum_keyof_typeof_member_name_union_resolver_fence() {
    // `keyof typeof Color` over a NUMERIC enum projects, via the real KeyOf path
    // over the enum `typeof` object, to the union of the member NAMES:
    // `"Blue" | "Green" | "Red"` (the declared names, NOT the reverse-mapped
    // numeric keys).
    let host = make_host_with_footprint();
    upsert_ts(
        &host,
        "/fixtures/enum_keyof_fence.ts",
        "export enum Color { Red, Green, Blue }\n\
         export type ColorKeys = keyof typeof Color;\n",
    );

    let (expr, _) = resolve_expr(
        &host,
        "/fixtures/enum_keyof_fence.ts",
        "ColorKeys",
        &[],
        ProjectionMode::Expanded,
    );

    assert_literal_union(&expr, &["Blue", "Green", "Red"]);
}

// ---------------------------------------------------------------------------
// Deferred-value member completeness fences — a member whose VALUE is deferred
// (a computed `1 << 2`, a bare member after an unknown running value) keeps its
// NAME on every projection surface and degrades its VALUE to the narrowest
// sound primitive. Dropping a known member NAME is false absence, NOT a
// deferred-value evaluation. These run (un-ignored) over LOCAL fixtures and pin
// the resolver directly: `typeof Enum`, `keyof typeof Enum`, and `Enum.Member`
// must all see EVERY member.
// ---------------------------------------------------------------------------

#[test]
fn enum_keyof_typeof_includes_deferred_member_names_fence() {
    // `keyof typeof E` over `enum E { A = 1 << 2, B, C = 5, D }` — `A`/`B` are
    // VALUE-deferred but their NAMES are members and MUST appear in the key
    // union. Expect all four names, not just the folded `C`/`D`.
    let host = make_host_with_footprint();
    upsert_ts(
        &host,
        "/fixtures/enum_keyof_deferred_fence.ts",
        "export enum E { A = 1 << 2, B, C = 5, D }\n\
         export type EKeys = keyof typeof E;\n",
    );

    let (expr, _) = resolve_expr(
        &host,
        "/fixtures/enum_keyof_deferred_fence.ts",
        "EKeys",
        &[],
        ProjectionMode::Expanded,
    );

    assert_literal_union(&expr, &["A", "B", "C", "D"]);
}

#[test]
fn enum_typeof_degrades_deferred_members_to_sound_primitive_fence() {
    // `typeof E` over `enum E { A = 1 << 2, B, C = 5, D }` — one synthetic
    // property per member: the folded members carry their literal (`C: 5`,
    // `D: 6`); the deferred members carry their DEGRADED sound primitive
    // (`A: number`, `B: number`), never vanish.
    let host = make_host_with_footprint();
    upsert_ts(
        &host,
        "/fixtures/enum_typeof_deferred_fence.ts",
        "export enum E { A = 1 << 2, B, C = 5, D }\n\
         export type ETypeof = typeof E;\n",
    );

    let (expr, _) = resolve_expr(
        &host,
        "/fixtures/enum_typeof_deferred_fence.ts",
        "ETypeof",
        &[],
        ProjectionMode::Expanded,
    );

    let props = object_props(&expr);
    assert_eq!(prop_names(&props), vec!["A", "B", "C", "D"]);
    assert_primitive(&props["A"].ty, PrimitiveName::Number);
    assert_primitive(&props["B"].ty, PrimitiveName::Number);
    assert_number_literal(&props["C"].ty, 5.0);
    assert_number_literal(&props["D"].ty, 6.0);
}

#[test]
fn all_deferred_enum_keyof_typeof_yields_member_name_union_fence() {
    // An ALL-deferred enum still has member NAMES. `keyof typeof Flags` =
    // `"A" | "B"` (never empty / `never`), even though no value folds: the
    // `typeof` object surfaces every member, degrading the values.
    let host = make_host_with_footprint();
    upsert_ts(
        &host,
        "/fixtures/enum_all_deferred_fence.ts",
        "export enum Flags { A = 1 << 0, B = 1 << 1 }\n\
         export type FlagKeys = keyof typeof Flags;\n",
    );

    let (expr, _) = resolve_expr(
        &host,
        "/fixtures/enum_all_deferred_fence.ts",
        "FlagKeys",
        &[],
        ProjectionMode::Expanded,
    );

    assert_literal_union(&expr, &["A", "B"]);
}

#[test]
fn enum_member_deferred_value_resolves_to_degraded_type_fence() {
    // `E.A` where `A`'s value is deferred (`1 << 2`) resolves to its DEGRADED
    // sound primitive (`number`), NOT a semantic miss. The member is DECLARED;
    // only its VALUE is deferred. (Contrast the gated UNDECLARED-member miss in
    // `enum_member_projection_is_gated_to_declared_members`.)
    let host = make_host_with_footprint();
    upsert_ts(
        &host,
        "/fixtures/enum_member_deferred_fence.ts",
        "export enum E { A = 1 << 2, B = 5 }\n\
         export type EA = E.A;\n",
    );

    let (expr, _) = resolve_expr(
        &host,
        "/fixtures/enum_member_deferred_fence.ts",
        "EA",
        &[],
        ProjectionMode::Expanded,
    );

    assert_primitive(&expr, PrimitiveName::Number);
}

// ---------------------------------------------------------------------------
// Cross-file / barrel `Enum.Member` projection — the prefix of `Enum.Member`
// resolves to its enum VALUE decl through the SAME export-target chase
// `typeof Enum` uses, so a barrel-re-exported enum projects its members exactly
// like a local one. Multi-file `upsert`; runs un-ignored.
// ---------------------------------------------------------------------------

#[test]
fn enum_member_projection_resolves_through_barrel_reexport() {
    // CROSS-FILE: `E.A` where `E` is imported from a BARREL that re-exports the
    // enum from a leaf (`export { E } from "./leaf"`). The prefix `E` resolves
    // to the BARREL (the import target), which declares NO local enum value
    // decl — only a re-export. The member hook must chase the re-export to the
    // leaf's enum value decl (the SAME export-target chase `typeof E` uses), so
    // `E.A` projects to the member literal `0` rather than a semantic miss. This
    // matches `typeof E`'s established cross-file behaviour.
    let host = make_host_with_footprint();
    upsert_ts(
        &host,
        "/fixtures/enum_xfile_leaf.ts",
        "export enum E { A, B }\n",
    );
    upsert_ts(
        &host,
        "/fixtures/enum_xfile_barrel.ts",
        "export { E } from \"/fixtures/enum_xfile_leaf\";\n",
    );
    upsert_ts(
        &host,
        "/fixtures/enum_xfile_main.ts",
        "import { E } from \"/fixtures/enum_xfile_barrel\";\n\
         export type X = E.A;\n",
    );

    let (expr, _) = resolve_expr(
        &host,
        "/fixtures/enum_xfile_main.ts",
        "X",
        &[],
        ProjectionMode::Expanded,
    );

    assert_number_literal(&expr, 0.0);
}

#[test]
fn cross_file_enum_member_projection_invalidates_on_leaf_edit() {
    // CACHE CORRECTNESS: the cross-file `E.A` projection reaches the leaf's enum
    // value decl through the SAME fact-traced chase `typeof E` uses, so the
    // leaf's content version enters the consuming query's read-set. Editing the
    // LEAF's member value MUST invalidate the warm `E.A` result. Resolve `E.A`
    // cold (= 0), reseed `A = 5` on the leaf, re-resolve: the result must follow
    // the edit (= 5), never serve the stale `0`.
    let host = make_host_with_footprint();
    upsert_ts(
        &host,
        "/fixtures/enum_xinval_leaf.ts",
        "export enum E { A, B }\n",
    );
    upsert_ts(
        &host,
        "/fixtures/enum_xinval_barrel.ts",
        "export { E } from \"/fixtures/enum_xinval_leaf\";\n",
    );
    upsert_ts(
        &host,
        "/fixtures/enum_xinval_main.ts",
        "import { E } from \"/fixtures/enum_xinval_barrel\";\n\
         export type X = E.A;\n",
    );

    let (cold, _) = resolve_expr(
        &host,
        "/fixtures/enum_xinval_main.ts",
        "X",
        &[],
        ProjectionMode::Expanded,
    );
    assert_number_literal(&cold, 0.0);

    // Reseed `A = 5` on the LEAF — a member-value edit on the declaring file.
    upsert_ts(
        &host,
        "/fixtures/enum_xinval_leaf.ts",
        "export enum E { A = 5, B }\n",
    );

    let (warm, _) = resolve_expr(
        &host,
        "/fixtures/enum_xinval_main.ts",
        "X",
        &[],
        ProjectionMode::Expanded,
    );
    assert_number_literal(&warm, 5.0);
}
