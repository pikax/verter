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

// ── Userland Pick<T,_K> = T shadowing lib's mapped Pick ─────────────────────
//   The userland alias ignores its `K` parameter and returns the
//   entire `T`. With it in scope, `defineProps<Pick<Cfg, 'alpha'>>()`
//   resolves to `Cfg`, so all three members surface (alpha + beta +
//   gamma — sorted alphabetically by the snapshot projection). The
//   lib's mapped `Pick<T, K>` would have surfaced only `alpha`.
//
//   Rule citation: Verter rule `./.claude/skills/type-resolution`
//   ("user shadowing wins" / TS-first resolution priority).
//   `phase-00-tier1-mismatches.md` row 5; closed by Phase 5h §5.10.
pub fn userland_shadowing_pick() -> SnapshotView {
    shell(vec![
        required_prop("alpha", "string"),
        required_prop("beta", "number"),
        required_prop("gamma", "boolean"),
    ])
}

// ── Exclude<'a' | 'b' | 'c', 'b'> — distributive conditional reduction ──────
//   Per TS spec §4.4: `Exclude<T,U> = T extends U ? never : T`
//   distributes over the union T and drops every member matching U.
//   `Exclude<'a' | 'b' | 'c', 'b'>` therefore reduces to `'a' | 'c'`
//   (the survivors after filtering out `'b'`). The renderer prints
//   the union in source order (`"a" | "c"`), preserving the
//   left-to-right occurrence of the surviving members in T.
//
//   Rule citation: TS spec §4.4 (distributive conditional / Exclude).
//   `phase-00-tier1-mismatches.md` row 1; closed by Phase 5i §5.11.
pub fn mapped_exclude() -> SnapshotView {
    shell(vec![required_prop("kind", "\"a\" | \"c\"")])
}

// ── Extract<'a' | 'b' | 'c', 'a' | 'b'> — distributive conditional ──────────
//   Per TS spec §4.4: `Extract<T,U> = T extends U ? T : never`
//   distributes over T and keeps every member assignable to U.
//   `Extract<'a' | 'b' | 'c', 'a' | 'b'>` therefore reduces to
//   `'a' | 'b'` (the survivors are the source members that are
//   assignable to one of the filter literals). The renderer prints
//   the union in source order (`"a" | "b"`).
//
//   Rule citation: TS spec §4.4 (distributive conditional / Extract).
//   `phase-00-tier1-mismatches.md` row 2; closed by Phase 5i §5.11.
pub fn mapped_extract() -> SnapshotView {
    shell(vec![required_prop("kind", "\"a\" | \"b\"")])
}

// ── { [K in 'A' | 'B' as `prefix${K}`]: number } — template-literal key ─────
//   Per TS spec §4.5: a mapped type's `as <expr>` clause re-maps
//   each iterated key through `<expr>`. When the expression is a
//   template-literal type referencing the mapper binder K, each
//   iterated K substitutes into the template; the folded literal
//   becomes the produced surface name. Iterating
//   K = 'A' | 'B' through `\`prefix${K}\`` produces members
//   `prefixA: number` and `prefixB: number`. Both are required
//   (no `?` modifier). The snapshot projection sorts members
//   alphabetically by name, so the surface order is
//   `[prefixA, prefixB]`.
//
//   Rule citation: TS spec §4.5 (template-literal types in mapped
//   key positions). `phase-00-tier1-mismatches.md` row 3; closed by
//   Phase 5i §5.11 (re-homed from 5k per §5.13 r15 table).
pub fn template_literal_as_key() -> SnapshotView {
    shell(vec![
        required_prop("prefixA", "number"),
        required_prop("prefixB", "number"),
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

// ═══════════════════════════════════════════════════════════════════════════
// Phase 0b — Class A property fixtures (component-meta macros).
// ═══════════════════════════════════════════════════════════════════════════

/// Convenience for non-required props that have a default value
/// declared via `withDefaults`. Vue's contract: a prop with a
/// withDefaults entry is no longer required (the default makes the
/// prop's runtime value present).
fn defaulted_prop(name: &str, type_signature: &str, default_signature: &str) -> PropView {
    PropView {
        name: name.to_string(),
        type_signature: type_signature.to_string(),
        required: false,
        has_default: true,
        default_signature: Some(default_signature.to_string()),
        doc: None,
    }
}

// ── defineProps + withDefaults — Verter macros §props ───────────────────────
//   `name: string` is required; `count?: number` becomes non-required
//   with `has_default = true` and `default_value = "0"` because
//   `withDefaults({ count: 0 })` populates the default. The original
//   optional marker is preserved in the type system but the
//   component-meta surface uses `required: false` because the
//   runtime value is always defined.
pub fn fixture_props_with_defaults() -> SnapshotView {
    shell(vec![
        defaulted_prop("count", "number", "0"),
        required_prop("name", "string"),
    ])
}

// ── defineEmits<T> — Verter macros §emits ───────────────────────────────────
//   One event `click` whose parameter list is `[evt: string]` —
//   tuple-form, single labelled element of primitive type.
pub fn fixture_events_typed() -> SnapshotView {
    SnapshotView {
        component_name: COMPONENT_NAME.to_string(),
        props: vec![],
        events: vec![EventView {
            name: "click".to_string(),
            params_signature: "[evt: string]".to_string(),
        }],
        slots: vec![],
        models: vec![],
        exposed: vec![],
        fallthrough: None,
        flags: empty_flags(),
    }
}

// ── defineExpose — Verter macros §expose ────────────────────────────────────
//   §0.6.1: Vue's documented public API uses the value form
//   `defineExpose({ ... })`. Each exposed binding declares its
//   function type explicitly so the resolver surfaces a typed
//   signature. The view sorts exposed entries alphabetically.
pub fn fixture_exposed_methods() -> SnapshotView {
    SnapshotView {
        component_name: COMPONENT_NAME.to_string(),
        props: vec![],
        events: vec![],
        slots: vec![],
        models: vec![],
        exposed: vec![
            ExposedView {
                name: "focus".to_string(),
                type_signature: "() => void".to_string(),
            },
            ExposedView {
                name: "reset".to_string(),
                type_signature: "() => void".to_string(),
            },
        ],
        fallthrough: None,
        flags: empty_flags(),
    }
}

// ── inheritAttrs: false — CLAUDE.md §Fallthrough ────────────────────────────
//   `defineOptions({ inheritAttrs: false })` zeros the fallthrough
//   surface. The single declared prop (`disabled`) survives on
//   `props`; the projection emits `Some(FallthroughView { inherit_attrs: false, ... })`
//   with surface_signature `{}` (no inherited members).
pub fn fixture_fallthrough_inherit() -> SnapshotView {
    SnapshotView {
        component_name: COMPONENT_NAME.to_string(),
        props: vec![optional_prop("disabled", "boolean")],
        events: vec![],
        slots: vec![],
        models: vec![],
        exposed: vec![],
        fallthrough: Some(FallthroughView {
            inherit_attrs: false,
            surface_signature: "{}".to_string(),
        }),
        flags: FlagsView {
            async_setup: false,
            has_inherit_attrs_false: true,
        },
    }
}

// ── single component root inheriting child surface — CLAUDE.md §Fallthrough ─
//   The wrapper has zero declared props. Its single component root
//   `<Inner />` exposes one prop `label: string`. That prop
//   propagates as the inherited fallthrough surface. The projection
//   emits `Some(FallthroughView { inherit_attrs: true, surface_signature: "{ label: string ... }" })`
//   where the signature carries a `from component:/inner.vue` source
//   tag from `format_inherited_sources`.
pub fn fixture_fallthrough_root_inherit() -> SnapshotView {
    SnapshotView {
        component_name: COMPONENT_NAME.to_string(),
        props: vec![],
        events: vec![],
        slots: vec![],
        models: vec![],
        exposed: vec![],
        fallthrough: Some(FallthroughView {
            inherit_attrs: true,
            surface_signature: "{ label: string /* from component:/inner.vue */ }".to_string(),
        }),
        flags: empty_flags(),
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Class A dispatch: lookup_class_a_expected
// ═══════════════════════════════════════════════════════════════════════════

pub fn lookup_class_a_expected(fixture_id: &str) -> Option<SnapshotView> {
    match fixture_id {
        // Phase 0a — mapped + structural.
        "mapped_pick_two_keys" => Some(mapped_pick_two_keys()),
        "mapped_omit_two_keys" => Some(mapped_omit_two_keys()),
        "mapped_partial" => Some(mapped_partial()),
        "mapped_required" => Some(mapped_required()),
        "mapped_readonly" => Some(mapped_readonly()),
        "mapped_record" => Some(mapped_record()),
        "indexed_access_two_levels" => Some(indexed_access_two_levels()),
        "keyof_intersection" => Some(keyof_intersection()),
        "conditional_distributive" => Some(conditional_distributive()),
        "intersection_of_objects" => Some(intersection_of_objects()),
        "recursive_alias_via_typeof" => Some(recursive_alias_via_typeof()),
        // Phase 5h §5.10 — fixture authored after the
        // ScopeShadowing thread closes the userland-shadow-pick gap.
        "userland_shadowing_pick" => Some(userland_shadowing_pick()),
        // Phase 5i §5.11 — fixtures authored after the
        // Exclude/Extract literal-type reduction (rows 1, 2) and
        // the mapper name_remap + template-literal fold (row 3,
        // re-homed from 5k per §5.13 r15) close the deferred gaps.
        "mapped_exclude" => Some(mapped_exclude()),
        "mapped_extract" => Some(mapped_extract()),
        "template_literal_as_key" => Some(template_literal_as_key()),
        // Phase 0b — component-meta property macros (5 of 7 land;
        // `fixture_slots_typed` and `fixture_models` deferred per
        // §0p.A.4 case 2 — see `phase-00b-tier1-mismatches.md`).
        "fixture_props_with_defaults" => Some(fixture_props_with_defaults()),
        "fixture_events_typed" => Some(fixture_events_typed()),
        "fixture_exposed_methods" => Some(fixture_exposed_methods()),
        "fixture_fallthrough_inherit" => Some(fixture_fallthrough_inherit()),
        "fixture_fallthrough_root_inherit" => Some(fixture_fallthrough_root_inherit()),
        _ => None,
    }
}
