# CCA1O4 scope disposition: the typed wire-schema extension (compiler.compiler-bridge)

- Status: proposed — awaiting operator/architect ratification
- Date: 2026-09-05
- Disposition: `ADOPT-NOW` for the native wire-schema extension and the
  crates beyond CCA1O4's named surfaces, recorded here because the review
  rounds found the expansion undispositioned, not because a fix owner may
  ratify their own rescope.
- Amends: nothing. No other charter's budget, no DAG edge, no other node's
  ledger row. CCA1O4's own charter gains a pointer section to this record.
- Asks for: one operator decision (see the last section).

## Why this record exists

CCA1O4's charter states "Production surfaces are `packages/unplugin/src/index.ts`
and `packages/unplugin/src/core/compiler.ts`" and excludes "Native/NAPI
signatures, benchmark/tools, TypeScript plugin, WASM, and any profile
deletion". The candidate changes ten production files across
`verter_compiler`, `verter_ffi`, `verter_protocol`, `verter_session`,
`@verter/unplugin` and `@verter/native`, and extends the typed host-request
wire schema with three new fields and two new wire enums. Budgets are
planning references, so the footprint alone is not the issue; the explicit
*exclusion* of native signature work is a scope boundary, and crossing it
requires this record.

## What crossed the boundary, and why the charter's own invariant compels it

The wire extension, all in `crates/verter_protocol/src/types.rs` with its
converter in `crates/verter_ffi/src/convert/input.rs` and the regenerated
byte-pinned mirror `packages/native/host-compile-request.generated.ts`:

- `FfiHostCompileIdentity.ssr_module_id: Option<String>`
- `FfiHostCompileIdentity.hmr_strategy: Option<FfiHmrStrategy>` (new enum)
- `FfiRuntimeProductRequest.style_processing: Option<FfiRuntimeStyleProcessing>`
  (new enum)

Without these three fields the typed route cannot satisfy the charter's own
invariant "Preserve Vue/Svelte bundling, HMR, manifest":

- `hmr_strategy` — the typed request carried no HMR strategy, so a migrated
  Vue carrier lost the `__file` / hot-accept trailer that the legacy
  profile-bearing route emits. Dropping HMR is not a migration.
- `ssr_module_id` — the SSR product's `ssrContext.modules` key derives from
  it; without the field the manifest entry a server build publishes changes
  key.
- `style_processing` — the Vite authored-only cascade (the plugin owns
  preprocessing under Vite's CSS pipeline; the compiler must NOT re-run the
  complete cascade over authored bytes) was inexpressible on the typed
  request. `RuntimeStyleProcessing::default() == Complete`, so an absent
  field keeps the pre-migration behavior and is not a silent flip.

The additions are disciplined: every field is `Option`-typed,
`deny_unknown_fields` is preserved, and the converter had already recorded
this exact addition as the anticipated "wire addition, not a silent default
flip". The generated TypeScript mirror was regenerated through its checked-in
generator, and the byte-pin guard
(`crates/verter_napi/tests/cases/host_compile_request_ts_freshness.rs`)
passes against it.

## The alternative considered

Carrying the wire extension as its own CCA1O2-family node and reverting it
here is the other option the review named. It is not separable in practice:
without the fields the migration fails the charter's preservation invariant,
so reverting them unlands the node's outcome rather than trimming it. The
honest split would sequence "extend the typed wire" BEFORE "migrate the
unplugin consumer"; the candidate instead landed both halves together, which
this record discloses.

## What is asked of the operator

Ratify ONE of:

- **(a) Accept the expansion as scoped here** (recommended). The three
  fields and two enums are the minimum the preservation invariant requires;
  each is additive, optional, and fail-closed. CCA1O4's charter already
  carries the pointer section naming this record.
- **(b) Reject.** Then the wire extension reverts, the unplugin migration
  cannot meet its own acceptance, and CCA1O4 needs a rescope that sequences
  a wire-extension node ahead of it.

Until the ruling, this record stands as the disclosure; it is not itself an
approval.

## Complete inventory of the rule- and evidence-bearing changes

An earlier inventory for this candidate classified three files and reached
none of the governance-bearing paths under `roadmap/`, which left the
operator without a whole-candidate view. The complete set of rule-bearing or
evidence-bearing paths this candidate touches, over its whole base-to-head
range, is enumerated here so the ruling above can be taken on the whole
footprint rather than on the wire extension alone.

Nothing in this set softens, removes, or exempts a rule. The two rule TEXTS
the candidate touches — the charter section and this record — only ADD
disclosure.

1. `authority/state/implemented.toml` — the trusted ledger. Two deltas.
   (a) This node's own predeclared `"CCA1O4"` line transitions
   `pending` → `implemented` with its commit message and date: the mandated
   transition, and the only status change in the file. (b) The `"MEM0"` and
   `"MEM1"` rows move from between `"G4"` and `"G5"` to their sorted position
   after `"MDXR0"`. Both rows arrived on the base while this branch was open,
   at a non-canonical offset; `serializeLedger` in `tools/ledger.mjs` declares
   the file canonical as "nodes sorted by id", and the merge resolution
   restored that order. Their payloads are byte-identical
   `{ status = "pending" }`, so no other node's implementation fact changes.
2. `charters/compiler-compiler-bridge/CCA1O4.md` — this node's own charter.
   Appends a "Recorded deviation" section pointing at this record. It deletes
   and rewords no normative clause; the "Native/NAPI signatures … are
   excluded" exclusion stands verbatim, and the appended text says so
   explicitly.
3. `decisions/2026-09-05-cca1o4-native-wire-extension-scope-disposition.md` —
   this record, new. It amends no charter budget, no DAG edge, and no other
   node's ledger row; its status is `proposed`.
4. `closure/typescript-mapper/register.toml` and
   `closure/typescript-mapper/closure.md` — the typescript-mapper closure
   instrument's recorded `P-targeted-domain` proof and its
   `CTL-targeted-selector` control. Five counts move (selected 9504 → 9510,
   executed and passed 8957 → 8963, skipped unchanged at 547) and the
   transcribed terminal summary is re-transcribed. Cause: this candidate and
   the base each added `#[test]` cases inside the three packages that proof's
   command selects, so the previously recorded counts no longer describe the
   tree. This is the instrument's freshness obligation, not an evidence
   relaxation — the comparison grammar, the skip basis, the declared 547
   expected skips and the control's refusal semantics are unchanged, and
   `node roadmap/0.1.0-tama/tools/closure-register.mjs --check` PASSES at the
   candidate head. An intermediate commit on this branch briefly replaced the
   transcribed duration with a "not transcribed" allowance; the allowance was
   dropped and the literal summary re-transcribed before this head, so the
   field is verbatim again.
5. `tools/closure-register.pins.mjs` — one pin digest,
   `control:CTL-targeted-selector.observed`, recomputed because entry 4
   rewrote the sentence it pins. No executable pin machinery changes.
6. `docs/api/unplugin.md` (outside `roadmap/`) — public API documentation,
   not a rule: it documents the optional `sass` peer dependency the non-Vite
   preprocessing path needs.

Explicit account of what is NOT touched, so the exclusions are stated rather
than inferred from silence:

- No `CLAUDE.md`, no `AGENTS.md`, no skill under `.claude/`, no architecture
  document, and no protocol document.
- No guard, assertion, exemption list, allowlist, or portability-marker
  manifest entry is added, relaxed, or removed.
- No DAG module, catalog, schema, train register, or other node's charter.
- The ignore/skip inventory is unchanged: the 547 expected skips the proof
  declares are the same 547 the base declares.

## Verification beyond the node's declared gate

CCA1O4's declared `targeted-domain` gate selects `verter_type_runtime`,
`verter_session` and `verter_protocol`. This candidate also changes
`verter_compiler`, `verter_ffi` and `verter_napi` — the latter two owning the
wire-conversion contract and the byte-pin freshness guard that is the only
proof the regenerated TypeScript mirror still matches the Rust schema this
candidate extends. A self-declared universe that excludes a changed surface
is not a pass, so those three packages were executed as well.

Developer-host receipts accompanying this candidate (a Windows x64
workstation; the whole-workspace lane in CI remains the merge-authoritative
one):

- `cargo nextest run --locked -p verter_compiler -p verter_ffi -p verter_napi`
  — 7350 run, 7350 passed, 6 skipped.
- The `@verter/unplugin` unit suite plus its bundler exit-regression harness —
  238 passed, 5 skipped across 12 files, and the SCSS virtual-style and Vite
  lib-build repros PASS.
- `tsc --noEmit` over the package’s own `tsconfig.json`: the style-preprocessing
  failure path reports no error there. The remaining errors in that projection
  are the base tree’s (an `ES2020` target against `String.replaceAll`, and the
  unplugin context typings) and no gate consumes it.
