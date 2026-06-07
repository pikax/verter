//! Fixture registry. Each entry maps to one synthetic Vue
//! project and a snapshot file. `class` discriminates the
//! correctness ground-truth tier (Class A) from the regression
//! baselines (Class B + C).
//!
//! The Class A fixtures (mapped-type + structural) carry hand-derived
//! rule-correct expected outputs. The remaining utility-type fixtures
//! (`mapped_exclude`, `mapped_extract`, `template_literal_as_key`,
//! `generic_substitution_via_typeof`, `userland_shadowing_pick`) are
//! authored with rule-correct expected once the resolver variants
//! close the gaps.
//!
//! The component-meta property fixtures are Class A; the Class B + C
//! regression baselines (corpus_representatives + pathologicals)
//! capture Verter's current output via `UPDATE_SNAPSHOTS=1` — they are
//! regression baselines only, not rule-derived.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FixtureClass {
    /// Hand-derived expected output from TS spec / Verter rules.
    /// Drift requires deliberate user review.
    ClassA,
    /// Existing corpus_representatives — regression baseline (Verter's
    /// current output captured to lock in non-drift, NOT validated
    /// from rules).
    ClassB,
    /// Pathological recursive-generic fixtures — same regression
    /// baseline treatment as Class B.
    ClassC,
}

impl FixtureClass {
    /// Snapshot-file suffix per class. Different suffix prevents
    /// collision and signals the regen policy (stricter for A).
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

// ── Userland Pick<T,_K> = T shadowing lib Pick — Verter ts-first rule ───────
//   The userland alias `Pick<T,_K> = T` returns the entire `T` (ignoring
//   `K`). With this alias in scope, `defineProps<Pick<Cfg, 'alpha'>>()`
//   resolves to `Cfg` itself — surfacing all three members
//   (`alpha`, `beta`, `gamma`) — NOT the lib's mapped-Pick output of
//   only `alpha`. Verter rule `./.claude/skills/type-resolution`
//   ("user shadowing wins"). Handled by the resolver-context
//   `ScopeShadowing` struct.
const USERLAND_SHADOWING_PICK_VUE: &str = r#"<script setup lang="ts">
type Pick<T, _K> = T;
interface Cfg {
  alpha: string;
  beta: number;
  gamma: boolean;
}
defineProps<Pick<Cfg, 'alpha'>>();
</script>
<template><div /></template>
"#;

// ── Exclude<T,U> over a literal union — TS spec §4.4 ────────────────────────
//   `Exclude<T,U> = T extends U ? never : T` distributes per-member
//   over T and drops every member matching U. For
//   `Exclude<'a' | 'b' | 'c', 'b'>`, only `'a'` and `'c'` survive.
//   Handled via per-member relation engine dispatch in
//   `build_builtin_utility`'s `Extract` / `Exclude` arms.
const MAPPED_EXCLUDE_VUE: &str = r#"<script setup lang="ts">
defineProps<{ kind: Exclude<'a' | 'b' | 'c', 'b'> }>();
</script>
<template><div /></template>
"#;

// ── Extract<T,U> over a literal union — TS spec §4.4 ────────────────────────
//   `Extract<T,U> = T extends U ? T : never` distributes per-member
//   over T and keeps every member matching U. For
//   `Extract<'a' | 'b' | 'c', 'a' | 'b'>`, only `'a'` and `'b'`
//   survive. Handled via the same arm that handles `Exclude`
//   (sister utility).
const MAPPED_EXTRACT_VUE: &str = r#"<script setup lang="ts">
defineProps<{ kind: Extract<'a' | 'b' | 'c', 'a' | 'b'> }>();
</script>
<template><div /></template>
"#;

// ── Generic substitution via typeof on a value-member path — TS spec §3.6 ───
//   `IdShape<T>` is a userland generic interface whose body references
//   `T`. The defineProps argument is `IdShape<typeof sample.id>` —
//   the type argument is a `typeof` expression whose path projects
//   the `id` member of the value `sample`. With `sample` typed as
//   `Sample { id: string }`, `typeof sample.id` evaluates to `string`.
//   Substitution into the body `{ id: T }` yields `{ id: string }`,
//   surfacing one required prop `id: string`.
//
//   The lowering attempts single-segment root resolution first
//   (`sample`), succeeds, projects the remaining `["id"]` path
//   through `ProjectPath { mode: Navigate }` to `string`, then
//   substitutes `T → string` in the instantiation body. The
//   materialised surface produces one required prop `id: string`,
//   matching `phase-00-tier1-mismatches.md` row 4.
//
//   Rule citation: TS spec §3.6 (generic substitution); CLAUDE.md
//   "generic substitutions are part of semantic meaning". The fixture
//   is a regression guard.
const GENERIC_SUBSTITUTION_VIA_TYPEOF_VUE: &str = r#"<script setup lang="ts">
interface Sample { id: string }
const sample: Sample = { id: "abc" };
interface IdShape<T> { id: T; }
defineProps<IdShape<typeof sample.id>>();
</script>
<template><div /></template>
"#;

// ── Template-literal mapped type key — TS spec §4.5 ─────────────────────────
//   `{ [K in 'A' | 'B' as `prefix${K}`]: number }` iterates
//   K = 'A' | 'B' and uses the `as <template>` clause to interpolate
//   K into a template literal, producing keys `prefixA` and
//   `prefixB`. The mapped value is always `number`. Handled by
//   applying `mapper.name_remap` during member iteration in
//   `build_mapped_type` and folding `TemplateLiteral` nodes into a
//   `Literal::String` when every expression resolves to a literal.
const TEMPLATE_LITERAL_AS_KEY_VUE: &str = r#"<script setup lang="ts">
type R = { [K in 'A' | 'B' as `prefix${K}`]: number };
defineProps<R>();
</script>
<template><div /></template>
"#;

// ═══════════════════════════════════════════════════════════════════════════
// Class A property fixtures (component-meta macros).
// ═══════════════════════════════════════════════════════════════════════════
//
// Each fixture exercises one component-meta macro surface so the
// discriminating self-test can target one MutationKind per
// row. Sources are minimal and hermetic — no cross-file imports —
// and the rule citation is in the companion derivation note.

// ── defineProps + withDefaults — Verter macros §props ───────────────────────
//   `name: string` is required (no default); `count?: number` becomes
//   non-required with `has_default = true` and `default_value = "0"`
//   when `withDefaults` provides `{ count: 0 }`.
const FIXTURE_PROPS_WITH_DEFAULTS_VUE: &str = r#"<script setup lang="ts">
withDefaults(defineProps<{ name: string; count?: number }>(), { count: 0 });
</script>
<template><div /></template>
"#;

// ── defineEmits<T> — Verter macros §emits ───────────────────────────────────
//   One event `click` with parameter list `[evt: string]`. Using a
//   primitive parameter type avoids cross-file `Event` imports.
const FIXTURE_EVENTS_TYPED_VUE: &str = r#"<script setup lang="ts">
defineEmits<{ click: [evt: string] }>();
</script>
<template><div /></template>
"#;

// ── defineSlots<T> typed bindings — Verter macros §slots ────────────────────
//   Two slots, each with a single typed binding on the slot
//   function's first parameter Object literal. The binding types
//   are primitives so no cross-file imports are needed.
//   `phase-00b-tier1-mismatches.md` row 1.
const FIXTURE_SLOTS_TYPED_VUE: &str = r#"<script setup lang="ts">
defineSlots<{
  default(props: { item: string }): any;
  named(props: { row: number }): any;
}>();
</script>
<template><div /></template>
"#;

// ── defineModel<T>() typed model — Verter macros §model ─────────────────────
//   Two model calls: `defineModel<string>()` (defaults to
//   `modelValue`) and `defineModel<number>('count')`. Both
//   optional, no defaults, surfacing as model + prop +
//   update:<name> event triples per Vue's documented contract.
//   `phase-00b-tier1-mismatches.md` row 2.
const FIXTURE_MODELS_VUE: &str = r#"<script setup lang="ts">
defineModel<string>();
defineModel<number>('count');
</script>
<template><div /></template>
"#;

// ── defineExpose — Verter macros §expose ────────────────────────────────────
//   Vue's documented public API uses the value
//   form `defineExpose({ ... })`; type-only `defineExpose<T>()` is
//   not part of the documented Vue 3 surface. The discriminating
//   self-test only checks `ExposedDropped` (the rule "every key of T
//   surfaces as exposed"), which is form-agnostic. Each exposed
//   binding declares its function type explicitly so the binding's
//   `type_annotation` is non-empty (otherwise the resolver returns
//   `Unknown` and the snapshot signature drifts).
const FIXTURE_EXPOSED_METHODS_VUE: &str = r#"<script setup lang="ts">
const focus: () => void = () => {};
const reset: () => void = () => {};
defineExpose({ focus, reset });
</script>
<template><div /></template>
"#;

// ── inheritAttrs: false — CLAUDE.md §Fallthrough ────────────────────────────
//   `defineOptions({ inheritAttrs: false })` zeros out the
//   fallthrough surface. The single declared prop is preserved on
//   `props`; the `fallthrough` projection becomes
//   `Some(FallthroughView { inherit_attrs: false, ... })` with an
//   empty surface signature.
const FIXTURE_FALLTHROUGH_INHERIT_VUE: &str = r#"<script setup lang="ts">
defineOptions({ inheritAttrs: false });
defineProps<{ disabled?: boolean }>();
</script>
<template><button /></template>
"#;

// ── single component root inheriting child surface — CLAUDE.md §Fallthrough ─
//   The wrapper `<Inner />` is a single component root: its declared
//   `label` prop propagates as the inherited fallthrough surface on
//   the wrapper. Inner declares `inheritAttrs: false` so its own
//   accepted surface is exactly `{ label }` (no native:div
//   intrinsics chained in). The wrapper therefore inherits exactly
//   `{ label: string /* from component:/inner.vue */ }` — a
//   hand-authorable signature with one component-sourced entry.
const FIXTURE_FALLTHROUGH_ROOT_INHERIT_WRAPPER_VUE: &str = r#"<script setup lang="ts">
import Inner from './inner.vue';
</script>
<template><Inner /></template>
"#;
const FIXTURE_FALLTHROUGH_ROOT_INHERIT_INNER_VUE: &str = r#"<script setup lang="ts">
defineOptions({ inheritAttrs: false });
defineProps<{ label: string }>();
</script>
<template><div /></template>
"#;

// ═══════════════════════════════════════════════════════════════════════════
// Class B regression sources (corpus_representatives).
// ═══════════════════════════════════════════════════════════════════════════
//
// Each source is the same SFC + cross-file types pair used by the
// `component_meta_audit/corpus_representatives/*.rs` tests. The Class
// B baseline captures Verter's current output as the regression
// reference; rule-derivation is deliberately out of scope.

const ACCORDION_VUE: &str = r#"<script setup lang="ts">
import type { AccordionItem } from './accordion_types';
defineProps<{ items: AccordionItem[]; multiple?: boolean }>();
</script>
<template>
  <div class="accordion">
    <details v-for="(item, i) in items" :key="i" :open="!multiple">
      <summary>{{ item.label }}</summary>
      <div>{{ item.content }}</div>
    </details>
  </div>
</template>
"#;
const ACCORDION_TYPES_TS: &str = r#"export interface AccordionItem {
  label: string;
  content: string;
  disabled?: boolean;
}
"#;

const ALERT_VUE: &str = r#"<script setup lang="ts">
import type { AlertVariant } from './alert_types';
defineProps<{ variant?: AlertVariant; title: string; dismissible?: boolean }>();
defineEmits<{ dismiss: [] }>();
</script>
<template>
  <div class="alert" :class="variant">
    <h3>{{ title }}</h3>
    <slot />
    <button v-if="dismissible" @click="$emit('dismiss')">&times;</button>
  </div>
</template>
"#;
const ALERT_TYPES_TS: &str = r#"export type AlertVariant = 'info' | 'success' | 'warning' | 'error';
"#;

const APP_VUE: &str = r#"<script setup lang="ts">
defineProps<{ title: string; mode?: 'light' | 'dark' }>();
</script>
<template>
  <div class="app" :class="mode">
    <header>{{ title }}</header>
    <main><slot /></main>
  </div>
</template>
"#;

const AUTH_FORM_VUE: &str = r#"<script setup lang="ts">
import type { AuthFormField, AuthFormSubmit } from './auth_form_types';
defineProps<{ fields: AuthFormField[]; submit: AuthFormSubmit }>();
defineEmits<{ submit: [value: Record<string, string>] }>();
</script>
<template>
  <form @submit.prevent="$emit('submit', {})">
    <div v-for="f in fields" :key="f.name">
      <label>{{ f.label }}</label>
      <input :type="f.type" :name="f.name" :required="f.required" />
    </div>
    <button type="submit">{{ submit.label }}</button>
  </form>
</template>
"#;
const AUTH_FORM_TYPES_TS: &str = r#"export interface AuthFormField {
  name: string;
  label: string;
  type: 'text' | 'email' | 'password';
  required?: boolean;
}
export interface AuthFormSubmit {
  label: string;
  loading?: boolean;
}
"#;

const AVATAR_VUE: &str = r#"<script setup lang="ts">
import type { AvatarSize, AvatarShape } from './avatar_types';
defineProps<{ src?: string; alt?: string; size?: AvatarSize; shape?: AvatarShape }>();
</script>
<template>
  <span class="avatar" :class="[size, shape]">
    <img v-if="src" :src="src" :alt="alt" />
    <slot v-else />
  </span>
</template>
"#;
const AVATAR_TYPES_TS: &str = r#"export type AvatarSize = 'xs' | 'sm' | 'md' | 'lg' | 'xl';
export type AvatarShape = 'circle' | 'square' | 'rounded';
"#;

const AVATAR_GROUP_VUE: &str = r#"<script setup lang="ts">
import type { AvatarSize } from './avatar_types';
import type { AvatarGroupItem } from './avatar_group_types';
defineProps<{ items: AvatarGroupItem[]; size?: AvatarSize; max?: number }>();
</script>
<template>
  <div class="avatar-group" :class="size">
    <span v-for="(item, i) in items.slice(0, max)" :key="i">
      <img :src="item.src" :alt="item.alt" />
    </span>
  </div>
</template>
"#;
const AVATAR_GROUP_TYPES_TS: &str = r#"export interface AvatarGroupItem {
  src: string;
  alt?: string;
}
"#;
const AVATAR_GROUP_AVATAR_TYPES_TS: &str = r#"export type AvatarSize = 'xs' | 'sm' | 'md' | 'lg' | 'xl';
"#;

// ═══════════════════════════════════════════════════════════════════════════
// Class C regression sources (pathological fixtures).
// ═══════════════════════════════════════════════════════════════════════════

const PATH_TABLE_VUE: &str = include_str!("../../test_fixtures/table.vue");
const PATH_TABLE_TYPES_TS: &str = include_str!("../../test_fixtures/table_types.ts");

const PATH_EDITOR_TOOLBAR_VUE: &str = include_str!("../../test_fixtures/editor_toolbar.vue");
const PATH_EDITOR_TOOLBAR_TYPES_TS: &str =
    include_str!("../../test_fixtures/editor_toolbar_types.ts");

const PATH_TABS_VUE: &str = include_str!("../../test_fixtures/tabs.vue");
const PATH_TABS_TYPES_TS: &str = include_str!("../../test_fixtures/tabs_types.ts");
const PATH_TABS_HELPER_TS: &str = include_str!("../../test_fixtures/tabs_helper.ts");

// ═══════════════════════════════════════════════════════════════════════════
// Per-fixture file sets.
// ═══════════════════════════════════════════════════════════════════════════
//
// The mapped-type and structural fixtures are self-contained — no
// cross-file imports. This is deliberate: their scope is mapped-type
// and structural resolution semantics, not import-graph traversal
// (covered by the existing component_meta_audit/external_type test).

const F_MAPPED_PICK_TWO_KEYS: &[(&str, &str)] = &[("/c.vue", MAPPED_PICK_TWO_KEYS_VUE)];
const F_MAPPED_OMIT_TWO_KEYS: &[(&str, &str)] = &[("/c.vue", MAPPED_OMIT_TWO_KEYS_VUE)];
const F_MAPPED_PARTIAL: &[(&str, &str)] = &[("/c.vue", MAPPED_PARTIAL_VUE)];
const F_MAPPED_REQUIRED: &[(&str, &str)] = &[("/c.vue", MAPPED_REQUIRED_VUE)];
const F_MAPPED_READONLY: &[(&str, &str)] = &[("/c.vue", MAPPED_READONLY_VUE)];
const F_MAPPED_RECORD: &[(&str, &str)] = &[("/c.vue", MAPPED_RECORD_VUE)];
const F_INDEXED_ACCESS_TWO_LEVELS: &[(&str, &str)] = &[("/c.vue", INDEXED_ACCESS_TWO_LEVELS_VUE)];
const F_KEYOF_INTERSECTION: &[(&str, &str)] = &[("/c.vue", KEYOF_INTERSECTION_VUE)];
const F_CONDITIONAL_DISTRIBUTIVE: &[(&str, &str)] = &[("/c.vue", CONDITIONAL_DISTRIBUTIVE_VUE)];
const F_INTERSECTION_OF_OBJECTS: &[(&str, &str)] = &[("/c.vue", INTERSECTION_OF_OBJECTS_VUE)];
const F_RECURSIVE_ALIAS_VIA_TYPEOF: &[(&str, &str)] = &[("/c.vue", RECURSIVE_ALIAS_VIA_TYPEOF_VUE)];
const F_USERLAND_SHADOWING_PICK: &[(&str, &str)] = &[("/c.vue", USERLAND_SHADOWING_PICK_VUE)];
const F_MAPPED_EXCLUDE: &[(&str, &str)] = &[("/c.vue", MAPPED_EXCLUDE_VUE)];
const F_MAPPED_EXTRACT: &[(&str, &str)] = &[("/c.vue", MAPPED_EXTRACT_VUE)];
const F_TEMPLATE_LITERAL_AS_KEY: &[(&str, &str)] = &[("/c.vue", TEMPLATE_LITERAL_AS_KEY_VUE)];
const F_GENERIC_SUBSTITUTION_VIA_TYPEOF: &[(&str, &str)] =
    &[("/c.vue", GENERIC_SUBSTITUTION_VIA_TYPEOF_VUE)];

// ── Class A property fixture file sets ─────────────────────────────
const F_FIXTURE_PROPS_WITH_DEFAULTS: &[(&str, &str)] =
    &[("/c.vue", FIXTURE_PROPS_WITH_DEFAULTS_VUE)];
const F_FIXTURE_EVENTS_TYPED: &[(&str, &str)] = &[("/c.vue", FIXTURE_EVENTS_TYPED_VUE)];
const F_FIXTURE_SLOTS_TYPED: &[(&str, &str)] = &[("/c.vue", FIXTURE_SLOTS_TYPED_VUE)];
const F_FIXTURE_MODELS: &[(&str, &str)] = &[("/c.vue", FIXTURE_MODELS_VUE)];
const F_FIXTURE_EXPOSED_METHODS: &[(&str, &str)] = &[("/c.vue", FIXTURE_EXPOSED_METHODS_VUE)];
const F_FIXTURE_FALLTHROUGH_INHERIT: &[(&str, &str)] =
    &[("/c.vue", FIXTURE_FALLTHROUGH_INHERIT_VUE)];
const F_FIXTURE_FALLTHROUGH_ROOT_INHERIT: &[(&str, &str)] = &[
    ("/c.vue", FIXTURE_FALLTHROUGH_ROOT_INHERIT_WRAPPER_VUE),
    ("/inner.vue", FIXTURE_FALLTHROUGH_ROOT_INHERIT_INNER_VUE),
];

// ── Class B regression file sets ───────────────────────────────────
const F_ACCORDION: &[(&str, &str)] = &[
    ("/accordion.vue", ACCORDION_VUE),
    ("/accordion_types.ts", ACCORDION_TYPES_TS),
];
const F_ALERT: &[(&str, &str)] = &[
    ("/alert.vue", ALERT_VUE),
    ("/alert_types.ts", ALERT_TYPES_TS),
];
const F_APP: &[(&str, &str)] = &[("/app.vue", APP_VUE)];
const F_AUTH_FORM: &[(&str, &str)] = &[
    ("/auth_form.vue", AUTH_FORM_VUE),
    ("/auth_form_types.ts", AUTH_FORM_TYPES_TS),
];
const F_AVATAR: &[(&str, &str)] = &[
    ("/avatar.vue", AVATAR_VUE),
    ("/avatar_types.ts", AVATAR_TYPES_TS),
];
const F_AVATAR_GROUP: &[(&str, &str)] = &[
    ("/avatar_group.vue", AVATAR_GROUP_VUE),
    ("/avatar_group_types.ts", AVATAR_GROUP_TYPES_TS),
    ("/avatar_types.ts", AVATAR_GROUP_AVATAR_TYPES_TS),
];

// ── Class C regression file sets ───────────────────────────────────
const F_PATH_TABLE_LOADING: &[(&str, &str)] = &[
    ("/table.vue", PATH_TABLE_VUE),
    ("/table_types.ts", PATH_TABLE_TYPES_TS),
];
const F_PATH_EDITOR_TOOLBAR: &[(&str, &str)] = &[
    ("/editor_toolbar.vue", PATH_EDITOR_TOOLBAR_VUE),
    ("/editor_toolbar_types.ts", PATH_EDITOR_TOOLBAR_TYPES_TS),
];
const F_PATH_TABS_DYNAMIC_HELPER: &[(&str, &str)] = &[
    ("/tabs.vue", PATH_TABS_VUE),
    ("/tabs_types.ts", PATH_TABS_TYPES_TS),
    ("/tabs_helper.ts", PATH_TABS_HELPER_TS),
];

// ═══════════════════════════════════════════════════════════════════════════
// Fixture registry.
// ═══════════════════════════════════════════════════════════════════════════
//
// Iteration-order discipline: Class B + C fixtures are listed
// FIRST so that under `UPDATE_SNAPSHOTS=1` the regression baselines
// are captured before the harness's by-design panic on Class A
// (see `correctness_snapshot_for_every_fixture` in
// `tests/correctness.rs`). The normal run iterates the
// same list and validates every entry against its committed
// snapshot — Class A against `expected.rs` via
// `ensure_class_a_expected_matches_snapshot`, Class B+C against the
// captured `<id>.regression.snap.json`. Test order is independent
// of correctness.
pub const FIXTURES: &[CorrectnessFixture] = &[
    // ── Class B — corpus_representatives regression baselines ──────
    CorrectnessFixture {
        id: "accordion",
        files: F_ACCORDION,
        target: "/accordion.vue",
        class: FixtureClass::ClassB,
    },
    CorrectnessFixture {
        id: "alert",
        files: F_ALERT,
        target: "/alert.vue",
        class: FixtureClass::ClassB,
    },
    CorrectnessFixture {
        id: "app",
        files: F_APP,
        target: "/app.vue",
        class: FixtureClass::ClassB,
    },
    CorrectnessFixture {
        id: "auth_form",
        files: F_AUTH_FORM,
        target: "/auth_form.vue",
        class: FixtureClass::ClassB,
    },
    CorrectnessFixture {
        id: "avatar",
        files: F_AVATAR,
        target: "/avatar.vue",
        class: FixtureClass::ClassB,
    },
    CorrectnessFixture {
        id: "avatar_group",
        files: F_AVATAR_GROUP,
        target: "/avatar_group.vue",
        class: FixtureClass::ClassB,
    },
    // ── Class C — pathological regression baselines ────────────────
    CorrectnessFixture {
        id: "pathological_table_loading_animation",
        files: F_PATH_TABLE_LOADING,
        target: "/table.vue",
        class: FixtureClass::ClassC,
    },
    CorrectnessFixture {
        id: "pathological_editor_toolbar_array_or_nested",
        files: F_PATH_EDITOR_TOOLBAR,
        target: "/editor_toolbar.vue",
        class: FixtureClass::ClassC,
    },
    CorrectnessFixture {
        id: "pathological_tabs_dynamic_helper",
        files: F_PATH_TABS_DYNAMIC_HELPER,
        target: "/tabs.vue",
        class: FixtureClass::ClassC,
    },
    // ── Class A — mapped types + structural ────────────────────────
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
    // Class A fixture for the userland-shadow-pick case, handled by
    // the resolver-context `ScopeShadowing` thread (recorded in
    // `phase-00-tier1-mismatches.md` row 5).
    CorrectnessFixture {
        id: "userland_shadowing_pick",
        files: F_USERLAND_SHADOWING_PICK,
        target: "/c.vue",
        class: FixtureClass::ClassA,
    },
    // Class A fixtures for the `Exclude` / `Extract` literal-type
    // reduction (rows 1, 2) and the mapper `name_remap` +
    // `TemplateLiteral` fold (row 3), recorded in
    // `phase-00-tier1-mismatches.md` rows 1-3.
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
        id: "template_literal_as_key",
        files: F_TEMPLATE_LITERAL_AS_KEY,
        target: "/c.vue",
        class: FixtureClass::ClassA,
    },
    // Class A fixture for the value-member typeof case, handled by the
    // single-segment-first lookup in `shallow_lower_type_expr`'s
    // `TypeExpr::TypeOf` arm (recorded in
    // `phase-00-tier1-mismatches.md` row 4).
    CorrectnessFixture {
        id: "generic_substitution_via_typeof",
        files: F_GENERIC_SUBSTITUTION_VIA_TYPEOF,
        target: "/c.vue",
        class: FixtureClass::ClassA,
    },
    // ── Class A — component-meta property macros ───────────────────
    //
    // The `fixture_slots_typed` (slot binding type literals) and
    // `fixture_models` (`defineModel<T>()` type T through the macro
    // path) cases are documented in `phase-00b-tier1-mismatches.md`
    // with rule citations and the diff.
    CorrectnessFixture {
        id: "fixture_props_with_defaults",
        files: F_FIXTURE_PROPS_WITH_DEFAULTS,
        target: "/c.vue",
        class: FixtureClass::ClassA,
    },
    CorrectnessFixture {
        id: "fixture_events_typed",
        files: F_FIXTURE_EVENTS_TYPED,
        target: "/c.vue",
        class: FixtureClass::ClassA,
    },
    // ── slot-binding + defineModel fixtures ──────────────
    //
    // Documented in `phase-00b-tier1-mismatches.md` rows 1-2:
    // - `fixture_slots_typed` via `project_slot_binding_member`.
    // - `fixture_models` via the `expand_field_expr` `DefineModel`
    //   branch.
    CorrectnessFixture {
        id: "fixture_slots_typed",
        files: F_FIXTURE_SLOTS_TYPED,
        target: "/c.vue",
        class: FixtureClass::ClassA,
    },
    CorrectnessFixture {
        id: "fixture_models",
        files: F_FIXTURE_MODELS,
        target: "/c.vue",
        class: FixtureClass::ClassA,
    },
    CorrectnessFixture {
        id: "fixture_exposed_methods",
        files: F_FIXTURE_EXPOSED_METHODS,
        target: "/c.vue",
        class: FixtureClass::ClassA,
    },
    CorrectnessFixture {
        id: "fixture_fallthrough_inherit",
        files: F_FIXTURE_FALLTHROUGH_INHERIT,
        target: "/c.vue",
        class: FixtureClass::ClassA,
    },
    CorrectnessFixture {
        id: "fixture_fallthrough_root_inherit",
        files: F_FIXTURE_FALLTHROUGH_ROOT_INHERIT,
        target: "/c.vue",
        class: FixtureClass::ClassA,
    },
];
