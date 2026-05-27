//! Cross-file regression tests for the
//! `prepared_surface.rs::TypeExpr::Intersection` non-fatal-unsupported
//! merge, plus the analyzer-side `declared_in_macro_type_arg`
//! provenance contract.
//!
//! ## Intersection-merge invariant under test
//!
//!   A `TypeExpr::Intersection([A, B])` lowered surface MUST publish
//!   the members contributed by any resolvable arm even when sibling
//!   arms return `PreparedSurfaceProjection::Unsupported`. Only when
//!   EVERY arm is `Unsupported` does the intersection collapse. This
//!   matches TypeScript's intersection semantics — an unresolvable
//!   `B` does not poison `A`'s contributions.
//!
//! ## Test scope
//!
//! Three layers of coverage:
//!
//! 1. **Body-member survives unresolvable heritage** — positive
//!    regression characterizations: the non-fatal-unsupported merge
//!    lets an explicit body member survive an unresolvable heritage
//!    arm. Realistic corpus shapes (`AuthForm.vue` / `Form.vue` /
//!    `Table.vue` from the nuxt-ui bench corpus) drive the cases.
//!
//! 2. **`intersection_merge_tests::merge_prepared_intersection_arms_*`**
//!    in `resolver_core/component_meta_query_engine/prepared_surface.rs`
//!    — discriminating tests that exercise the pure
//!    `merge_prepared_intersection_arms` helper directly with
//!    synthesised `PreparedSurfaceProjection` inputs, bypassing the
//!    component-meta pipeline's rescue paths entirely. Reverting the
//!    helper's `// Skip` arm on `Unsupported` to a hard
//!    `return PreparedSurfaceProjection::Unsupported;` makes the
//!    `..._skips_unsupported_arm_when_sibling_resolves` and
//!    `..._treats_empty_arm_as_resolved` tests fail with precise
//!    diffs.
//!
//! 3. **Analyzer-side `declared_in_macro_type_arg` contract** —
//!    `inline_literal_..._for_on_event_shadow_of_declared_emit`
//!    and `local_interface_own_body_marked_declared_while_heritage_arm_unresolvable`.
//!    Inline-literal `defineProps<{ onSubmit?: ... }>()` + a
//!    declared `submit` emit must yield an `AnalyzedPropField`
//!    whose `declared_in_macro_type_arg` is `true` (and
//!    `PropAnalysis` / `FfiPropMeta` / `PropMeta` likewise). These
//!    tests discriminate at the analyzer boundary — removing the
//!    body-extractor `declared = true` populations makes them
//!    fail.
//!
//! ## Cross-file imported-interface provenance scope
//!
//! Cross-file `defineProps<ImportedProps>()` provenance flow
//! requires threading the `declared_in_macro_type_arg` bit through
//! five types (`ResolvedProp`, `ProjectedMember`, `SurfaceMember`,
//! `ExpandedField`, `ExpandedProperty`) across three crates, plus
//! prepared-surface walker heritage tracking, plus FFI / proto
//! re-wire, plus ~30 fixture constructor updates. This is a
//! dedicated architectural cycle not exercised by the current
//! bench corpus (0/179 components hit the cross-file
//! imported-macro-root shape). Membership is still covered today
//! via the intersection-merge fix (the discriminating helper
//! tests above); override semantics for the cross-file case is
//! tracked as future work.

use super::*;
use crate::types::HostConfig;
use crate::VerterHost;
use std::sync::Arc;

fn test_scheduler_config() -> verter_scheduler::scheduler::SchedulerConfig {
    verter_scheduler::scheduler::SchedulerConfig {
        cpu_threads: 1,
        ..verter_scheduler::scheduler::SchedulerConfig::default()
    }
}

fn make_project() -> Arc<MetaProject> {
    let host = VerterHost::new_standalone_with_scheduler_config(
        HostConfig {
            analysis_level: crate::types::AnalysisLevel::Full,
            ..HostConfig::default()
        },
        test_scheduler_config(),
    );
    MetaProject::new(host)
}

fn get_meta(
    project: &Arc<MetaProject>,
    canonical_id: &str,
) -> verter_semantic::analysis::component_meta::ComponentMetaAnalysis {
    let session = project.open_session_batch().unwrap();
    session
        .get_component_meta(canonical_id)
        .unwrap()
        .expect("get_component_meta should return metadata")
}

/// Phase C Class A — positive regression characterization for the
/// non-fatal-unsupported intersection merge: heritage chain to an
/// unresolvable external type + explicit body members.
///
/// `Intersection([Ref(Unresolvable), Object{ onSubmit, title }])`
/// publishes `onSubmit` and `title` from the Object arm even though
/// the Ref arm cannot be resolved. Mirrors the structural shape of
/// nuxt-ui `AuthForm.vue` / `Form.vue` (whose corpus failure
/// exposed the bug).
///
/// See the module docstring for the discrimination caveat: the
/// truly discriminating proof of the Phase C fix is the
/// 177-component bench gate, since synthetic single-component
/// fixtures often hit the analyzer-side rescue path before the
/// prepared-surface intersection runs.
#[test]
fn intersection_keeps_class_a_explicit_body_member_when_heritage_arm_is_unresolvable() {
    let project = make_project();

    project
        .upsert_base(
            "/node_modules/external-pkg/index.d.ts",
            r#"// Intentionally empty: the SFC imports `MissingFormAttrs`
// from here but the package does not export the symbol, so the
// heritage arm is structurally unresolvable.
"#,
        )
        .unwrap();

    project
        .upsert_base(
            "/types/external.ts",
            r#"import type { MissingFormAttrs } from 'external-pkg'

// Generic Ref carrier — instantiated at the defineProps site so
// the prepared-surface projector sees an `Intersection` of a Ref
// to an unresolvable heritage type and an explicit Object body.
export interface FormPropsCarrier<T = unknown> extends MissingFormAttrs {
    onSubmit?: (event: T) => void
    title?: string
}
"#,
        )
        .unwrap();

    project
        .upsert_base(
            "/AuthForm.vue",
            r#"<script setup lang="ts">
import type { FormPropsCarrier } from './types/external'

defineProps<FormPropsCarrier<Event>>()
defineEmits<{ submit: [event: Event] }>()
</script>
<template><form><slot /></form></template>"#,
        )
        .unwrap();

    project.host().set_import_dependencies(
        "/types/external.ts",
        vec![crate::types::DependencyResolution {
            specifier: "external-pkg".to_string(),
            resolved_canonical_id: Some("/node_modules/external-pkg/index.d.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );
    project.host().set_import_dependencies(
        "/AuthForm.vue",
        vec![crate::types::DependencyResolution {
            specifier: "./types/external".to_string(),
            resolved_canonical_id: Some("/types/external.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );

    let meta = get_meta(&project, "/AuthForm.vue");
    let prop_names: Vec<&str> = meta.props.iter().map(|prop| prop.name.as_str()).collect();

    assert!(
        prop_names.contains(&"onSubmit"),
        "Phase C Class A: explicit body member `onSubmit` MUST be \
         published when the heritage arm is unresolvable. \
         Got props: {prop_names:?}"
    );
    assert!(
        prop_names.contains(&"title"),
        "Phase C Class A: sibling body member `title` MUST also \
         be published — proves the merge keeps the Object arm's \
         full surface. Got props: {prop_names:?}"
    );
}

/// Phase C Class B — positive regression characterization for the
/// `Omit<Unresolvable, K>` heritage + explicit body re-introduction
/// of the omitted keys. Mirrors nuxt-ui `Table.vue`'s
/// `TableOptions` shape.
///
/// See the module docstring for the discrimination caveat.
#[test]
fn intersection_keeps_class_b_explicit_body_members_when_omit_argument_is_unresolvable() {
    let project = make_project();

    project
        .upsert_base(
            "/node_modules/external-table/index.d.ts",
            r#"// Intentionally empty: the SFC's heritage clause references
// `CoreOptions` from here but the package does not export it.
"#,
        )
        .unwrap();

    project
        .upsert_base(
            "/types/table.ts",
            r#"import type { CoreOptions } from 'external-table'

// Generic Ref carrier — instantiated at the defineProps site so
// the prepared-surface projector walks the instantiated TypeExpr
// through the `Intersection` branch with the Omit arm structurally
// Unsupported.
export interface TablePropsCarrier<T = unknown>
    extends Omit<CoreOptions<T>, 'state' | 'onStateChange' | 'renderFallbackValue'> {
    state?: { rows: number; data: T }
    onStateChange?: (next: { rows: number; data: T }) => void
    renderFallbackValue?: string
}
"#,
        )
        .unwrap();

    project
        .upsert_base(
            "/Table.vue",
            r#"<script setup lang="ts">
import type { TablePropsCarrier } from './types/table'

defineProps<TablePropsCarrier<string>>()
</script>
<template><table><slot /></table></template>"#,
        )
        .unwrap();

    project.host().set_import_dependencies(
        "/types/table.ts",
        vec![crate::types::DependencyResolution {
            specifier: "external-table".to_string(),
            resolved_canonical_id: Some("/node_modules/external-table/index.d.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );
    project.host().set_import_dependencies(
        "/Table.vue",
        vec![crate::types::DependencyResolution {
            specifier: "./types/table".to_string(),
            resolved_canonical_id: Some("/types/table.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );

    let meta = get_meta(&project, "/Table.vue");
    let prop_names: Vec<&str> = meta.props.iter().map(|prop| prop.name.as_str()).collect();

    for name in &["state", "onStateChange", "renderFallbackValue"] {
        assert!(
            prop_names.contains(name),
            "Phase C Class B: explicit body re-introduction `{name}` \
             MUST be published when the `Omit<Unresolvable, ...>` \
             heritage arm is unresolvable. Got props: {prop_names:?}"
        );
    }
}

/// Fix 1 — analyzer-side `declared_in_macro_type_arg` contract for
/// the inline-literal `defineProps<{ ... }>()` form.
///
/// Verifies the brief's exact case: `defineProps<{ onSubmit?: ... }>()`
/// alongside `defineEmits<{ submit: ... }>()` must yield a prop
/// whose `declared_in_macro_type_arg` is `true`. With the fact set,
/// the `Refined` policy retains `onSubmit` against the `submit`
/// emit's "on Event" shadow.
///
/// **Discriminating property**: hardcoding `declared_in_macro_type_arg:
/// false` in the analyzer's inline-`TSTypeLiteral` extractor (or in
/// `PropAnalysis`) makes this assertion red. With the Fix 1
/// threading in place, it is green.
#[test]
fn inline_literal_define_props_marks_author_declared_for_on_event_shadow_of_declared_emit() {
    let project = make_project();
    project
        .upsert_base(
            "/Component.vue",
            r#"<script setup lang="ts">
defineProps<{ onSubmit?: (event: Event) => void; title?: string }>()
defineEmits<{ submit: [event: Event] }>()
</script>
<template><form><slot /></form></template>"#,
        )
        .unwrap();

    let meta = get_meta(&project, "/Component.vue");

    let on_submit = meta
        .props
        .iter()
        .find(|p| p.name == "onSubmit")
        .expect("Fix 1: inline-literal `onSubmit` should appear in meta.props");
    assert!(
        on_submit.declared_in_macro_type_arg,
        "Fix 1: inline-literal `defineProps<{{ onSubmit?: ... }}>()` \
         must mark `onSubmit` as author-declared at the macro T body. \
         Got declared_in_macro_type_arg = {}",
        on_submit.declared_in_macro_type_arg
    );

    let title = meta
        .props
        .iter()
        .find(|p| p.name == "title")
        .expect("Fix 1: sibling `title` should appear in meta.props");
    assert!(
        title.declared_in_macro_type_arg,
        "Fix 1: every member of an inline-literal `defineProps<{{...}}>()` \
         arg must be author-declared (got declared_in_macro_type_arg = \
         {} for `title`).",
        title.declared_in_macro_type_arg
    );
}

/// Fix 1 — analyzer-side provenance separation: when the macro T is
/// a local interface that `extends` an unresolvable external type,
/// the interface's own body members must be marked
/// `declared_in_macro_type_arg = true`. Heritage (`extends ...`)
/// members are walked with `declared = false` per the analyzer's
/// `resolve_interface_decl(..., declared = false)` call in
/// `resolve_type_to_prop_fields`.
///
/// **Discriminating property**: changing the analyzer's
/// `resolve_type_to_prop_fields` to use a single shared `declared`
/// across heritage AND own-body branches (instead of separating
/// `false` for heritage and the caller's `declared_in_macro_type_arg`
/// for own body) would either leak inheritance into the fact
/// (false-positive) or strip own-body fact (false-negative). Either
/// regression makes this assertion fail.
#[test]
fn local_interface_own_body_marked_declared_while_heritage_arm_unresolvable() {
    let project = make_project();
    project
        .upsert_base(
            "/Local.vue",
            r#"<script setup lang="ts">
interface Props extends UnresolvableExternalAttrs {
    onSubmit?: (event: Event) => void
    explicit?: string
}
defineProps<Props>()
defineEmits<{ submit: [event: Event] }>()
</script>
<template><form><slot /></form></template>"#,
        )
        .unwrap();

    let meta = get_meta(&project, "/Local.vue");

    let on_submit = meta
        .props
        .iter()
        .find(|p| p.name == "onSubmit")
        .expect("local-interface own body member `onSubmit` should appear");
    assert!(
        on_submit.declared_in_macro_type_arg,
        "Fix 1: `onSubmit` lives in the local interface's own body \
         (`interface Props extends Unresolvable {{ onSubmit?: ... }}`) — \
         it must be marked author-declared at the macro T. \
         Got declared_in_macro_type_arg = {}",
        on_submit.declared_in_macro_type_arg
    );

    let explicit = meta
        .props
        .iter()
        .find(|p| p.name == "explicit")
        .expect("local-interface own body member `explicit` should appear");
    assert!(
        explicit.declared_in_macro_type_arg,
        "Fix 1: sibling own-body member `explicit` must also be \
         author-declared (got declared_in_macro_type_arg = {}).",
        explicit.declared_in_macro_type_arg
    );
}

// Note: an integration-level F4 discriminating test was attempted in
// this file using an `Omit<Generic<T>, key>` cross-file heritage
// fixture, but empirically the component-meta pipeline's rescue paths
// (analyzer-side intersection branch, cold-resolver, parser-side
// utility-heritage handler) collectively cover the synthetic shape
// even when the prepared-surface intersection short-circuit is
// reverted. The unit-level discriminating coverage now lives in
// `crates/verter_session/src/resolver_core/component_meta_query_engine/prepared_surface.rs`
// under `intersection_merge_tests::merge_prepared_intersection_arms_*`,
// which exercises the pure `merge_prepared_intersection_arms` helper
// directly with synthesised `PreparedSurfaceProjection` inputs.
// Reverting the helper's `// Skip` arm to the pre-fix
// `return PreparedSurfaceProjection::Unsupported;` makes those unit
// tests fail with precise diffs — see
// `bench-evidence/r20fix2-f4-red-discrimination.txt` for the RED
// output observed during R20-fix2 verification.

/// R21-F1 c3 discriminating test for the `defineModel` projector
/// path. `defineModel<T>()` synthesizes the model member at the
/// macro's T position — the member is structurally author-declared
/// in the macro's type argument by virtue of the `defineModel`
/// syntax itself.
///
/// Asserts the published prop entry for the model name (default
/// `modelValue`, or the explicit name passed as the first argument)
/// carries `declared_in_macro_type_arg = true`.
///
/// **Discriminating property**: reverting the analyzer-side
/// `synthesize_model_prop_and_event` push to
/// `declared_in_macro_type_arg: false` causes both assertions below
/// to FAIL — that path is the load-bearing producer of `meta.props`
/// for defineModel. The `model.rs:project_model` projector site
/// independently sets `declared_in_macro_type_arg: true` on its
/// `ExpandedField` (R21-F1 c3) so the projector arm of the
/// publication chain also reports the correct structural fact when
/// the analyzer's synthesis is bypassed by a downstream consumer.
#[test]
fn r21_c3_define_model_props_marked_declared_in_macro_type_arg() {
    let project = make_project();
    project
        .upsert_base(
            "/Model.vue",
            r#"<script setup lang="ts">
defineModel<string>()
defineModel<boolean>('open')
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let meta = get_meta(&project, "/Model.vue");

    let model_value = meta
        .props
        .iter()
        .find(|p| p.name == "modelValue")
        .expect("defineModel<string>() must publish `modelValue` prop");
    assert!(
        model_value.declared_in_macro_type_arg,
        "R21-F1 c3: `modelValue` (defineModel<string>() default name) \
         must be marked declared_in_macro_type_arg = true. The model \
         member is structurally author-declared at the macro T \
         position via the `defineModel` syntax itself. Got declared = {}",
        model_value.declared_in_macro_type_arg
    );

    let open = meta
        .props
        .iter()
        .find(|p| p.name == "open")
        .expect("defineModel<boolean>('open') must publish `open` prop");
    assert!(
        open.declared_in_macro_type_arg,
        "R21-F1 c3: `open` (explicit defineModel name) must be marked \
         declared_in_macro_type_arg = true — the explicit name is \
         author-declared at the macro call site. Got declared = {}",
        open.declared_in_macro_type_arg
    );
}
