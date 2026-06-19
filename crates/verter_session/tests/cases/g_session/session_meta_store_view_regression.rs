//! Regression: the shared-host / SESSION component-meta entry must
//! provide a real request-bound `ResolverContext` (with a working
//! `store_view`) all the way down to the macro-DTO surface read.
//!
//! ## The bug this characterizes
//!
//! `extract_component_meta_from_resolved{,_with_facts}` receive a real
//! `ctx: &dyn ResolverContext` but used to pass the **bare `&VerterHost`**
//! (not `ctx`) into `component_meta_resolved_macros`, whose body calls
//! `vue_macro_dtos_with_ctx(ctx, …)` → `ctx.store_view()`. On the bare
//! `impl ResolverContext for VerterHost` rail, `store_view()` panics in a
//! production (`debug_assertions` OFF) build:
//!
//! ```text
//! internal compiler error: ResolverContext::store_view() called on bare
//! &VerterHost — construct HostResolverContext::new(host, &view, overlay)
//! at the request entry
//! ```
//!
//! That is exactly the panic the `repo_first_pass` meta-ui benchmark hit
//! on every nuxt-ui component (the bench is a `--release`, i.e.
//! `debug_assertions` OFF, native build). The host-direct
//! `VerterHost::get_component_meta` corpus test did NOT catch it because
//! it threads its own ctx end-to-end; the session payload entry
//! (`MetaSession::get_component_meta_payload` — the napi `getComponentMeta`
//! path the bench drives) reached the bare-host call.
//!
//! ## Why the loop body must fire (and what triggers it)
//!
//! `component_meta_resolved_macros` only resolves a macro DTO through
//! `vue_macro_dtos_with_ctx` for macros whose RAW surface is **not**
//! authoritative — i.e. the macro type argument is a CROSS-FILE imported
//! reference (`defineProps<ButtonProps>()` from `./types`) rather than an
//! inline object literal (`defineProps<{ a: string }>()`). An inline
//! literal short-circuits the loop body, so a single-file inline fixture
//! never reaches `ctx.store_view()`. The fixture below imports every
//! macro's type so the loop body fires for props / emits / slots.
//!
//! ## Discrimination
//!
//! This is an INTEGRATION test (`tests/*.rs`): the lib is compiled with
//! `cfg(test)` OFF. The bare-host `store_view()` fallback is gated
//! `#[cfg(any(test, debug_assertions))]` (leak) vs
//! `#[cfg(not(any(test, debug_assertions)))]` (panic). So:
//!
//! - `cargo test -p verter_session --tests` (DEBUG → `debug_assertions`
//!   ON): the leak arm returns a valid base view, so the buggy and fixed
//!   trees BOTH resolve the (correct) cross-file surface and this test
//!   passes. It still locks the resolved surface content against future
//!   regressions of the cross-file resolution itself.
//! - `cargo test --release -p verter_session
//!   --test main session_meta_store_view_regression` (RELEASE →
//!   `debug_assertions` OFF, `cfg(test)` OFF): the panic arm is active.
//!   On the PRE-FIX tree the bare host reaches `store_view()` and this
//!   test PANICS. On the POST-FIX tree the real ctx is threaded and it
//!   passes. That release run is the discriminating reproduction of the
//!   bench panic — run it both ways to prove the discrimination.

use verter_session::component_meta_host::ComponentMetaHost;
use verter_session::meta_resolve::ResolvedComponentMetaState;
use verter_session::{AnalysisLevel, HostConfig};

const TYPES_TS: &str = r#"export interface ButtonProps { label: string; size?: 'sm' | 'md' }
export interface ButtonEmits { (e: 'click', payload: number): void }
export interface RowApi { name: string; value: number }
export interface ButtonSlots {
  default(props: { item: number }): any
  row(props: Pick<RowApi, 'name'>): any
}
"#;

const CHILD_VUE: &str = r#"<script setup lang="ts">
import type { ButtonProps } from './types'
defineProps<ButtonProps>()
</script>
<template><button><slot /></button></template>"#;

// Owner SFC: EVERY macro takes a CROSS-FILE imported type argument, so
// `component_meta_resolved_macros` resolves each DTO through
// `vue_macro_dtos_with_ctx(ctx, …)`. It also renders a child `.vue`
// component (single component root) so the fallthrough recursion runs.
const OWNER_VUE: &str = r#"<script setup lang="ts">
import type { ButtonProps, ButtonEmits, ButtonSlots } from './types'
import Child from './Child.vue'
defineProps<ButtonProps>()
defineEmits<ButtonEmits>()
defineModel<string>()
defineSlots<ButtonSlots>()
</script>
<template><Child>content</Child></template>"#;

fn make_host() -> ComponentMetaHost {
    let host = ComponentMetaHost::new_standalone(HostConfig {
        analysis_level: AnalysisLevel::Full,
        ..HostConfig::default()
    });
    host.upsert_base("/types.ts", TYPES_TS).unwrap();
    host.upsert_base("/Child.vue", CHILD_VUE).unwrap();
    host.upsert_base("/Comp.vue", OWNER_VUE).unwrap();
    host
}

/// The exact bench entry: napi `getComponentMeta` →
/// `MetaSession::get_component_meta_payload`. PRE-FIX this panics on the
/// bare-host `store_view()` in a release build; POST-FIX it threads the
/// real ctx and returns the encoded payload.
#[test]
fn session_payload_cross_file_macros_resolve_via_real_ctx() {
    let host = make_host();
    let session = host.open_session_batch().expect("session opens");

    // Encode just the per-kind member counts so we can assert the
    // cross-file surface resolved (and not via an always-true predicate).
    fn encode(
        analysis: verter_semantic::analysis::component_meta::ComponentMetaAnalysis,
        _resolved: &ResolvedComponentMetaState,
    ) -> Vec<u8> {
        format!(
            "props={} events={} slots={}",
            analysis.props.len(),
            analysis.events.len(),
            analysis.slots.len(),
        )
        .into_bytes()
    }

    let payload = session
        .get_component_meta_payload("/Comp.vue", encode)
        .expect("payload call succeeds (no bare-host store_view panic)")
        .expect("owner resolves to a component payload");
    let text = String::from_utf8(payload).expect("payload is the marker string");

    // Discriminating content: the cross-file `ButtonProps` (label, size)
    // plus `defineModel`'s `modelValue` => 3 props; `ButtonEmits` (click)
    // plus model's `update:modelValue` => 2 events; `ButtonSlots`
    // (default, row) => 2 slots. A bare-host read in a leak (debug) build
    // resolves the SAME surface, so this also locks the resolution.
    assert_eq!(
        text, "props=3 events=2 slots=2",
        "session payload path must resolve the cross-file macro surface; got `{text}`"
    );
}

/// The structured session entry (`MetaSession::get_component_meta`) shares
/// the same `extract_component_meta_from_resolved_with_facts` ->
/// `component_meta_resolved_macros` path, so it has the same bare-host
/// exposure. Assert the concrete member NAMES so the surface is locked,
/// not just the counts.
#[test]
fn session_structured_cross_file_macros_resolve_via_real_ctx() {
    let host = make_host();
    let session = host.open_session_batch().expect("session opens");

    let meta = session
        .get_component_meta("/Comp.vue")
        .expect("query succeeds (no bare-host store_view panic)")
        .expect("owner resolves to a component");

    let prop_names: Vec<&str> = meta.props.iter().map(|p| p.name.as_str()).collect();
    let event_names: Vec<&str> = meta.events.iter().map(|e| e.name.as_str()).collect();
    let slot_names: Vec<&str> = meta.slots.iter().map(|s| s.name.as_str()).collect();

    // Cross-file `ButtonProps` members + the model prop.
    assert!(
        prop_names.contains(&"label"),
        "imported ButtonProps.label must resolve; got {prop_names:?}"
    );
    assert!(
        prop_names.contains(&"size"),
        "imported ButtonProps.size must resolve; got {prop_names:?}"
    );
    assert!(
        prop_names.contains(&"modelValue"),
        "defineModel must publish modelValue; got {prop_names:?}"
    );
    // Cross-file `ButtonEmits` event + the model update event.
    assert!(
        event_names.contains(&"click"),
        "imported ButtonEmits.click must resolve; got {event_names:?}"
    );
    assert!(
        event_names.contains(&"update:modelValue"),
        "defineModel must publish update:modelValue; got {event_names:?}"
    );
    // Cross-file `ButtonSlots` members.
    assert!(
        slot_names.contains(&"default"),
        "imported ButtonSlots.default must resolve; got {slot_names:?}"
    );
    assert!(
        slot_names.contains(&"row"),
        "imported ButtonSlots.row must resolve; got {slot_names:?}"
    );
}

/// The child `.vue` (whose props are a cross-file import) resolved through
/// the same session payload path — the simplest single-macro witness that
/// the loop body fires for an imported props type.
#[test]
fn session_child_cross_file_props_resolve_via_real_ctx() {
    let host = make_host();
    let session = host.open_session_batch().expect("session opens");

    let meta = session
        .get_component_meta("/Child.vue")
        .expect("query succeeds (no bare-host store_view panic)")
        .expect("child resolves to a component");

    let prop_names: Vec<&str> = meta.props.iter().map(|p| p.name.as_str()).collect();
    assert!(
        prop_names.contains(&"label") && prop_names.contains(&"size"),
        "imported ButtonProps must resolve on the child; got {prop_names:?}"
    );
}
