//! Batch / scalar public-API render paths capture ONE fixed store view per
//! batch (and per scalar N=1 call) and thread its shared cold seed + the base
//! session view through the registry's component-API projector.
//!
//! Invariants characterized here:
//!
//! 1. **`from_host` calls are O(1), not O(N).** A warm batch of N macro-bearing
//!    SFCs performs a (near-)constant number of `HostStoreView::from_host`
//!    calls — the one per-batch `capture_batch_fixed_view`, not one per item.
//!    The legacy per-call read on the macro-deps path
//!    (`virtual_file_pipeline.rs` `resolver_store_view_read().into_cold_seed_view()`)
//!    was THE O(N²) cliff: each macro-bearing render took its own store-view
//!    read, and that read missed the warm cache because the call's first deep
//!    semantic demand bumped `artifact_generation` → advanced the store-view
//!    token → next call rebuilt. The batch collapses all of it onto one read.
//! 2. **Full-workspace sweeps stay O(1).** A warm batch performs ~O(1) actual
//!    `build_coherent` sweeps — one capture per batch.
//! 3. **Batch == scalar.** Scalar (`get_public_api`) and batch
//!    (`get_public_api_batch`) are the SAME shared `render_public_api_items`
//!    body (scalar = N=1), so the rendered bytes (`code` + `source_map`) are
//!    byte-identical for the same component across shared/unique imports,
//!    SFC-to-SFC macro deps, an inert external augmenter, base-host mutation,
//!    and mid-batch lazy publication. Cross-item correctness is served by
//!    per-item ON-DEMAND materialization + GLOBAL artifact publication (NOT a
//!    shared batch overlay): each item's render creates its own fresh
//!    `CanonicalCompletionOverlay`; the shared cold seed only supplies the
//!    stable base snapshot that avoids the O(N) per-call store-view rebuild.

use std::sync::Arc;

use crate::resolver_store::COHERENT_BUILD_SWEEPS_THIS_THREAD;
use crate::types::{AnalysisLevel, HostConfig, TscResponse, UpsertRequest};
use crate::{FileLanguage, VerterHost};

fn single_thread_scheduler() -> verter_scheduler::scheduler::SchedulerConfig {
    // Single CPU thread so the PER-THREAD coherent-sweep counter
    // (`warm_public_api_batch_sweeps_stay_o1`) reflects only this batch's
    // sweeps. The public-API batch runs SEQUENTIALLY on the calling thread
    // anyway (no coordinator fan-out — it mutates the dependency cache via
    // `sync_transitive_macro_type_dependencies`), so the thread-local captures
    // exactly this batch's sweeps regardless; the single thread is belt-and-
    // braces against any internal pool work.
    verter_scheduler::scheduler::SchedulerConfig {
        cpu_threads: 1,
        ..verter_scheduler::scheduler::SchedulerConfig::default()
    }
}

fn make_host() -> VerterHost {
    VerterHost::new_standalone_with_scheduler_config(
        HostConfig {
            analysis_level: AnalysisLevel::Full,
            ..HostConfig::default()
        },
        single_thread_scheduler(),
    )
}

fn upsert_vue(host: &VerterHost, canonical: &str, source: &str) {
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: Some(canonical.to_string()),
            input_id: canonical.to_string(),
            source: Arc::<str>::from(source),
            file_language: FileLanguage::vue(),
            aliases: Vec::new(),
        })
        .unwrap_or_else(|e| panic!("upsert {canonical}: {e:?}"));
}

fn upsert_ts(host: &VerterHost, canonical: &str, source: &str) {
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: Some(canonical.to_string()),
            input_id: canonical.to_string(),
            source: Arc::<str>::from(source),
            file_language: FileLanguage::script_ts(),
            aliases: Vec::new(),
        })
        .unwrap_or_else(|e| panic!("upsert {canonical}: {e:?}"));
}

/// CRLF -> LF normalization (cross-platform byte-comparison rule).
fn norm(s: &str) -> String {
    s.replace("\r\n", "\n")
}

/// Normalized `(code, source_map)` for a `TscResponse`.
fn norm_response(r: &TscResponse) -> (String, Option<String>) {
    (norm(&r.code), r.source_map.as_ref().map(|m| norm(m)))
}

const SHARED_TYPES_TS: &str = r#"export interface ButtonProps { label: string; size?: 'sm' | 'md' }
export interface ButtonEmits { (e: 'click', payload: number): void }
"#;

/// Cross-file owner SFC: `defineProps`/`defineEmits` take IMPORTED type
/// arguments, so each render walks the import graph (the macro-deps path that
/// took the per-call store-view read) — not an inline-literal short-circuit.
fn shared_owner_sfc(idx: usize) -> String {
    format!(
        r#"<script setup lang="ts">
import type {{ ButtonProps, ButtonEmits }} from './types'
defineProps<ButtonProps>()
defineEmits<ButtonEmits>()
const local_{idx} = {idx}
</script>
<template><button>{{{{ local_{idx} }}}}</button></template>"#
    )
}

/// Build `count` cross-file SFCs that all import the SHARED `./types`. Returns
/// the canonical ids in input order.
fn build_shared_corpus(host: &VerterHost, count: usize) -> Vec<String> {
    upsert_ts(host, "/src/types.ts", SHARED_TYPES_TS);
    let mut ids = Vec::with_capacity(count);
    for i in 0..count {
        let canonical = format!("/src/Comp{i}.vue");
        upsert_vue(host, &canonical, &shared_owner_sfc(i));
        ids.push(canonical);
    }
    ids
}

fn id_refs(ids: &[String]) -> Vec<&str> {
    ids.iter().map(String::as_str).collect()
}

/// Assert the scalar (`get_public_api`) bytes equal the batch
/// (`get_public_api_batch`) slot for every id (code AND source_map, CRLF->LF
/// normalized). Returns the batch responses for further content assertions.
fn assert_scalar_equals_batch(host: &VerterHost, ids: &[String]) -> Vec<Option<TscResponse>> {
    let refs = id_refs(ids);
    let batch = public_api_batch(host, &refs);
    assert_eq!(batch.len(), ids.len(), "one batch slot per input id");
    for (id, slot) in ids.iter().zip(batch.iter()) {
        let scalar = host
            .get_public_api(id)
            .expect("scalar public API projection");
        match (scalar.as_ref(), slot.as_ref()) {
            (Some(s), Some(b)) => {
                assert_eq!(
                    norm_response(b),
                    norm_response(s),
                    "batch bytes diverged from scalar bytes for {id} — the fixed \
                     view must change only how reads are served, never the output",
                );
            }
            (None, None) => {}
            (s, b) => panic!(
                "scalar/batch presence diverged for {id}: scalar.is_some()={} \
                 batch.is_some()={}",
                s.is_some(),
                b.is_some()
            ),
        }
    }
    batch
}

fn public_api_batch(host: &VerterHost, ids: &[&str]) -> Vec<Option<TscResponse>> {
    host.get_public_api_batch(ids)
        .into_iter()
        .map(|slot| slot.expect("batch public API projection"))
        .collect()
}

// ── (1) THE perf proof / hard gate ──────────────────────────────────────────

/// A WARM public-API batch of N macro-bearing SFCs performs O(1) `from_host`
/// store-view reads, NOT O(N).
///
/// HERMETIC (per-host counter): reads
/// `host.provenance().store_view_from_host_reads`, a per-`VerterHost` atomic
/// bumped in the `HostStoreView::from_host_read` chokepoint. `make_host` builds
/// a fresh host, so a concurrent test reading store views on a DIFFERENT host
/// can never inflate this reset→measure window.
///
/// DISCRIMINATION: a per-item batch (loop `get_public_api` per id) takes the
/// legacy `resolver_store_view_read()` once PER macro-bearing render — so a
/// warm batch of N performs ≥ N `from_host` reads, failing the `< N` bound.
/// The shared per-batch `capture_batch_fixed_view` is one read for the whole
/// batch; every item threads its cold seed, taking ZERO further reads, so the
/// delta is a small constant. The companion `>= 1` assertion proves the
/// counter is LIVE (the capture's read was counted), so a dead counter cannot
/// trivially satisfy `< N` with 0.
#[test]
fn warm_public_api_batch_from_host_calls_are_o1_not_per_item() {
    use std::sync::atomic::Ordering::Relaxed;
    let host = make_host();
    const N: usize = 12;
    let ids = build_shared_corpus(&host, N);
    let refs = id_refs(&ids);

    // Cold pass: populate the extract + transitive-dep caches.
    let cold = public_api_batch(&host, &refs);
    assert_eq!(cold.len(), N, "one slot per input");
    assert!(
        cold.iter().all(|slot| slot.is_some()),
        "every cold slot renders a public-API surface",
    );

    // WARM pass: measure this host's `from_host` reads in isolation.
    host.provenance()
        .store_view_from_host_reads
        .store(0, Relaxed);
    let warm = public_api_batch(&host, &refs);
    let warm_from_host = host.provenance().store_view_from_host_reads.load(Relaxed);

    assert_eq!(warm.len(), N);
    assert!(
        warm.iter().all(|slot| slot.is_some()),
        "every warm slot still renders a public-API surface",
    );
    assert!(
        warm_from_host >= 1,
        "the warm batch MUST perform at least one real `from_host` read on this \
         host (the per-batch fixed-view capture), so a dead counter cannot \
         trivially satisfy the O(1) bound below; observed {warm_from_host}",
    );
    assert!(
        warm_from_host < N as u64,
        "a warm public-API batch of N={N} must perform O(1) `from_host` calls \
         (the one per-batch fixed-view capture), NOT one per item. Observed \
         {warm_from_host} `from_host` reads on this host — a per-item batch \
         re-reads the store view in each macro-bearing render (≥ N={N}).",
    );
}

/// A WARM public-API batch performs ~O(1) actual full-workspace
/// `build_coherent` SWEEPS (not O(N)).
///
/// DISCRIMINATION: the per-thread sweep counter bumps once per real sweep. With
/// the single per-batch capture a warm batch sweeps at most once. A per-item
/// path that re-read + rebuilt the base view per item would drive this O(N).
#[test]
fn warm_public_api_batch_sweeps_stay_o1() {
    let host = make_host();
    const N: usize = 12;
    let ids = build_shared_corpus(&host, N);
    let refs = id_refs(&ids);

    // Cold pass warms the manager's base view + the result caches.
    let _ = public_api_batch(&host, &refs);

    COHERENT_BUILD_SWEEPS_THIS_THREAD.with(|c| c.set(0));
    let _ = public_api_batch(&host, &refs);
    let warm_sweeps = COHERENT_BUILD_SWEEPS_THIS_THREAD.with(std::cell::Cell::get);

    assert!(
        warm_sweeps <= 1,
        "a warm public-API batch of N={N} must collapse onto ~O(1) full-\
         workspace sweeps (the single per-batch capture); observed \
         {warm_sweeps} on this thread. An O(N) value means the per-item path \
         rebuilt the base view per render.",
    );
}

// ── (2) Scalar↔batch byte identity (general) ────────────────────────────────

/// `get_public_api(id)` bytes == the `get_public_api_batch([...])` slot for
/// that id — `TscResponse.code` AND `source_map` byte-identical (CRLF->LF
/// normalized) — and the rendered surface carries the cross-file MATERIALIZED
/// emit payload (a real import-graph walk, not an empty / failed resolution).
#[test]
fn public_api_batch_equals_scalar_bytes() {
    let host = make_host();
    const N: usize = 5;
    let ids = build_shared_corpus(&host, N);
    let batch = assert_scalar_equals_batch(&host, &ids);

    // Discriminating content: the cross-file `ButtonEmits` call signature
    // (`(e: 'click', payload: number)`) MATERIALIZES into the surface as
    // `payload: number` (emit signatures expand cross-file; imported props
    // would stay shallow refs). A stale / empty / failed import-graph walk would
    // leave the emit unresolved → no `payload: number` → RED. (The shallow
    // `ButtonProps` prop ref appears regardless of the walk — it comes from the
    // SFC's own macro arg — so it is NOT a discriminating content check.)
    let first = batch[0].as_ref().expect("first slot renders");
    assert!(
        first.code.contains("payload: number"),
        "the rendered public-API surface must MATERIALIZE the imported \
         `ButtonEmits` payload as `payload: number`; got:\n{}",
        first.code,
    );
}

// ── (3) Correctness matrix — each a scalar↔batch byte-identity case ──────────

/// (a) Shared imports: every SFC imports the SAME `./types`.
#[test]
fn matrix_shared_imports_scalar_equals_batch() {
    let host = make_host();
    let ids = build_shared_corpus(&host, 6);
    let batch = assert_scalar_equals_batch(&host, &ids);
    // Discriminating: every item resolves the SHARED `./types` `ButtonEmits`
    // and MATERIALIZES its `payload: number` (a stale view sharing the one fixed
    // view would drop it). Beyond byte parity, this proves the shared view
    // serves the same cross-file dep to every item, not an empty resolution.
    for (i, slot) in batch.iter().enumerate() {
        let r = slot.as_ref().unwrap_or_else(|| panic!("Comp{i} renders"));
        assert!(
            r.code.contains("payload: number"),
            "Comp{i} must MATERIALIZE the shared `ButtonEmits` payload \
             `payload: number`; got:\n{}",
            r.code,
        );
    }
}

/// (b) Unique imports: each SFC imports its OWN `./types_i`.
///
/// Mutation recipe: make terminal event literals single-quoted, collapse every
/// handler to `() => void`, or route every row to `go0`. The exact `$emit` and
/// handler assertions below must reject the quote/shape/ownership mutation,
/// while `public_api_batch_equals_scalar_bytes` remains a passing control.
#[test]
fn matrix_unique_imports_scalar_equals_batch() {
    let host = make_host();
    const N: usize = 6;
    let mut ids = Vec::with_capacity(N);
    for i in 0..N {
        upsert_ts(
            &host,
            &format!("/src/types_{i}.ts"),
            &format!(
                "export interface P{i} {{ id_{i}: number; label_{i}: string }}\n\
                 export interface E{i} {{ (e: 'go{i}', n: number): void }}\n"
            ),
        );
        let sfc = format!(
            r#"<script setup lang="ts">
import type {{ P{i}, E{i} }} from './types_{i}'
defineProps<P{i}>()
defineEmits<E{i}>()
</script>
<template><div /></template>"#
        );
        let canonical = format!("/src/Uniq{i}.vue");
        upsert_vue(&host, &canonical, &sfc);
        ids.push(canonical);
    }
    let batch = assert_scalar_equals_batch(&host, &ids);
    // Each surface resolves its OWN distinct cross-file import with NO cross-talk
    // between items sharing the one fixed view. Discriminating on a MATERIALIZED
    // surface: item i's emit `E{i} = (e: 'go{i}', n: number)` materializes as
    // the compiler-canonical double-quoted `$emit` overload plus its exact
    // handler prop, and item i must NOT carry any OTHER item's rows. A
    // shared-view cross-talk bug (item i served item j's resolved emit) would
    // surface a foreign signature/handler → RED; a stale/empty view would drop
    // item i's own rows → RED.
    for (i, slot) in batch.iter().enumerate() {
        let r = slot.as_ref().unwrap_or_else(|| panic!("Uniq{i} renders"));
        let own_emit = format!(r#"((event: "go{i}", n: number) => void)"#);
        let own_handler = format!(r#""onGo{i}"?: (n: number) => void"#);
        assert!(
            r.code.contains(&own_emit) && r.code.contains(&own_handler),
            "Uniq{i} surface must MATERIALIZE its exact `{own_emit}` overload \
             and `{own_handler}` handler; \
             got:\n{}",
            r.code,
        );
        for j in 0..N {
            if j == i {
                continue;
            }
            let foreign_emit = format!(r#"((event: "go{j}", n: number) => void)"#);
            let foreign_handler = format!(r#""onGo{j}"?: (n: number) => void"#);
            assert!(
                !r.code.contains(&foreign_emit) && !r.code.contains(&foreign_handler),
                "Uniq{i} surface leaked another item's `{foreign_emit}` overload \
                 or `{foreign_handler}` handler (fixed-view cross-talk); got:\n{}",
                r.code,
            );
        }
    }
}

/// (c) SFC-to-SFC macro deps: a `.vue` macro imports a type exported from
/// ANOTHER `.vue`'s `<script>` — the hardest cross-file case. Batch and scalar
/// MUST agree, AND the cross-SFC EMIT type must materialize.
#[test]
fn matrix_sfc_to_sfc_macro_dep_scalar_equals_batch() {
    let host = make_host();
    // Child.vue exports a props model AND an emit call-signature interface from
    // its plain `<script>` block.
    upsert_vue(
        &host,
        "/src/Child.vue",
        r#"<script lang="ts">
export interface ChildModel { childId: number; childLabel: string }
export interface ChildEmits { (e: 'childEvt', payload: number): void }
</script>
<script setup lang="ts">
defineProps<{ a: string }>()
</script>
<template><div /></template>"#,
    );
    // Parent.vue imports the sibling SFC's props model (shallow ref) AND its
    // emit interface (materialized).
    upsert_vue(
        &host,
        "/src/Parent.vue",
        r#"<script setup lang="ts">
import type { ChildModel, ChildEmits } from './Child.vue'
defineProps<ChildModel>()
defineEmits<ChildEmits>()
</script>
<template><div /></template>"#,
    );
    let ids = vec!["/src/Child.vue".to_string(), "/src/Parent.vue".to_string()];
    let batch = assert_scalar_equals_batch(&host, &ids);
    assert!(
        batch.iter().all(|s| s.is_some()),
        "both SFCs render a public-API surface",
    );
    // Parent's emit surface MATERIALIZES the sibling SFC's `ChildEmits`
    // `(e: 'childEvt', payload: number)` — only reachable by resolving the
    // cross-SFC `<script>` export. A failed/empty cross-SFC walk would drop it.
    let parent = batch[1].as_ref().expect("Parent renders");
    assert!(
        parent.code.contains("event: \"childEvt\", payload: number"),
        "Parent must MATERIALIZE the sibling SFC's `ChildEmits` \
         `(event: \"childEvt\", payload: number)` signature; got:\n{}",
        parent.code,
    );
}

/// (d) An external `declare module` augmenter in the workspace does NOT poison
/// shared-view resolution: the SFC still materializes its imported emit, and
/// scalar == batch. The augmented PROP member stays shallow (shallow-by-default).
///
/// RE-SCOPED (was `matrix_declaration_augmentation_scalar_equals_batch`, which
/// only asserted the shallow `AugProps` ref is present — non-discriminating,
/// since that ref comes from the SFC's own macro arg and survives any
/// resolution outcome).
///
/// ARCHITECTURAL NOTE — augmentation is INVISIBLE on this surface: the
/// verter-tsc public-API surface is SHALLOW-by-default (the Component-Meta
/// Shallow-By-Default CRITICAL rule — imported alias names stay shallow). An
/// imported named PROP type (`AugProps`) is emitted as a REF, never expanded
/// into members, so a `declare module` augmenter that ADDS members to it cannot
/// appear (there is no expanded surface for the augmented `extra` to land in).
/// Verified empirically: emit CALL-signature augmentation is likewise not
/// stitched on this external-type collection path — only PRIMITIVE emit payloads
/// inline. So "the augmentation materializes into the surface" is NOT a property
/// this surface has; asserting the augmented `extra` is present would be RED on
/// the CORRECT tree. Deep-augmentation coverage belongs to `get_component_meta`,
/// not the shallow public-API surface.
///
/// What this DOES prove, discriminatingly:
///  * with an augmenter file LOADED in the workspace, the SFC's imported EMIT
///    (`AugEmits`) still MATERIALIZES `payload: number` — a stale / empty /
///    augmenter-poisoned shared view would drop it → RED, AND
///  * scalar == batch byte identity, AND
///  * the augmented PROP member `extra` stays SHALLOW (absent) — documents the
///    shallow-by-default boundary.
/// Mutation recipe: single-quote terminal literals, replace the `AugEmits`
/// handler parameters with `()`, or retarget the event name. The exact emit +
/// handler pair must fail; the shallow `AugProps` assertion remains a control.
#[test]
fn matrix_external_augmenter_does_not_poison_emit_materialization_scalar_equals_batch() {
    let host = make_host();
    upsert_ts(
        &host,
        "/src/base.ts",
        "export interface AugProps { label: string }\n\
         export interface AugEmits { (e: 'augd', payload: number): void }\n",
    );
    // An ambient augmenter that adds a member to the imported PROP interface.
    upsert_ts(
        &host,
        "/src/augment.ts",
        "import './base'\ndeclare module './base' {\n  interface AugProps { extra: number }\n}\n",
    );
    upsert_vue(
        &host,
        "/src/Augmented.vue",
        r#"<script setup lang="ts">
import type { AugProps, AugEmits } from './base'
defineProps<AugProps>()
defineEmits<AugEmits>()
</script>
<template><div /></template>"#,
    );
    let ids = vec!["/src/Augmented.vue".to_string()];
    let batch = assert_scalar_equals_batch(&host, &ids);
    let r = batch[0].as_ref().expect("augmented SFC renders");
    // The imported emit still MATERIALIZES despite the augmenter in the
    // workspace (the augmenter does not poison the shared-view resolution).
    assert!(
        r.code
            .contains(r#"((event: "augd", payload: number) => void)"#)
            && r.code.contains(r#""onAugd"?: (payload: number) => void"#),
        "with an external augmenter loaded, the imported `AugEmits` must still \
         MATERIALIZE the canonical `((event: \"augd\", payload: number) => void)` \
         overload and exact handler; got:\n{}",
        r.code,
    );
    // The augmented PROP member stays SHALLOW (shallow-by-default): `AugProps`
    // is a bare ref, and the augmenter's `extra` does NOT appear.
    assert!(
        r.code.contains("AugProps") && !r.code.contains("extra"),
        "the imported PROP type must stay a shallow `AugProps` ref with NO \
         expanded `extra` member (shallow-by-default); got:\n{}",
        r.code,
    );
}

/// (e) BASE-HOST dependency mutation visible through the threaded base
/// `HostViewRef`.
///
/// SCOPE (reframed): this proves the batch threads the LIVE base host view
/// (`HostViewRef::new(self)`) — NOT a session / completion overlay. It mutates
/// the BASE host (re-`upsert`s the dependency) and confirms the next batch
/// observes the new content. Emit signatures ARE materialized cross-file
/// (unlike props, which stay shallow refs), so mutating the DEPENDENCY's emit
/// payload type changes the rendered surface: after the mutation both scalar and
/// batch MUST resolve the NEW payload AND agree byte-for-byte. A stale /
/// `None`-degraded threaded view would still resolve the OLD payload (or diverge
/// from scalar).
///
/// Session/completion-overlay coverage is N/A here: there is NO session-scoped
/// public-API entry — the host-level path threads the base `HostViewRef`, never
/// an overlay view. A future session-scoped public-API entry would need the real
/// overlay view (and likely a `SessionResolverContext`) threaded instead of the
/// base view; until such an entry exists there is no overlay path to cover.
#[test]
fn matrix_dependency_emit_mutation_base_host_view_scalar_equals_batch() {
    let host = make_host();
    upsert_ts(
        &host,
        "/src/types.ts",
        "export interface MEmits { (e: 'go', payload: number): void }\n",
    );
    upsert_vue(
        &host,
        "/src/Mut.vue",
        r#"<script setup lang="ts">
import type { MEmits } from './types'
defineEmits<MEmits>()
</script>
<template><div /></template>"#,
    );
    let ids = vec!["/src/Mut.vue".to_string()];
    let before = assert_scalar_equals_batch(&host, &ids);
    let r0 = before[0].as_ref().expect("renders before mutation");
    assert!(
        r0.code.contains("payload: number"),
        "the imported emit payload must materialize as `number` before the \
         mutation; got:\n{}",
        r0.code,
    );

    // Mutate the DEPENDENCY's emit payload: number -> string. The threaded LIVE
    // view must observe it on the next batch.
    upsert_ts(
        &host,
        "/src/types.ts",
        "export interface MEmits { (e: 'go', payload: string): void }\n",
    );
    let after = assert_scalar_equals_batch(&host, &ids);
    let r1 = after[0].as_ref().expect("renders after mutation");
    assert!(
        r1.code.contains("payload: string") && !r1.code.contains("payload: number"),
        "after the dependency emit mutation the threaded view must resolve the \
         NEW `string` payload (a stale / None-degraded view would still show \
         `number`); got:\n{}",
        r1.code,
    );
}

/// (f) MID-BATCH lazy publication (forward order): a LATER item imports a type
/// declared in an EARLIER item's `.vue` script. At fixed-view capture neither
/// is materialized; per-item on-demand materialization + global artifact
/// publication (NOT a shared batch overlay) make item-2 resolve item-1's type.
/// The batch slot for the consumer MUST equal its scalar render.
///
/// DISCRIMINATION (strengthened): the consumer imports the EARLIER dep's EMIT
/// type and `defineEmits<DepEmits>()`. Emit call signatures MATERIALIZE into the
/// public-API output (`$emit: ((event: 'fwd', payload: number) => void)`),
/// unlike imported PROPS which stay shallow refs — so the consumer's bytes
/// DEPEND on the earlier dep's actual `<script>` content. A stale / shallow /
/// empty / `None`-degraded threaded view that failed to serve the earlier dep's
/// type on demand would drop `payload: number`, flipping the assertion RED. A
/// bare `is_some()` (the pre-strengthening assertion) passed even on a shallow
/// `DepModel` prop ref and so could not catch that.
/// Mutation recipe: single-quote terminal literals, erase the handler payload,
/// or retarget the admitted event away from `fwd`. The exact overload + handler
/// pair must fail while `public_api_batch_equals_scalar_bytes` stays green.
#[test]
fn matrix_midbatch_lazy_publication_forward_scalar_equals_batch() {
    let host = make_host();
    upsert_vue(
        &host,
        "/src/Dep.vue",
        r#"<script lang="ts">
export interface DepEmits { (e: 'fwd', payload: number): void }
</script>
<script setup lang="ts">
defineProps<{ z: string }>()
</script>
<template><div /></template>"#,
    );
    upsert_vue(
        &host,
        "/src/Consumer.vue",
        r#"<script setup lang="ts">
import type { DepEmits } from './Dep.vue'
defineEmits<DepEmits>()
</script>
<template><div /></template>"#,
    );
    // Forward: dep first, consumer second.
    let ids = vec!["/src/Dep.vue".to_string(), "/src/Consumer.vue".to_string()];
    let batch = assert_scalar_equals_batch(&host, &ids);
    let consumer = batch[1]
        .as_ref()
        .expect("the consumer (later item, depends on item-1's type) must resolve");
    // The consumer's emit surface carries the EARLIER dep's MATERIALIZED payload
    // — only reachable by on-demand materializing `Dep.vue`'s `<script>` export.
    assert!(
        consumer
            .code
            .contains(r#"((event: "fwd", payload: number) => void)"#)
            && consumer
                .code
                .contains(r#""onFwd"?: (payload: number) => void"#),
        "the consumer's emit surface must MATERIALIZE the earlier-declared dep's \
         canonical `((event: \"fwd\", payload: number) => void)` overload and \
         exact handler; a stale/shallow/empty view would drop it. got:\n{}",
        consumer.code,
    );
}

/// (f-inverse) MID-BATCH lazy publication (inverse order): the consumer is
/// FIRST and its dependency is declared LATER in the id list. The on-demand
/// `ensure_indexed_ready_serve` of the (later) dep makes the consumer resolve
/// regardless of batch order; batch MUST equal scalar.
///
/// DISCRIMINATION (strengthened): the consumer imports the dep's EMIT type and
/// `defineEmits<DepBEmits>()`. Emit call signatures MATERIALIZE into the
/// public-API output (`$emit: ((event: 'bInv', payload: number) => void)`),
/// unlike imported PROPS which stay shallow refs. So the consumer's bytes
/// DEPEND on the LATER dep's actual `<script>` content: a stale / shallow /
/// empty / `None`-degraded threaded view that failed to materialize the
/// later-declared dep on demand would drop `payload: number` (the emit would
/// fall back to an unresolved signature) — flipping the assertion RED. A bare
/// `is_some()` (the pre-strengthening assertion) could not catch that.
/// Mutation recipe: single-quote terminal literals, erase the handler payload,
/// or retarget the admitted event away from `bInv`. The exact overload +
/// handler pair must fail while `public_api_batch_equals_scalar_bytes` stays green.
#[test]
fn matrix_midbatch_lazy_publication_inverse_scalar_equals_batch() {
    let host = make_host();
    upsert_vue(
        &host,
        "/src/DepB.vue",
        r#"<script lang="ts">
export interface DepBEmits { (e: 'bInv', payload: number): void }
</script>
<script setup lang="ts">
defineProps<{ z: string }>()
</script>
<template><div /></template>"#,
    );
    upsert_vue(
        &host,
        "/src/ConsumerB.vue",
        r#"<script setup lang="ts">
import type { DepBEmits } from './DepB.vue'
defineEmits<DepBEmits>()
</script>
<template><div /></template>"#,
    );
    // Inverse: consumer FIRST, its dep LATER in the id list.
    let ids = vec![
        "/src/ConsumerB.vue".to_string(),
        "/src/DepB.vue".to_string(),
    ];
    let batch = assert_scalar_equals_batch(&host, &ids);
    let consumer = batch[0]
        .as_ref()
        .expect("the consumer (first item, dep declared later in the list) must resolve");
    // The consumer's emit surface carries the LATER dep's MATERIALIZED payload
    // — only reachable by on-demand materializing `DepB.vue` during the
    // consumer's render (it is the SECOND id, not yet rendered at this point).
    assert!(
        consumer
            .code
            .contains(r#"((event: "bInv", payload: number) => void)"#)
            && consumer
                .code
                .contains(r#""onBInv"?: (payload: number) => void"#),
        "the consumer's emit surface must MATERIALIZE the later-declared dep's \
         canonical `((event: \"bInv\", payload: number) => void)` overload and \
         exact handler (on-demand serve of the LATER batch item); a \
         stale/shallow/empty view would drop it. got:\n{}",
        consumer.code,
    );
}

#[test]
fn empty_public_api_batch_is_empty() {
    let host = make_host();
    let batch = public_api_batch(&host, &[]);
    assert!(batch.is_empty(), "an empty batch returns no slots");
}

/// Order preservation + non-Vue / missing slots are `None` (not dropped).
#[test]
fn public_api_batch_preserves_order_and_none_slots() {
    let host = make_host();
    upsert_ts(
        &host,
        "/src/types.ts",
        "export interface OProps { label: string }\n",
    );
    upsert_vue(
        &host,
        "/src/Ok.vue",
        r#"<script setup lang="ts">
import type { OProps } from './types'
defineProps<OProps>()
</script>
<template><div /></template>"#,
    );
    upsert_ts(&host, "/src/plain.ts", "export const x = 1;\n");
    // Order: [Vue, plain-ts (no public API), missing].
    let refs = ["/src/Ok.vue", "/src/plain.ts", "/src/Missing.vue"];
    let batch = public_api_batch(&host, &refs);
    assert_eq!(batch.len(), 3, "one slot per input id, in order");
    assert!(batch[0].is_some(), "the Vue SFC renders a slot");
    assert!(
        batch[1].is_none(),
        "a plain .ts projects no public-API slot"
    );
    assert!(batch[2].is_none(), "a missing canonical projects no slot");
    // The rendered slot is byte-identical to the scalar render.
    let scalar = host
        .get_public_api("/src/Ok.vue")
        .expect("scalar public API projection")
        .expect("scalar renders");
    assert_eq!(
        norm_response(batch[0].as_ref().unwrap()),
        norm_response(&scalar),
        "the in-order Vue slot must be byte-identical to its scalar render",
    );
}
