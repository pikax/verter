//! Phase 0 — fixture registry. Each entry maps to one synthetic Vue
//! project and a snapshot file. `class` discriminates the
//! correctness ground-truth tier (Class A) from the regression
//! baselines (Class B + C).
//!
//! Phase 0a authors the 16 Class A fixtures listed below (11
//! mapped-type + 5 structural). Phase 0b will append the 7 Class A
//! component-meta property fixtures plus the Class B + C regression
//! baselines.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FixtureClass {
    /// Hand-derived expected output from TS spec / Verter rules.
    /// Drift requires deliberate user review.
    ClassA,
    /// Existing corpus_representatives — regression baseline (Verter's
    /// current output captured to lock in non-drift, NOT validated
    /// from rules).
    #[allow(dead_code)] // Phase 0a does not author Class B fixtures.
    ClassB,
    /// Pathological recursive-generic fixtures — same regression
    /// baseline treatment as Class B.
    #[allow(dead_code)] // Phase 0a does not author Class C fixtures.
    ClassC,
}

impl FixtureClass {
    /// Snapshot-file suffix per class. Different suffix prevents
    /// collision and signals the regen policy (§0.6.4 stricter for A).
    pub const fn suffix(self) -> &'static str {
        match self {
            FixtureClass::ClassA => "correctness",
            FixtureClass::ClassB | FixtureClass::ClassC => "regression",
        }
    }
}

pub struct CorrectnessFixture {
    pub id: &'static str,
    pub files: &'static [(&'static str, &'static str)],
    pub target: &'static str,
    pub class: FixtureClass,
}

impl CorrectnessFixture {
    pub const fn is_class_a(&self) -> bool {
        matches!(self.class, FixtureClass::ClassA)
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Fixture sources — minimal hermetic SFCs, one resolution rule each.
// ═══════════════════════════════════════════════════════════════════════════

// ── Pick<T,K> — TS spec §4.4 ────────────────────────────────────────────────
const MAPPED_PICK_TWO_KEYS_VUE: &str = r#"<script setup lang="ts">
interface Source {
  alpha: string;
  beta: number;
  gamma: boolean;
  delta: string;
}
defineProps<Pick<Source, 'alpha' | 'beta'>>();
</script>
<template><div /></template>
"#;

// ── Omit<T,K> — TS spec §4.4 ────────────────────────────────────────────────
const MAPPED_OMIT_TWO_KEYS_VUE: &str = r#"<script setup lang="ts">
interface Source {
  alpha: string;
  beta: number;
  gamma: boolean;
  delta: string;
}
defineProps<Omit<Source, 'alpha' | 'beta'>>();
</script>
<template><div /></template>
"#;

// ── Partial<T> — TS spec §4.4 ───────────────────────────────────────────────
const MAPPED_PARTIAL_VUE: &str = r#"<script setup lang="ts">
interface Source {
  a: string;
  b: number;
}
defineProps<Partial<Source>>();
</script>
<template><div /></template>
"#;

// ── Required<T> — TS spec §4.4 ──────────────────────────────────────────────
const MAPPED_REQUIRED_VUE: &str = r#"<script setup lang="ts">
interface Source {
  a?: string;
  b?: number;
}
defineProps<Required<Source>>();
</script>
<template><div /></template>
"#;

// ── Readonly<T> — TS spec §4.4 ──────────────────────────────────────────────
const MAPPED_READONLY_VUE: &str = r#"<script setup lang="ts">
interface Source {
  a: string;
  b: number;
}
defineProps<Readonly<Source>>();
</script>
<template><div /></template>
"#;

// ── Record<K,V> — TS spec §4.4 ──────────────────────────────────────────────
const MAPPED_RECORD_VUE: &str = r#"<script setup lang="ts">
defineProps<Record<'x' | 'y', number>>();
</script>
<template><div /></template>
"#;

// ── Exclude<T,U> — TS spec §4.4 (utility type from lib.es5.d.ts) ────────────
const MAPPED_EXCLUDE_VUE: &str = r#"<script setup lang="ts">
type Filtered = Exclude<'a' | 'b' | 'c', 'b'>;
defineProps<{ kind: Filtered }>();
</script>
<template><div /></template>
"#;

// ── Extract<T,U> — TS spec §4.4 ─────────────────────────────────────────────
const MAPPED_EXTRACT_VUE: &str = r#"<script setup lang="ts">
type Kept = Extract<'a' | 'b' | 'c', 'a' | 'b'>;
defineProps<{ kind: Kept }>();
</script>
<template><div /></template>
"#;

// ── T['variants']['size'] — TS spec §4.5 (indexed access) ───────────────────
const INDEXED_ACCESS_TWO_LEVELS_VUE: &str = r#"<script setup lang="ts">
interface ButtonStyles {
  variants: {
    size: 'sm' | 'md' | 'lg';
    color: 'red' | 'blue';
  };
}
defineProps<{ size: ButtonStyles['variants']['size'] }>();
</script>
<template><div /></template>
"#;

// ── keyof (A & B) — TS spec §4.5 (keyof + intersection) ─────────────────────
const KEYOF_INTERSECTION_VUE: &str = r#"<script setup lang="ts">
interface A { foo: string; bar: number; }
interface B { baz: boolean; }
defineProps<{ key: keyof (A & B) }>();
</script>
<template><div /></template>
"#;

// ── T extends string ? T : never — TS spec §4.6 (distributive cond) ─────────
const CONDITIONAL_DISTRIBUTIVE_VUE: &str = r#"<script setup lang="ts">
type StringsOnly<T> = T extends string ? T : never;
defineProps<{ kind: StringsOnly<'a' | 'b'> }>();
</script>
<template><div /></template>
"#;

// ── { a } & { b } — TS spec §3.10 (intersection of objects) ─────────────────
const INTERSECTION_OF_OBJECTS_VUE: &str = r#"<script setup lang="ts">
defineProps<{ a: string } & { b: number }>();
</script>
<template><div /></template>
"#;

// ── Recursive type alias via typeof — TS spec §3.7 ──────────────────────────
const RECURSIVE_ALIAS_VIA_TYPEOF_VUE: &str = r#"<script setup lang="ts">
interface Tree {
  label: string;
  children?: Tree[];
}
defineProps<{ root: Tree }>();
</script>
<template><div /></template>
"#;

// ── Template literal as key — TS spec §4.5 ──────────────────────────────────
const TEMPLATE_LITERAL_AS_KEY_VUE: &str = r#"<script setup lang="ts">
type PrefixedKeys<K extends string> = { [P in `prefix${K}`]: number };
defineProps<PrefixedKeys<'A' | 'B'>>();
</script>
<template><div /></template>
"#;

// ── Generic substitution via typeof — TS spec §3.6 ──────────────────────────
const GENERIC_SUBSTITUTION_VIA_TYPEOF_VUE: &str = r#"<script setup lang="ts">
const sample = { id: 'a', count: 42 };
type IdShape<T> = { id: T };
defineProps<IdShape<typeof sample.id>>();
</script>
<template><div /></template>
"#;

// ── Userland Pick<T,K>=T shadowing lib — Verter ts-first rule ───────────────
const USERLAND_SHADOWING_PICK_VUE: &str = r#"<script setup lang="ts">
// Userland `Pick<T,K>` here ignores the second parameter and yields
// the entire source `T`. A correct ts-first resolver MUST treat this
// userland declaration as taking precedence over `lib.es5.d.ts`'s
// `Pick`. If the resolver dispatches to the lib definition, the
// snapshot will show only `alpha` and `beta`.
type Pick<T, _K> = T;
interface Source {
  alpha: string;
  beta: number;
  gamma: boolean;
}
defineProps<Pick<Source, 'alpha' | 'beta'>>();
</script>
<template><div /></template>
"#;

// ═══════════════════════════════════════════════════════════════════════════
// Per-fixture file sets.
// ═══════════════════════════════════════════════════════════════════════════
//
// All Phase 0a fixtures are self-contained — no cross-file imports.
// This is deliberate: Phase 0a's scope is mapped-type and structural
// resolution semantics, not import-graph traversal (covered by the
// existing component_meta_audit/external_type test).

const F_MAPPED_PICK_TWO_KEYS: &[(&str, &str)] = &[("/c.vue", MAPPED_PICK_TWO_KEYS_VUE)];
const F_MAPPED_OMIT_TWO_KEYS: &[(&str, &str)] = &[("/c.vue", MAPPED_OMIT_TWO_KEYS_VUE)];
const F_MAPPED_PARTIAL: &[(&str, &str)] = &[("/c.vue", MAPPED_PARTIAL_VUE)];
const F_MAPPED_REQUIRED: &[(&str, &str)] = &[("/c.vue", MAPPED_REQUIRED_VUE)];
const F_MAPPED_READONLY: &[(&str, &str)] = &[("/c.vue", MAPPED_READONLY_VUE)];
const F_MAPPED_RECORD: &[(&str, &str)] = &[("/c.vue", MAPPED_RECORD_VUE)];
const F_MAPPED_EXCLUDE: &[(&str, &str)] = &[("/c.vue", MAPPED_EXCLUDE_VUE)];
const F_MAPPED_EXTRACT: &[(&str, &str)] = &[("/c.vue", MAPPED_EXTRACT_VUE)];
const F_INDEXED_ACCESS_TWO_LEVELS: &[(&str, &str)] = &[("/c.vue", INDEXED_ACCESS_TWO_LEVELS_VUE)];
const F_KEYOF_INTERSECTION: &[(&str, &str)] = &[("/c.vue", KEYOF_INTERSECTION_VUE)];
const F_CONDITIONAL_DISTRIBUTIVE: &[(&str, &str)] = &[("/c.vue", CONDITIONAL_DISTRIBUTIVE_VUE)];
const F_INTERSECTION_OF_OBJECTS: &[(&str, &str)] = &[("/c.vue", INTERSECTION_OF_OBJECTS_VUE)];
const F_RECURSIVE_ALIAS_VIA_TYPEOF: &[(&str, &str)] = &[("/c.vue", RECURSIVE_ALIAS_VIA_TYPEOF_VUE)];
const F_TEMPLATE_LITERAL_AS_KEY: &[(&str, &str)] = &[("/c.vue", TEMPLATE_LITERAL_AS_KEY_VUE)];
const F_GENERIC_SUBSTITUTION_VIA_TYPEOF: &[(&str, &str)] =
    &[("/c.vue", GENERIC_SUBSTITUTION_VIA_TYPEOF_VUE)];
const F_USERLAND_SHADOWING_PICK: &[(&str, &str)] = &[("/c.vue", USERLAND_SHADOWING_PICK_VUE)];

// ═══════════════════════════════════════════════════════════════════════════
// Phase 0a Class A registry.
// ═══════════════════════════════════════════════════════════════════════════
pub const FIXTURES: &[CorrectnessFixture] = &[
    CorrectnessFixture {
        id: "mapped_pick_two_keys",
        files: F_MAPPED_PICK_TWO_KEYS,
        target: "/c.vue",
        class: FixtureClass::ClassA,
    },
    CorrectnessFixture {
        id: "mapped_omit_two_keys",
        files: F_MAPPED_OMIT_TWO_KEYS,
        target: "/c.vue",
        class: FixtureClass::ClassA,
    },
    CorrectnessFixture {
        id: "mapped_partial",
        files: F_MAPPED_PARTIAL,
        target: "/c.vue",
        class: FixtureClass::ClassA,
    },
    CorrectnessFixture {
        id: "mapped_required",
        files: F_MAPPED_REQUIRED,
        target: "/c.vue",
        class: FixtureClass::ClassA,
    },
    CorrectnessFixture {
        id: "mapped_readonly",
        files: F_MAPPED_READONLY,
        target: "/c.vue",
        class: FixtureClass::ClassA,
    },
    CorrectnessFixture {
        id: "mapped_record",
        files: F_MAPPED_RECORD,
        target: "/c.vue",
        class: FixtureClass::ClassA,
    },
    CorrectnessFixture {
        id: "mapped_exclude",
        files: F_MAPPED_EXCLUDE,
        target: "/c.vue",
        class: FixtureClass::ClassA,
    },
    CorrectnessFixture {
        id: "mapped_extract",
        files: F_MAPPED_EXTRACT,
        target: "/c.vue",
        class: FixtureClass::ClassA,
    },
    CorrectnessFixture {
        id: "indexed_access_two_levels",
        files: F_INDEXED_ACCESS_TWO_LEVELS,
        target: "/c.vue",
        class: FixtureClass::ClassA,
    },
    CorrectnessFixture {
        id: "keyof_intersection",
        files: F_KEYOF_INTERSECTION,
        target: "/c.vue",
        class: FixtureClass::ClassA,
    },
    CorrectnessFixture {
        id: "conditional_distributive",
        files: F_CONDITIONAL_DISTRIBUTIVE,
        target: "/c.vue",
        class: FixtureClass::ClassA,
    },
    CorrectnessFixture {
        id: "intersection_of_objects",
        files: F_INTERSECTION_OF_OBJECTS,
        target: "/c.vue",
        class: FixtureClass::ClassA,
    },
    CorrectnessFixture {
        id: "recursive_alias_via_typeof",
        files: F_RECURSIVE_ALIAS_VIA_TYPEOF,
        target: "/c.vue",
        class: FixtureClass::ClassA,
    },
    CorrectnessFixture {
        id: "template_literal_as_key",
        files: F_TEMPLATE_LITERAL_AS_KEY,
        target: "/c.vue",
        class: FixtureClass::ClassA,
    },
    CorrectnessFixture {
        id: "generic_substitution_via_typeof",
        files: F_GENERIC_SUBSTITUTION_VIA_TYPEOF,
        target: "/c.vue",
        class: FixtureClass::ClassA,
    },
    CorrectnessFixture {
        id: "userland_shadowing_pick",
        files: F_USERLAND_SHADOWING_PICK,
        target: "/c.vue",
        class: FixtureClass::ClassA,
    },
];
