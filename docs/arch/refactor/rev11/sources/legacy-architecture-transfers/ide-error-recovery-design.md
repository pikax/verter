# IDE Error Recovery — Diagnostics Parity For Broken `.vue`/`.svelte` Carriers

> Status: DESIGN (uncommitted draft for CTO/user sign-off). Read-only diagnostic + design block — no production code changed.
> Architecture authority: this design carries a NEUTRAL un-primed codex-architect verdict (full A–F verdict reproduced in §6). It triangulates three independent sources: a verified Verter code trace, Volar/TypeScript prior art (verified from source), and the codex verdict.
> Owning skills to update on landing: `/compiler-codegen` (IDE Script Error Recovery section), `/host-session` (diagnostic publish path), `/position-encoding` (mapping-drop rule). Promotes the "Carrier IDE TS Surface Principle" toward its diagnostics bar.

## 1. Problem statement (user-reported)

When a `.vue` `<script setup>` (or a template expression) becomes SYNTACTICALLY BROKEN mid-edit, the user perceives that ALL diagnostics vanish. Canonical repro (`templateRef.vue`, observation-only):

```vue
<script setup lang="ts">
import { useTemplateRef } from "vue";
const modelEl = useTemplateRef('modelEl')
let foo = '';
const bar: number = "x";   // pre-existing TYPE error (TS2322)
modelEl.                    // user types this — INCOMPLETE member access = syntax error
</script>
<template>
    <div ref="modelEl">{{ foo }}</div>
</template>
```

A plain `.ts` file behaves correctly under the same break: TypeScript's parser RECOVERS (best-effort AST + a zero-width error node), so type-checking CONTINUES; the editor shows the UNION of (the new "Identifier expected" syntax error) + (the surviving type diagnostics). The user's explicit requirement: **a broken `.vue` script/template must behave like a broken `.ts` — proper error recovery, never an empty/blank diagnostic set — and the fix must be a real error-recovery mechanism, NOT special-cased edge handling in the resolver.**

Expected end-state: published diagnostics = UNION of (syntax error(s)) + (surviving type diagnostics), all mapped to the carrier source, for both Vue and Svelte over the shared LSP path.

## 2. Empirical ground truth (verified, including a REAL tsserver run)

Established by instrumented experiments (throwaway harness in a disposable worktree) plus a REAL `tsserver` end-to-end run against the `single-project` fixture.

**G1 — The compiler is NOT the failure.** Verter ALREADY recovers a broken `<script setup>`. `compile()` (`CompileTarget::IDE`) SUCCEEDS on the broken input and emits OXC-VALID TSX:
- the dangling `modelEl.` gets a member-hole placeholder → `modelEl.valueOf`;
- open brackets get scope closers; the `___VERTER___TemplateBindingFN()` wrapper closes;
- a deliberate type error on a clean line (`const bar: number = "x"`) SURVIVES verbatim in the emitted TSX.

Recovery lives in `crates/verter_compiler/src/ide/script_recover.rs` (`ScriptTokenScanner::recover_plan` → `ScriptSetupRecoveryPlan`) and fires from `crates/verter_compiler/src/ide/script/setup.rs`. It emits OUTPUT-ONLY UNMAPPED `CodeTransform` inserts (`MemberHole` → `out.prepend_static(at, "valueOf")`, `ExpressionHole` → `(undefined)`, scope closers appended at the wrapper end).

**G2 — The source map is robust to mid-file unmapped inserts.** `Chunk::Inserted` emits a single UNMAPPED token (`source_id = None`) advancing only the GENERATED cursor; `Chunk::Original` always emits its TRUE source coordinates (never recomputed from generation progress), and a run's source extent is clamped to the source line's true content length (`crates/verter_compiler/src/code_transform/source_map.rs:163-259`; `crates/verter_lsp/src/documents/position_map.rs:296-327`). Consequence: a mid-file insert shifts only GENERATED positions — it does NOT corrupt the mapping of clean content before OR after it.

**G3 — REAL tsserver delta (decisive).** Clean `.vue` with `const bar: number="x"` + `const ok = modelEl.value`; then append `modelEl.`:
- BEFORE: 2 provider diagnostics, BOTH map back to the carrier.
- AFTER: 3 raw provider diagnostics — 1 MAPPED, 2 DROPPED.
  - `TS2322` on `bar` (clean line) → STILL MAPS, identical carrier range. **Clean-line diagnostics survive.**
  - `TS6133` "'modelEl' declared but never read" → DROPPED. The provider anchored it inside the synthetic `___VERTER___unwrapped` / destructure region (offset ~1487), which is unmapped.
  - `TS6133` "'ok' declared but never read" → DROPPED (same synthetic region).
  - The dangling-member SYNTAX ERROR itself → reported by NOBODY.

So "ALL diagnostics lost" is imprecise: clean-line diagnostics DO survive. What the user perceives as "everything gone" in a small file is the compounding of: the missing syntax-error marker at the broken spot + the unused-symbol diagnostics being displaced into unmapped synthetic code and dropped.

## 3. Root cause — three compounding mechanisms

### M1 (PRIMARY) — No native syntax diagnostic; `has_errors` is wrongly coupled to "can build IDE surface"

`crates/verter_session/src/parse.rs` `build_vue_script_outputs` (≈ lines 1326-1345) OXC-parses the `.vue` script but harvests ONLY `catch_analysis_panic` PANICS into diagnostics. It NEVER converts OXC's recoverable `parse_result.errors` (the real syntax errors) into native `HostDiagnostic`s:

```rust
let Some(parse_result) = parse_result else { return outputs; };
if parse_result.panicked { return outputs; }
// parse_result.errors is NEVER read → no native syntax diagnostic
```

Therefore a broken `<script setup>` produces ZERO error-severity native diagnostics. Because no native error exists, the compile does NOT bail (the `if compile_diags.has_errors { return Err(...) }` gate in `crates/verter_session/src/host_resolve/virtual_file_pipeline.rs` stays false), recovery TSX is produced, and the post-recovery TSX is OXC-clean so the TS provider sees no syntax error. Net: **the broken spot gets no marker from anyone.** This is the dominant cause and the core TypeScript-parity gap. (TypeScript: `createMissingNode` in `src/compiler/parser.ts` inserts a zero-width error node + an "Identifier expected" diagnostic AND keeps checking.)

The deeper architectural defect: Verter conflates "a (recoverable) diagnostic exists" with "the IDE TSX surface cannot be built." These must be SEPARATE channels (a recoverable syntax error must publish a diagnostic AND keep the recovered TSX surface).

### M2 (SECONDARY) — Recovery displaces references / unused-copy liveness, into UNMAPPED synthetic code

Precise mechanism (reconciled against the code, refines the surface symptom):

- `crates/verter_compiler/src/ide/script/setup.rs:139` derives `script_complete = parser_ret.errors.is_empty()`. On a broken script this is `false`, so `all_sources_complete` is `false` (line ≈759-761), so `should_omit` is ALWAYS `false`. **The unused-binding OMISSION path already fails open correctly under recovery** — it does NOT relocate the source decl's TS6133.
- BUT the fail-open keep-everything path KEEPS all bindings: it emits a value-read `modelEl: modelEl as unknown as typeof modelEl` into `___VERTER___unwrapped` AND a destructure `const { modelEl, ok } = ___VERTER___unwrapped`. Under recovery, the user's body reads the SOURCE bindings (`modelEl.valueOf` reads the outer `modelEl`; `ok` is unreferenced), so the DESTRUCTURED COPIES (`const { modelEl, ok }`) become spuriously UNUSED. TS6133 then fires on the DESTRUCTURE BLOCK — an unmapped synthetic region — and is dropped by M3.
- Additionally, the `valueOf` member-hole REWRITE means the user's `modelEl` reference at the broken site is consumed into `modelEl.valueOf` (the user token is no longer a clean, standalone mapped read), so liveness/find-refs/hover semantics drift around the recovered region even beyond this specific TS6133.

So M2 is two-part: (a) the member-hole rewrite changes program semantics around the user token, and (b) the fail-open keep-everything destructure copies go unused under recovery and emit TS6133 into synthetic code.

### M3 (TERTIARY) — Strict mapping-drop silently discards diagnostics in synthetic regions

`crates/verter_lsp/src/type_provider/merge/diagnostics.rs:37-55` drops any provider diagnostic whose TSX range fails `tsx_range_to_carrier_range` (`crates/verter_lsp/src/type_provider/merge/position.rs:369-400` → `PositionMapper::tsx_range_to_carrier`, strict endpoint-compatibility: a range straddling synthetic content is dropped). This is CORRECT for genuinely-synthetic regions; here it silently swallows the M2-displaced diagnostics (`dropped += 1`, `tracing::debug` only).

### Separate: HARD-FAIL template path

A broken `<template>` (not script) makes `compile()` return `Err`; the LSP `get_ide()` returns `None`, and the publish set collapses to the (empty) native-only set (`crates/verter_lsp/src/sync_coordinator.rs:643-701`, the `else { verter_diags }` / early `publish_diagnostics(uri, verter_diags)` branches). Combined with M1 (empty native set), this clears all squiggles. Different code path from the broken-script case; same architectural defect (recoverable breakage treated as "no virtual file").

### Minor: degraded typing under recovery

Under recovery, `useTemplateRef('x')` loses its synthesized `<ReturnType<typeof ___VERTER___Comp{n}>, "x">` type arguments (degraded typing, not invalidity). Tracked as a follow-up, not core to the diagnostics-loss bug.

## 4. Prior art — Volar + TypeScript (verified from source)

- **Volar = PASS-THROUGH-AND-LET-TS-RECOVER, not recover-and-patch.** `@vue/language-core` emits the user's `<script setup>` body and template expressions VERBATIM as source-mapped chunks (`generateSfcBlockSection` / `generateCodeWithTransforms`); it transforms only at known macro seams. User identifiers stay REAL MAPPED references at their original offsets, so TS liveness / find-refs / rename stay accurate. Template bindings emit `__VLS_ctx.<original-token>` (original token, original offset) so references link template↔script.
- **Two diagnostic rails.** (1) NATIVE Vue parse diagnostics from `@vue/compiler-dom` errors, published directly with `source: 'vue'` (`vue-compiler-dom-errors` plugin), independent of TS. (2) TS-on-virtual-file diagnostics mapped back. Structural/template break → native rail; script type/syntax break → TS rail.
- **Boundaries, not rewrites.** Volar issue #3632 / PR #4692: a trailing syntax error was masked when the generated wrapper line fused with the user's incomplete statement into something valid. Fix: inspect `block.ast.parseDiagnostics` and, if a parse error sits at the region's trailing edge, append a synthetic `;` + a zero-width `verification` marker. It does NOT rewrite the user's broken tokens.
- **Per-chunk capability flags + targeted suppression.** Each chunk carries `verification` / `completion` / `semantic` / `navigation` flags; specific TS codes can be suppressed on specific synthetic chunks (e.g. `doNotReportTs6133` for unused-variable). Template codegen is gated `if (template.ast)` — recover per-node, never bail the whole template.
- **TypeScript parser contract** (`microsoft/TypeScript` `src/compiler/parser.ts`): `createMissingNode` inserts a ZERO-WIDTH empty identifier + "Identifier expected", `finishNode` marks `NodeFlags.ThisNodeHasError`, KEEPS the node in the tree; the checker runs over the best-effort tree. TS does not rewrite user tokens.

Verter's current recovery is the OPPOSITE of Volar's model: it aggressively REWRITES broken tokens (`modelEl.` → `modelEl.valueOf`) to make OXC happy, which (per M2) changes liveness and displaces diagnostics. The durable direction is Volar's hybrid: preserve user tokens verbatim, add minimal synthetic structure (boundaries / clearly-synthetic repair chunks), add a native syntax rail, and let the real TS provider recover.

## 5. Design — two-rail diagnostics + reference-preserving recovery

The correct, durable design (lowest-owner-crate placement per Verter's shared-codebase rule):

### 5.1 Native syntax-diagnostic rail (owner: `verter_session`) — addresses M1

- In `crates/verter_session/src/parse.rs`, harvest OXC `parse_result.errors` (the recoverable syntax errors) and convert them into native carrier `HostDiagnostic`s. OXC spans are over the EXTRACTED script content; map them to `.vue` carrier offsets by adding the script block's `content_start` (the same offset baked into recovery facts). Store them as native diagnostics independent of provider diagnostics, so EVERY consumer (LSP, MCP, component-meta) gets them — not LSP-only, not provider-dependent.
- The same rail covers Svelte: Svelte parser/compiler syntax diagnostics become native carrier diagnostics in `verter_session` (the Svelte projector is a separate codegen path but shares the host diagnostic store).
- For the template, harvest the template/SFC compiler's recoverable parse errors into the native rail as well (mirrors Volar's `source: 'vue'` rail).

### 5.2 Decouple "recoverable diagnostic exists" from "IDE TSX cannot be built" (owner: `verter_session`) — addresses M1 + hard-fail

- Split the diagnostic channel from the surface-production decision. A RECOVERABLE syntax error sets recovery/`script_complete` metadata and publishes a native diagnostic, but MUST NOT make `compile()`/the IDE pipeline bail or drop the recovered TSX. Only a genuinely catastrophic failure (no recoverable surface at all) returns `Err`.
- The published diagnostic set is then the UNION: native syntax diagnostics ∪ mapped provider diagnostics (over the recovered TSX).

### 5.3 Reference-preserving recovery + fail-open usage analysis (owner: `verter_compiler`, with `verter_session` carrying recovery flags) — addresses M2

Two changes, BOTH required (per the architect; iii alone is insufficient):

1. **Reference-preserving member/expression recovery.** Reduce the aggressive rewrite toward Volar's verbatim+boundary model: the user's token (`modelEl`) must remain a real MAPPED read at its original span; ONLY the synthetic repair material (the `.valueOf` placeholder, or a boundary terminator) may be unmapped, and it should be clearly marked synthetic (compiler-owned). This keeps liveness / navigation / references stable around the recovered region.
2. **Fail-open usage analysis when recovery participated.** Recovery mode is incomplete knowledge → under Verter's existing "unknown ⇒ used" invariant it must fail open. `script_complete = false` already disables OMISSION. The NEW requirement is the destructure-copy liveness (the precise M2(b)): under recovery the kept destructure copies (`const { modelEl, ok } = ___VERTER___unwrapped`) go unused and emit TS6133 into synthetic code. Resolve by ONE of (decide at implementation, preference toward the capability-flag option to match architect-D):
   - (a) extend the existing `void(name)` keep-alive (already used for script/style-used bindings) to cover ALL kept bindings whenever recovery participated, so the destructure copies are always read; OR
   - (b) mark the `___VERTER___unwrapped` / destructure chunks as synthetic and intentionally suppress TS6133 on them (Volar `doNotReportTs6133` analogue; needs a per-chunk suppression mechanism — see 5.4); OR
   - (c) under recovery, emit the value-reads so the destructured locals (not the source bindings) are read.

   Option (a) is the smallest change and stays inside `verter_compiler` (no `verter_lsp` plumbing); option (b) is the most Volar-faithful and aligns with architect-D's "mark synthetic chunks for intentional suppression" but needs chunk-capability plumbing. The implementer should evaluate (a) first as the minimal correct fix and escalate to (b) only if (a) proves unsound.

### 5.4 Keep mapping-drop STRICT (owner: `verter_lsp`) — addresses M3

- Do NOT add heuristic re-anchoring of synthetic-helper diagnostics back to user symbols (it invents provenance, risks wrong locations, duplicate diagnostics, misleading quick-fix/navigation). Keep `tsx_range_to_carrier_range` strict.
- The correct fix is UPSTREAM (5.3: don't displace user-symbol diagnostics into synthetic code). If a per-chunk suppression mechanism is added (5.3 option b), that is an INTENTIONAL suppression by chunk metadata, NOT a re-anchor. A future narrow exception (re-anchoring) is allowed only via an explicit chunk-metadata source-provenance contract, never heuristics in LSP merge code.

### 5.5 Best-effort template recovery (owner: `verter_compiler`, surfaced via `verter_session`) — addresses hard-fail template path

- The IDE TEMPLATE codegen should become error-recovering: produce best-effort JSX from the PARTIAL template AST (recover per-node / per-expression, consistent with the script path and Volar's `if (template.ast)` gate), so `compile()` returns Ok with a recovered TSX surface instead of `get_ide() → None`. The native rail (5.1) reports the template SYNTAX error; the recovered surface preserves the surviving script/template-expression provider diagnostics.
- Last-good TSX is acceptable only as a temporary fallback for catastrophic compiler failure — NOT as the main design (it is stale-state: diagnostics/completions can describe code the user no longer has).

### 5.6 Source de-duplication (owner: `verter_lsp` publish/merge) — the architect's counter-argument

Once recovery is reduced and TS sees more broken script verbatim, the native rail AND the provider rail may both report the SAME user syntax failure. De-duplicate by span/code/message at publish/merge time (classify by source). The native rail remains necessary for SFC/template syntax, provider-masked syntax, and compile-recovery cases.

## 6. Architect verdict (NEUTRAL, un-primed) — reproduced

Full verdict at `.feedback/_recov_architect2.out` (round 2, decisive, `__DONE__`). Summary:

- **A:** YES — add a native syntax-diagnostic rail in `verter_session/parse.rs` (harvest OXC `parse_result.errors` → carrier offsets). Decouple `has_errors` from "can build IDE surface": a recoverable syntax error sets `script_complete=false` but must NOT bail compile. Publish = native syntax ∪ mapped provider. Svelte needs the same rail.
- **B:** BOTH (iii) disable omission when recovery participated AND reference-preserving member recovery (keep `modelEl` a mapped user reference; only the synthetic `.valueOf` unmapped). iii alone is insufficient — it leaves rename/find-refs/hover/liveness fragile.
- **C:** Volar-style hybrid: preserve user tokens verbatim + boundary/zero-width placeholders only; reduce aggressive rewriting. Long-term target is NOT "rewrite broken syntax until TS is happy."
- **D:** Keep mapping-drop STRICT; NO heuristic re-anchoring. Fix upstream + mark synthetic chunks for intentional suppression. Future re-anchor exception only via explicit chunk-metadata provenance contract.
- **E:** Template path → best-effort IDE codegen + native template diagnostics; NOT empty-publish, NOT primarily last-good. The native rail reports the syntax error but does not by itself satisfy the IDE-surface requirement.
- **F:** `verter_session` change UNAVOIDABLE (needs sign-off). Stage S1/S2/S3 (below). 6 discriminating tests, real-provider mandatory.
- **Strongest counter-argument:** native + provider rails can duplicate syntax reports once recovery is reduced → classify sources and de-dup by span/code/message (5.6).

The round-1 consult (un-primed, exploratory; ran out of budget before a final verdict) independently reached the same direction: "separate diagnostic severity from can-still-build-a-recovery-surface"; "the compile path treats compile diagnostics as fatal for virtual-file production — wrong coupling"; "recovery should set an explicit 'script usage incomplete' bit and keep omission disabled whenever syntax recovery participated."

## 7. Scope, crates, decomposition

**`verter_session` is unavoidable** and per the user's standing rule requires explicit sign-off before edits (it owns parse, compile orchestration, native diagnostic storage, the project cache). `verter_compiler` must change for recovery/codegen behavior. `verter_lsp` stays mostly strict (merge tests/observability + de-dup; per-chunk suppression plumbing only if 5.3(b) is chosen).

Staged (the architect's ordering; each stage independently testable, lands as one clean conventional commit):

- **S1 — Native syntax rail + bail-decoupling.** Owner: `verter_session` (+ small `verter_lsp` publish union/de-dup). Harvest Vue/Svelte script syntax errors into native carrier diagnostics; split recoverable-diagnostic from fatal-cannot-build-surface; broken script publishes the syntax error PLUS the surviving provider diagnostics. This is the highest-value stage and addresses M1 directly. **Discriminating-test prerequisite:** the `.vue` carrier-merge published path is not test-reachable today (`publish_full_diagnostics` is push-only and `pub(super)`); S1 must add a thin `#[cfg(test)] pub(crate)` accessor (refactor `publish_full_diagnostics` → a shared `compute_full_diagnostics(uri) -> Vec<Diagnostic>` used by both the publisher and the test).
- **S2 — Recovery metadata + reference-preserving recovery + destructure-copy fail-open.** Owner: `verter_compiler` (+ `verter_session` recovery flags). Reference-preserving member/expression recovery (user token stays mapped); resolve the M2(b) unused-destructure-copy (5.3 option a preferred, else b).
- **S3 — Template best-effort recovery.** Owner: `verter_compiler`, surfaced via `verter_session`. Broken templates produce native template diagnostics AND a recovered IDE TSX surface instead of `get_ide() → None`.

## 8. Discriminating tests (TDD — RED before, GREEN after; real-provider MANDATORY)

All carrier-merge parity tests use the real-provider harness (`crate::test_harness`, `real_provider_test!` macro → both `_tsserver` and `_tsgo` variants; both providers ARE runnable in this checkout — `single-project` fixture has `node_modules/vue`, plus `tsserver.js` and `tsgo.exe`). Use `single-project` fixture (`packages/vue-vscode/e2e/fixtures/single-project`, already contains `BrokenTemplateExpr.vue`). Assertion idiom: `real_provider_tests/diagnostics.rs` `diagnostics_until_nonempty` + range/code/message asserts; clean↔broken via two `open_virtual` opens (or `did_change`).

1. **Broken `<script setup>` keeps type diagnostics AND reports the syntax error.** Open clean `.vue` (assert TS2322 on `bar` present). Append `modelEl.`. Assert the published set contains BOTH a syntax diagnostic at the `modelEl.` site AND the TS2322 on `bar`. (RED today: syntax diagnostic absent.)
2. **`bar` carrier range unchanged** between clean and broken (proves G2 robustness and no global shift).
3. **No user-symbol diagnostic relocates into unmapped synthetic regions** under recovery — assert no `TS6133` is silently dropped for a binding the user genuinely uses; assert kept-binding destructure copies do not emit a dropped TS6133. (Targets M2(b).)
4. **Clean-file negative guard:** clean `.vue` output is byte-stable / snapshot-stable where recovery is inactive (no behavior change in the common case).
5. **Broken `<template>` keeps diagnostics:** broken template publishes a native template syntax diagnostic AND retains the surviving script/provider diagnostics (not an empty set). (Targets the hard-fail path + S3.)
6. **Both providers + real-provider E2E mandatory** — mocked diagnostics are insufficient for these parity facts (whether a binding "counts as used", where TS pins a diagnostic, whether a diagnostic surfaces are runtime facts only the real provider reveals).

Plus architecture-guard considerations: any new `(CRITICAL)`-bearing recovery rule needs a guard or discriminating regression test in the same change. The existing `ide_script_recovery_guard.rs` (no truncate-and-reparse / no synthesize-then-reparse) must continue to hold; reference-preserving recovery must not reintroduce a reparse.

## 9. Constraints honored

- **CodeTransform-as-SSOT:** all recovery output stays `CodeTransform` ops (the reference-preserving member hole is a mapped chunk for the user token + an unmapped chunk for the synthetic placeholder; boundary terminators are unmapped inserts). No post-hoc string edits.
- **Two-codegen-paths:** only the IDE path (`CompileTarget::IDE`/`TSX`) is touched; VDOM/Vapor untouched.
- **Typed-IR / no-reparse:** recovery facts come from the original clean OXC AST or original-span token recovery; no synthesize-then-reparse. The native syntax rail reads OXC's own `parse_result.errors` (no string heuristics).
- **Shared-codebase / lowest-owner:** the native rail lands in `verter_session` so LSP, MCP, and component-meta all benefit.
- **Cross-platform & hermeticity:** tests use vendored fixtures only (`single-project`); carrier-offset mapping uses span arithmetic (no path assumptions).

## 10. Open items for sign-off

- **`verter_session` edit sign-off** (S1, S2-flags, S3 surface) — required by the user's standing rule.
- **5.3 option (a) vs (b)** for the destructure-copy fix — recommend (a) first (minimal, compiler-local); escalate to (b) (chunk-capability suppression) only if (a) is unsound. Either way, NOT a re-anchor (architect-D).
- The `useTemplateRef` type-arg loss under recovery (§3 minor) — fold into S2 or track as a separate follow-up.
