//! PROVISIONAL `CarrierBatch` MEASUREMENT + DECISION (§2.1 / §2.7 of the
//! external-TS-engine architecture).
//!
//! The architecture contemplates ONE new carrier role, `CarrierBatch`: a
//! minimal-diagnostic surface for the cold TSC batch run that OMITS the
//! interactive-only IDE embellishments (inlay/semantic-token scaffolding,
//! hover-only helper regions) that have NO type-checking role, while preserving
//! identical type-checking semantics. It is kept as a DISTINCT storage slot ONLY
//! IF the measurement shows a MATERIAL cold-perf gain over sharing the single
//! `CarrierIde` surface for the batch; otherwise it is MERGED into `CarrierIde`.
//! The gate `carrier_batch_typechecks_same_as_ide` must hold either way (same
//! DIAGNOSTIC set on the leaf surface).
//!
//! ## The honest measurement and its DECISION: MERGE into `CarrierIde`
//!
//! There is exactly ONE IDE codegen path (`CompileTarget::IDE`). The only
//! per-file knobs that change the generated TSX are three flags:
//!   - `strict_slots` — emits `strictRenderSlot(...)`, a TYPE CHECK (typed slot
//!     children → diagnostics);
//!   - `conditional_root_narrowing` — emits root-narrowing guards, also
//!     diagnostic-affecting;
//!   - `embed_ambient_types` — embeds a `declare module "@verter/types"` ambient
//!     block so imports resolve WITHOUT the real package; a RESOLUTION-CONTEXT
//!     toggle (a fixed shared preamble), NOT an interactive embellishment. With
//!     `@verter/types` present in `node_modules` the diagnostics are identical
//!     either way.
//!
//! Two facts decide it (architect-confirmed):
//! 1. **There is NO valid diagnostic-preserving minimal profile to measure a gain
//!    from today.** A legitimate `CarrierBatch` (§2.7) must KEEP `strict_slots` +
//!    `conditional_root_narrowing` (turning them off changes the diagnostic set,
//!    violating `carrier_batch_typechecks_same_as_ide`), and the only legitimate
//!    omissions — inlay/semantic-token/hover scaffolding — are NOT separately
//!    toggleable in the current codegen. So no distinct minimal-diagnostic
//!    surface exists.
//! 2. **Codegen wall-time is unchanged.** Toggling the flags leaves median
//!    codegen wall-time within noise; the only large output-size delta is the
//!    `embed_ambient_types` shared preamble, which is amortizable
//!    resolution-context, not per-file cold-codegen work. A byte-size threshold
//!    would MISTAKE that shared preamble for meaningful per-file carrier
//!    complexity, so the decision is NOT keyed to output size.
//!
//! Therefore: **MERGE `CarrierBatch` into `CarrierIde`.** The cold TSC path reads
//! `CarrierIde`; the `carrier_batch_typechecks_same_as_ide` gate holds trivially
//! (one surface); the `carrier_batch_role_stored_and_invalidated` guard is
//! vacuously satisfied (no distinct role). A later perf-corpus pass RE-RUNS the equivalence gate on
//! the real perf corpus and is the place to revisit a distinct `CarrierBatch`
//! once real diagnostic-preserving omission knobs and a cold-perf corpus exist.
//!
//! This test ENCODES the decision: it asserts (a) the diagnostic-preserving
//! surface is IDENTICAL across the resolution-context toggle (the equivalence the
//! merged-surface gate stands on), and (b) toggling the flags yields no codegen
//! wall-time saving (the no-material-cold-perf-gain finding the MERGE rests on).

use std::time::Instant;

use oxc_allocator::Allocator;
use verter_compiler::compile::{compile, CodegenOptions, CompileTarget, VerterCompileOptions};

/// A rich SFC: a `<script setup lang="ts">` with props/emits + a template that
/// exercises interpolations, `v-if`/`v-for`/`v-bind`/`v-on`/`v-model` and a slot
/// — every type-observable template-expression construct a batch carrier must
/// keep, plus the slot that drives `strict_slots`.
const RICH_SFC: &str = r#"<script setup lang="ts">
import { ref, computed } from 'vue'

interface Item { id: number; label: string }

const props = defineProps<{ items: Item[]; title: string; max?: number }>()
const emit = defineEmits<{ (e: 'pick', id: number): void; (e: 'close'): void }>()

const query = ref('')
const selected = ref<number | null>(null)
const visible = computed(() => props.items.filter(i => i.label.includes(query.value)))

function choose(id: number) {
  selected.value = id
  emit('pick', id)
}
</script>

<template>
  <section :data-title="title" :class="{ active: selected != null }">
    <h2>{{ title }} ({{ visible.length }}/{{ items.length }})</h2>
    <input v-model="query" :placeholder="`max ${max ?? 0}`" />
    <ul>
      <li
        v-for="item in visible"
        :key="item.id"
        @click="choose(item.id)"
        :class="{ chosen: item.id === selected }"
      >
        {{ item.label }}
        <slot name="row" :item="item" :index="item.id" />
      </li>
    </ul>
    <button v-if="selected != null" @click="emit('close')">Close</button>
  </section>
</template>
"#;

/// The shared leaf diagnostic profile: every DIAGNOSTIC-affecting embellishment
/// stays ON (`strict_slots`, `conditional_root_narrowing`). This is the surface
/// the cold batch run reads — `CarrierIde` itself, since `CarrierBatch` is merged.
fn diagnostic_profile(embed_ambient_types: bool) -> CodegenOptions {
    CodegenOptions {
        target: CompileTarget::IDE,
        filename: Some("Widget.vue".to_string()),
        // Diagnostic-affecting flags are KEPT — a CarrierBatch may NOT drop these
        // without changing the diagnostic set (carrier_batch_typechecks_same_as_ide).
        strict_slots: true,
        conditional_root_narrowing: true,
        // The ONLY axis a batch run legitimately differs on: the ambient-types
        // resolution-context preamble (batch resolves @verter/types from disk).
        embed_ambient_types,
        ..Default::default()
    }
}

fn verter_opts() -> VerterCompileOptions {
    VerterCompileOptions {
        source_map: true,
        ..Default::default()
    }
}

fn compile_tsx(options: &CodegenOptions) -> String {
    let allocator = Allocator::new();
    compile(RICH_SFC, options, &verter_opts(), &allocator)
        .tsx
        .expect("CompileTarget::IDE must produce a TSX block")
        .code
}

/// Median codegen wall-time (ms) over `runs` compilations under `options`.
fn median_codegen_ms(options: &CodegenOptions, runs: usize) -> f64 {
    let mut samples: Vec<f64> = Vec::with_capacity(runs);
    for _ in 0..runs {
        let allocator = Allocator::new();
        let start = Instant::now();
        let result = compile(RICH_SFC, options, &verter_opts(), &allocator);
        let elapsed = start.elapsed().as_secs_f64() * 1000.0;
        std::hint::black_box(&result.tsx);
        samples.push(elapsed);
    }
    samples.sort_by(|a, b| a.partial_cmp(b).unwrap());
    samples[samples.len() / 2]
}

/// The merged-surface equivalence the `carrier_batch_typechecks_same_as_ide` gate
/// stands on: the DIAGNOSTIC-bearing surface (every type check kept) is identical
/// regardless of the `embed_ambient_types` resolution-context toggle — so a batch
/// run that resolves `@verter/types` from disk type-checks the SAME script +
/// template as the interactive surface that embeds it. The only textual
/// difference is the ambient preamble, which carries no diagnostic of the SFC's
/// own code.
#[test]
fn diagnostic_surface_is_identical_across_ambient_resolution_toggle() {
    let embedded = compile_tsx(&diagnostic_profile(true));
    let from_disk = compile_tsx(&diagnostic_profile(false));

    // Both type-check the full script + template surface.
    for needle in [
        "items",
        "title",
        "choose",
        "visible",
        "query",
        "strictRenderSlot",
    ] {
        assert!(
            embedded.contains(needle),
            "embedded-ambient surface must type-check `{needle}`"
        );
        assert!(
            from_disk.contains(needle),
            "resolve-from-disk surface must type-check the SAME `{needle}` \
             (no diagnostic-observable construct dropped)"
        );
    }

    // The ONLY difference is the APPENDED ambient `@verter/types` block: the
    // embedded surface is the from-disk surface with that resolution-context block
    // added at the end (the SFC's own diagnostic-bearing code — the
    // `___VERTER___TemplateBindingFN` body — is byte-identical). So the from-disk
    // surface is a strict PREFIX of the embedded one, and the extra suffix contains
    // ONLY the ambient declarations (no diagnostic of the SFC's own code).
    assert!(
        embedded.starts_with(&from_disk),
        "the resolve-from-disk surface must be a byte-exact prefix of the \
         embed-ambient surface — the SFC's own diagnostic-bearing code is identical; \
         only the ambient resolution-context block is appended"
    );
    let appended = &embedded[from_disk.len()..];
    assert!(
        appended.contains("@verter/types") || appended.contains("declare module"),
        "the only extra content in the embed-ambient surface is the ambient \
         resolution-context block — embed_ambient_types is resolution context, not a \
         diagnostic difference; this is why CarrierBatch merges into CarrierIde and \
         the equivalence gate holds on the single surface"
    );
    // And the appended block carries NONE of the SFC's own template/script
    // diagnostics — it is purely the `@verter/types` ambient declarations.
    for sfc_specific in ["choose", "visible", "TemplateBindingFN", "data-title"] {
        assert!(
            !appended.contains(sfc_specific),
            "the appended ambient block must not contain the SFC-specific symbol \
             `{sfc_specific}` (it is resolution context only)"
        );
    }
}

/// THE measurement + DECISION (merge). Asserts the no-material-cold-perf-gain
/// finding on the CORRECT axis: toggling the only available knobs yields NO
/// codegen wall-time saving. (The decision is NOT keyed to output byte size: the
/// large size delta is the amortizable `embed_ambient_types` shared preamble, not
/// per-file cold-codegen work — keying on size would mistake a shared preamble for
/// per-file carrier complexity.)
#[test]
fn carrier_batch_measurement_decision_merge_into_ide() {
    const RUNS: usize = 64;

    // Codegen wall-time with the ambient preamble embedded vs resolved-from-disk —
    // the largest available textual difference. If even THIS does not save codegen
    // time, no distinct minimal profile (which would differ by LESS) could.
    let with_ambient_ms = median_codegen_ms(&diagnostic_profile(true), RUNS);
    let without_ambient_ms = median_codegen_ms(&diagnostic_profile(false), RUNS);

    let with_bytes = compile_tsx(&diagnostic_profile(true)).len();
    let without_bytes = compile_tsx(&diagnostic_profile(false)).len();

    eprintln!(
        "CarrierBatch measurement (kitchen-sink-class SFC, {RUNS} runs):\n  \
         embed_ambient_types=ON:  median {with_ambient_ms:.4} ms, {with_bytes} bytes\n  \
         embed_ambient_types=OFF: median {without_ambient_ms:.4} ms, {without_bytes} bytes\n  \
         DECISION: MERGE CarrierBatch into CarrierIde (codegen wall-time unchanged; \
         the byte delta is the amortizable @verter/types shared preamble, not \
         per-file cold-codegen work; no valid diagnostic-preserving minimal profile \
         exists today)."
    );

    // The no-material-cold-perf-gain finding, asserted on the CORRECT direction
    // AND scale-invariantly. The risk that would RE-OPEN the merge decision is the
    // minimal (OFF) profile becoming a genuine cold-codegen WIN — i.e. OFF
    // materially FASTER than ON. We bound that as a RATIO so it holds at the
    // sub-millisecond medians these compilations run at: the gate FAILS when ON
    // takes MORE THAN ~2x the OFF time (equivalently, OFF is a >2x speedup). The
    // tiny `+ 0.02 ms` floor only avoids div-by-zero-class flakiness when both
    // medians are near zero; it is NOT a slack that could swamp the ratio (a
    // multiplicative bound, unlike the earlier additive `- 0.5` which went
    // negative at sub-ms scale and could never trip). Today the medians sit near
    // 1.0x, so ON is nowhere near 2x OFF and the bound holds.
    //
    // NOTE: this gate covers only the LARGEST available textual difference
    // (embed_ambient_types) — and even removing it yields no codegen win. The
    // load-bearing justification for MERGE is the companion fact, not this timing
    // number: there is NO valid diagnostic-preserving minimal profile to measure a
    // gain from (strict_slots + conditional_root_narrowing must stay on, and the
    // only legitimate omissions — inlay/semantic-token/hover scaffolding — are not
    // separately toggleable). This assertion's job is narrow: catch a FUTURE change
    // that makes a minimal profile a genuine cold-codegen win, which would re-open
    // the decision.
    assert!(
        with_ambient_ms <= without_ambient_ms * 2.0 + 0.02,
        "MEASUREMENT: the minimal (embellishment-OFF) profile is NOT a material \
         cold-codegen win over the full (ON) profile — ON does not take >2x the OFF \
         time (ON={with_ambient_ms:.4} ms, OFF={without_ambient_ms:.4} ms) — combined \
         with the absence of any valid diagnostic-preserving minimal profile, the \
         honest decision is to MERGE CarrierBatch into CarrierIde. If this FAILS, the \
         minimal profile has become materially faster (a >2x cold-codegen win) and \
         the keep-distinct decision must be RE-OPENED (add a distinct CarrierBatch \
         slot + invalidation, re-run the equivalence gate on the perf corpus)."
    );
}
