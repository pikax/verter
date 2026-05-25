//! Phase C cross-file regression tests for the
//! `prepared_surface.rs::TypeExpr::Intersection` non-fatal-unsupported
//! merge, plus Fix 1's analyzer-side `declared_in_macro_type_arg`
//! provenance contract.
//!
//! ## Phase C invariant under test
//!
//!   A `TypeExpr::Intersection([A, B])` lowered surface MUST publish
//!   the members contributed by any resolvable arm even when sibling
//!   arms return `PreparedSurfaceProjection::Unsupported`. Only when
//!   EVERY arm is `Unsupported` does the intersection collapse. This
//!   matches TypeScript's intersection semantics — an unresolvable
//!   `B` does not poison `A`'s contributions.
//!
//! ## Test scope and discrimination caveat
//!
//! The Class A / Class B tests below characterize the **post-fix
//! invariant**: with the non-fatal-unsupported merge in place, the
//! explicit body member survives an unresolvable heritage arm. They
//! are POSITIVE regression guards — if a future change reverts the
//! Intersection branch to the short-circuit form and these tests
//! still pass against synthetic fixtures, that does not vindicate the
//! revert: the prepared-surface path is one of several paths
//! contributing to `meta.props`, and synthetic single-component
//! fixtures often have the analyzer's local-resolver solver fallback
//! rescue them even when the prepared-surface intersection is broken
//! (this is the `[solver-rescue]` blocker the Phase C
//! implementer documented in the original report).
//!
//! The discriminating proof of the Phase C fix is the
//! 177-component nuxt-ui bench corpus — `pnpm --filter
//! @verter/benchmark bench:meta:ui -- --scenarios=repo_first_pass
//! --hard-timeout-ms=60000`. The corpus contains `AuthForm.vue`,
//! `Form.vue`, and `Table.vue` whose actual TS shapes do hit the
//! `prepared_surface.rs::TypeExpr::Intersection` branch and lose
//! `onSubmit` / `state` / `onStateChange` / `renderFallbackValue`
//! under the pre-fix short-circuit.
//!
//! The tests below remain valuable as positive regression characterizations:
//! if either the explicit body member OR the sibling assertion ever
//! disappears from these fixtures, that is a real signal that the
//! intersection short-circuit (or an equivalent regression) has
//! returned somewhere in the pipeline.
//!
//! ## Fix 1 `declared_in_macro_type_arg` analyzer-side contract
//!
//! Inline-literal `defineProps<{ onSubmit?: ... }>()` + a declared
//! `submit` emit must yield an `AnalyzedPropField` whose
//! `declared_in_macro_type_arg` is `true` (and `PropAnalysis` /
//! `FfiPropMeta` / `PropMeta` likewise). This is the structural fact
//! that the `Refined` publication policy consults to preserve
//! Vue intrinsics (`class`, `style`) and `on{Event}` shadows of
//! declared emits when the author wrote them on purpose.
//!
//! The Fix 1 tests (`inline_literal_..._for_on_event_shadow_of_declared_emit`
//! and `local_interface_own_body_marked_declared_while_heritage_arm_unresolvable`)
//! ARE discriminating at the analyzer boundary: removing the
//! `declared_in_macro_type_arg: true` populations from the analyzer's
//! body extractors makes them fail.
//!
//! Cross-file resolution (importing the prop interface from another
//! file) currently loses provenance at the `SurfaceMember` boundary
//! — that propagation belongs to a follow-up that threads the fact
//! through `prepared_surface.rs` and `SurfaceMember` to
//! `ExpandedField`. See `TODO(follow-up)` in
//! `extract_props_from_macro`.

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
