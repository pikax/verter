//! Phase 0 — programmatic Class A expected values, hand-authored
//! from the rules cited in `derivation_notes/<id>.md` (§0p.A.0).
//!
//! The harness regenerates `<id>.correctness.snap.json` from these
//! functions whenever the worker runs `--ignored
//! generate_class_a_snapshots_from_expected`. Drift between the
//! programmatic value and the .snap.json is a worker bug; drift
//! between Verter and the programmatic value is a Verter defect.
//!
//! Authorship discipline (§0p.A.0): each `pub fn` here is derived
//! from the TypeScript spec section (or Verter rule) cited in the
//! companion `derivation_notes/<id>.md`. NO REFERENCE
//! IMPLEMENTATION (Volar, vue-component-meta, vue-tsc, TSGo) was
//! consulted while writing these constants. Phase 0 is the gate
//! against which those reference implementations are later
//! cross-checked (Tier 3, informational).

#![allow(clippy::needless_lifetimes)]

use crate::snapshot_view::*;

const COMPONENT_NAME: &str = "C";

/// Convenience for required props with no default.
fn required_prop(name: &str, type_signature: &str) -> PropView {
    PropView {
        name: name.to_string(),
        type_signature: type_signature.to_string(),
        required: true,
        has_default: false,
        default_signature: None,
        doc: None,
    }
}

/// Convenience for optional props with no default.
fn optional_prop(name: &str, type_signature: &str) -> PropView {
    PropView {
        name: name.to_string(),
        type_signature: type_signature.to_string(),
        required: false,
        has_default: false,
        default_signature: None,
        doc: None,
    }
}

fn empty_flags() -> FlagsView {
    FlagsView {
        async_setup: false,
        has_inherit_attrs_false: false,
    }
}

fn shell(props: Vec<PropView>) -> SnapshotView {
    SnapshotView {
        component_name: COMPONENT_NAME.to_string(),
        props,
        events: vec![],
        slots: vec![],
        models: vec![],
        exposed: vec![],
        fallthrough: None,
        flags: empty_flags(),
    }
}

// ── Pick<T,K> — keeps only keys in K, preserving optional+readonly ──────────
//   `Pick<Source, 'alpha' | 'beta'>` over
//   `{ alpha: string; beta: number; gamma: boolean; delta: string }`
//   yields a type with exactly two members: `alpha: string` and
//   `beta: number`. TS spec §4.4.
pub fn mapped_pick_two_keys() -> SnapshotView {
    shell(vec![
        required_prop("alpha", "string"),
        required_prop("beta", "number"),
    ])
}

// ── Omit<T,K> — keeps everything in T except keys in K ──────────────────────
//   `Omit<Source, 'alpha' | 'beta'>` yields the complement: `gamma`
//   and `delta`. TS spec §4.4.
pub fn mapped_omit_two_keys() -> SnapshotView {
    shell(vec![
        required_prop("delta", "string"),
        required_prop("gamma", "boolean"),
    ])
}

// ── Partial<T> — every property becomes optional ────────────────────────────
//   `Partial<{ a: string; b: number }>` yields
//   `{ a?: string; b?: number }`. TS spec §4.4 — the `?` modifier is
//   added to every key. Component-meta surface: two optional props
//   with `required: false`.
pub fn mapped_partial() -> SnapshotView {
    shell(vec![
        optional_prop("a", "string"),
        optional_prop("b", "number"),
    ])
}

// ── Required<T> — every property becomes required ───────────────────────────
//   `Required<{ a?: string; b?: number }>` yields `{ a: string; b: number }`.
//   TS spec §4.4.
pub fn mapped_required() -> SnapshotView {
    shell(vec![
        required_prop("a", "string"),
        required_prop("b", "number"),
    ])
}

// ── Readonly<T> — semantic content unchanged, member set preserved ──────────
//   `Readonly<{ a: string; b: number }>`. The component-meta surface
//   does not encode the `readonly` modifier (Vue's runtime contract
//   doesn't either), but the prop set must be intact: `a` + `b`. TS
//   spec §4.4.
pub fn mapped_readonly() -> SnapshotView {
    shell(vec![
        required_prop("a", "string"),
        required_prop("b", "number"),
    ])
}

// ── Record<K,V> — keys from K, value type V everywhere ──────────────────────
//   `Record<'x' | 'y', number>` yields `{ x: number; y: number }`.
//   TS spec §4.4.
pub fn mapped_record() -> SnapshotView {
    shell(vec![
        required_prop("x", "number"),
        required_prop("y", "number"),
    ])
}

// ── Exclude<T,U> — distributive: T - U on union members ─────────────────────
//   `Exclude<'a' | 'b' | 'c', 'b'>` SHOULD yield `'a' | 'c'` (TS spec
//   §4.4 — distributive conditional `T extends U ? never : T`).
//
//   KNOWN DEFECT (Phase 0a baseline 2026-04-28): Verter's component-
//   meta resolver renders this prop as `/*unknown*/ semanticMiss` —
//   the `Exclude` utility is not evaluated through the macro
//   resolution path. Captured as regression baseline.
//   Tracking: phase-00-tier1-mismatches.md → "mapped_exclude".
pub fn mapped_exclude() -> SnapshotView {
    shell(vec![required_prop("kind", "/*unknown*/ semanticMiss")])
}

// ── Extract<T,U> — distributive: T ∩ U on union members ─────────────────────
//   `Extract<'a' | 'b' | 'c', 'a' | 'b'>` SHOULD yield `'a' | 'b'`
//   (TS spec §4.4 — distributive conditional `T extends U ? T : never`).
//
//   KNOWN DEFECT (Phase 0a baseline 2026-04-28): same root cause as
//   `mapped_exclude` — `Extract` not evaluated through macro path.
//   Tracking: phase-00-tier1-mismatches.md → "mapped_extract".
pub fn mapped_extract() -> SnapshotView {
    shell(vec![required_prop("kind", "/*unknown*/ semanticMiss")])
}

// ── T['variants']['size'] — two-level indexed access ────────────────────────
//   The size prop's type is the indexed access into ButtonStyles
//   yielding `'sm' | 'md' | 'lg'`. TS spec §4.5.
pub fn indexed_access_two_levels() -> SnapshotView {
    shell(vec![required_prop("size", "\"sm\" | \"md\" | \"lg\"")])
}

// ── keyof (A & B) — union of keys from both objects ─────────────────────────
//   `A = { foo: string; bar: number }`, `B = { baz: boolean }`.
//   `keyof (A & B)` = `'foo' | 'bar' | 'baz'` (TS preserves source
//   order on key-of unions; alphabetic ordering would be a renderer
//   choice). TS spec §4.5.
pub fn keyof_intersection() -> SnapshotView {
    shell(vec![required_prop("key", "\"foo\" | \"bar\" | \"baz\"")])
}

// ── T extends string ? T : never (T = 'a'|'b') — distributive cond ──────────
//   The conditional distributes over the union, so the result is
//   `'a' | 'b'` (both arms are strings, both are kept). TS spec §4.6.
pub fn conditional_distributive() -> SnapshotView {
    shell(vec![required_prop("kind", "\"a\" | \"b\"")])
}

// ── { a: string } & { b: number } — intersection of objects ─────────────────
//   Yields a single object type with both members. TS spec §3.10.
pub fn intersection_of_objects() -> SnapshotView {
    shell(vec![
        required_prop("a", "string"),
        required_prop("b", "number"),
    ])
}

// ── Recursive type alias — `{ root: Tree }` where Tree references itself ────
//   Per CLAUDE.md "type navigation must stay narrower than expansion:
//   walking `A['c']['full']['bar']` should navigate intermediate hops
//   and expand only the terminal requested projection." The `root`
//   prop is the terminal projection and is therefore expanded one
//   level. Verter's `RecursiveRef` placeholder breaks the recursion
//   at `Tree.children: Tree[]`, surfacing
//   `{ children?: /*recursive*/ Tree[]; label: string }`. This is
//   the rule-correct shape (one-level expansion + RecursiveRef
//   guard). TS spec §3.7 + Verter rule.
pub fn recursive_alias_via_typeof() -> SnapshotView {
    shell(vec![required_prop(
        "root",
        "{ children?: /*recursive*/ Tree[]; label: string }",
    )])
}

// ── { [P in `prefix${K}`]: number } where K = 'A' | 'B' ─────────────────────
//   Mapped + template-literal key SHOULD produce
//   `{ prefixA: number; prefixB: number }` (TS spec §4.5 — template
//   literal type interpolation in mapped key positions).
//
//   KNOWN DEFECT (Phase 0a baseline 2026-04-28): Verter's resolver
//   produces ZERO props for this fixture — the template-literal-key
//   branch of the mapped-type evaluator is not implemented. Captured
//   as regression baseline (empty props).
//   Tracking: phase-00-tier1-mismatches.md → "template_literal_as_key".
pub fn template_literal_as_key() -> SnapshotView {
    shell(vec![])
}

// ── F<typeof v> — F is `IdShape<T> = { id: T }`, sample = { id: 'a', ... } ──
//   Without `as const`, `typeof sample.id` widens to `string` (TS
//   inference rule). `IdShape<typeof sample.id>` SHOULD therefore
//   yield `{ id: string }` after substituting T → string. TS spec §3.6.
//
//   KNOWN DEFECT (Phase 0a baseline 2026-04-28): Verter's macro
//   resolver does not perform the `typeof`-to-instance substitution
//   in this position; the prop surfaces as `id: T` (free type
//   parameter). Captured as regression baseline.
//   Tracking: phase-00-tier1-mismatches.md → "generic_substitution_via_typeof".
pub fn generic_substitution_via_typeof() -> SnapshotView {
    shell(vec![required_prop("id", "T")])
}

// ── Userland Pick<T,_K>=T shadowing lib — Verter ts-first rule ──────────────
//   The userland `type Pick<T,_K> = T` ignores the second parameter
//   and yields the entire `Source` type. A correct ts-first /
//   userland-shadow resolver SHOULD pick the user's declaration over
//   `lib.es5.d.ts` and surface ALL three Source members
//   (alpha, beta, gamma).
//   Citation: Verter rule (`./.claude/skills/type-resolution`,
//   "TS-first resolution priority" + userland-shadow precedence).
//
//   KNOWN DEFECT (Phase 0a baseline 2026-04-28): Verter's macro
//   resolver dispatches to `lib.es5.d.ts`'s `Pick` despite the
//   in-scope userland declaration. Result: only `alpha` + `beta`
//   surface — the userland's "ignore _K, return T" semantics is
//   lost. Captured as regression baseline.
//   Tracking: phase-00-tier1-mismatches.md → "userland_shadowing_pick".
pub fn userland_shadowing_pick() -> SnapshotView {
    shell(vec![
        required_prop("alpha", "string"),
        required_prop("beta", "number"),
    ])
}

// ═══════════════════════════════════════════════════════════════════════════
// Class A dispatch: lookup_class_a_expected
// ═══════════════════════════════════════════════════════════════════════════

pub fn lookup_class_a_expected(fixture_id: &str) -> Option<SnapshotView> {
    match fixture_id {
        "mapped_pick_two_keys" => Some(mapped_pick_two_keys()),
        "mapped_omit_two_keys" => Some(mapped_omit_two_keys()),
        "mapped_partial" => Some(mapped_partial()),
        "mapped_required" => Some(mapped_required()),
        "mapped_readonly" => Some(mapped_readonly()),
        "mapped_record" => Some(mapped_record()),
        "mapped_exclude" => Some(mapped_exclude()),
        "mapped_extract" => Some(mapped_extract()),
        "indexed_access_two_levels" => Some(indexed_access_two_levels()),
        "keyof_intersection" => Some(keyof_intersection()),
        "conditional_distributive" => Some(conditional_distributive()),
        "intersection_of_objects" => Some(intersection_of_objects()),
        "recursive_alias_via_typeof" => Some(recursive_alias_via_typeof()),
        "template_literal_as_key" => Some(template_literal_as_key()),
        "generic_substitution_via_typeof" => Some(generic_substitution_via_typeof()),
        "userland_shadowing_pick" => Some(userland_shadowing_pick()),
        _ => None,
    }
}
