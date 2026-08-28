# C1 eleventh deviation — F12: phase-4 observation wiring and phase-7 module resolution form one cutover SCC

Found while assessing whether the shared blocker identified this round
(`normalized_analysis_canonical` → `resolve_for_persistent_state` →
`verter_workspace`'s resolution engine, now confirmed to block 8 of
`ResolverObservation`'s remaining methods across two method groups)
justifies pulling phase 7 (`ProjectResolver` → `ModuleResolverCore`,
scoping-spec.md §4 step 7) forward. Dispositioned via a fresh Codex xhigh
consult. Full consult prompt/output: `/tmp/c1-deviation6-consult-prompt.md`
/ `/tmp/c1-deviation6-consult-output.md` (not committed — ephemeral
scratch; this file is the durable record).

## Finding

Traced the full chain precisely this round:
`ResolverContext::normalized_analysis_canonical`'s slow path
(`host_manage/eval_env.rs:780-801`) → `resolve_for_persistent_state`
(`host_lifecycle.rs:270-297`) → `self.ws().resolve_import_outcome(...)`,
whose sibling `resolve_import_outcome_with_evidence`'s own doc comment
(`verter_workspace/src/engine.rs:2658`) reads: "Sealed outer boundary
around exact lookup, selection, cache lookup, resolver observations,
provider projection, and completion admission." This is not a narrow
helper — it IS `verter_workspace`'s resolution engine, the exact machinery
`resolve_import_outcome_in_published` (`engine.rs:3088`) drives: capturing
a transaction, refreshing evidence, checking retained candidates, invoking
the resolver, validating under a write fence, and publishing a decision
node — the SAME system the scoping-spec's F4 correction already targets
for the 2,122-line `ProjectResolver` at phase 7.

One deferral (declining to design `normalized_analysis_canonical`'s
`AttemptOutcome` shape independently of phase 7, round 3's original call)
now blocks 8 of `ResolverObservation`'s remaining methods across two
groups: `shallow_file_state`'s 5 non-decl-body accessors (Part E) and all
3 `prepared_decl_bundle`-family methods (both routes reach
`ensure_indexed_ready_serve`, which normalizes via this same path before
even attempting its cache fast path).

## Disposition: sequencing correction ADOPT-NOW; phase-7 IMPLEMENTATION DEFERRED

**Verdict: DEFER the phase-7 implementation in this session, but ADOPT-NOW
the finding as a sequencing correction.** Phases 4-6 are no longer fully
closable before phase 7 as the scoping-spec's literal order assumes — the
eight blocked observation methods and `ModuleResolverCore` now form ONE
cutover SCC. This is a sequencing deviation, not an architecture reopen:
the missing observations are finite and already fit the intended
`AttemptOutcome`/`InputKey`/`LoadSet` taxonomy — what's missing is not
vocabulary design, it's converting the resolution ALGORITHM and placing
the attempt/publication boundary correctly.

**Explicit instruction: continue every genuinely independent
disposition-table row now.** Do NOT implement either workaround examined
and REJECTED below.

### Two workarounds explicitly REJECTED

1. **A narrow `AttemptOutcome<ResolutionPublication>` wrapper around
   `resolve_import_outcome`** — NOT legitimate. Wrapping a blocking/
   live-read operation AFTER it executes does not make it an I/O-free
   attempt; it just hides the block behind a type. `ResolutionOutcome`
   (carries `SignatureAdmission`) and `ResolutionPublication` (carries an
   Engine-minted `ReadSetSignature`, `resolution_currency.rs:1724`) are
   DELIBERATELY not dependency-neutral — they're transaction-admission
   envelopes, a different owner than `AttemptOutcome<T>` (verter_semantic)
   entirely.
2. **"Runtime JS canonicals always return `NeedInputs`" as a permanent,
   documented gap** — REJECTED, disqualifying, not merely undesirable:
   - After the same JS input is requested once, the next retry has an
     empty delta and terminates as `InputResolutionNoProgress` (contract
     §4 step 5) — the caller can NEVER make progress for this file class.
   - A fully preloaded I/O-free lifecycle could never produce the
     blocking lifecycle's `.d.ts`-companion answer — violates C1-AC-1
     (same query, same content, different lifecycle ⇒ same answer) and
     the input-loading contract's final-result-equivalence test (§9).
   - Functionally an always-unknown stub for one entire supported file
     class — reading the raw `.js` artifact without normalization is
     actively WRONG whenever an admitted `.d.ts` companion exists, not
     merely incomplete.
   - `ensure_indexed_ready_serve` normalizes BEFORE even attempting its
     cache fast path (`prepared_decl.rs:1899`) — the slow path is
     semantically load-bearing, not an edge case.

### The three envelope owners (for whoever does this cutover)

| Envelope | Meaning | Owner |
|---|---|---|
| `AttemptOutcome<T>` | Can the pure kernel answer from this immutable input view? | `verter_semantic` |
| `ResolutionOutcome` | Result plus transaction admission/cacheability trace | `verter_workspace` |
| `ResolutionPublication<T>` | Engine-minted durable-sink admission or refusal | `verter_workspace` |

Correct seam:

```text
ModuleResolverCore::attempt(request, immutable probe inputs)
    -> AttemptOutcome<Option<ResolveResult>>

workspace Engine adapter:
    capture transaction
    -> invoke attempt
    -> on NeedInputs, load/commit/retry
    -> on Complete, validate/admit
    -> mint ResolutionOutcome / ResolutionPublication
```

### Minimum legitimate phase-7 slice (for the future cutover, NOT this session)

The whole Engine does not move, but the whole `ProjectResolver` ALGORITHM
must converge — a normalization-only duplicate would violate the
single-resolution-authority rule and C1-AC-9:

- Make resolver request/result/context DTOs semantic-owned.
- Convert the shared resolution algorithm to immutable observations plus
  `AttemptOutcome`.
- Keep transaction capture, evidence refresh, currency, caching, and
  publication in `verter_workspace`.
- Ensure `resolve_with_reader`, explicit-project resolution, and
  `preferred_specifier` all round-trip through that SAME core — no
  residual `WorkspaceRead`-driven resolver left standing anywhere.
- Reverse the Cargo edge atomically. `verter_workspace` cannot consume
  `verter_semantic`'s `AttemptOutcome` today because `verter_semantic`
  still depends on `verter_workspace` (`verter_semantic/Cargo.toml:28`) —
  this is F4's already-known edge reversal, confirmed still pending.

### Deferral record

- **Durable owner**: C1's own dedicated phase-4/7 resolution cutover
  (not punted to a later architecture block — this stays C1-owned and
  must close before C1 closes).
- **Resolution gate**: before scoping-spec step 8 (Cargo-edge deletion),
  no later than C1 plan close.
- **Acceptance gates**: C1-AC-1 (lifecycle answer equivalence), C1-AC-5
  (full `AttemptOutcome` coverage — the 8 open methods), C1-AC-9 (no
  direct scheduler/tsgo I/O left in the relocated `ModuleResolverCore`).
- **Required tests** (named now so the eventual cutover has a concrete
  target, not invented later): runtime JS with/without a `.d.ts`
  companion; candidate precedence; source-sibling vs. declaration
  behavior; `node_modules` package-follow evidence; admitted-miss vs.
  refusal; I/O-free `NeedInputs → loaded retry → Complete` answer equality
  against the existing blocking lifecycle.
- **Evidence that would justify STARTING that cutover** (a checklist for
  whoever picks it up): (1) the remaining `verter_semantic ->
  verter_workspace` references are ready to disappear atomically; (2)
  every direct resolver read is inventoried as pure computation or
  `PathProbe`/`RealPath`/`PackageManifest`; (3) the workspace driver can
  retain the existing transaction/admission fences around `Complete`; (4)
  all resolver entry points demonstrably share the converted core; (5) the
  JS-companion characterization suite (the "required tests" list above)
  is in place BEFORE the cutover starts, not after.

## What this means for the rest of phase 4

The 8 blocked methods (`export_target`/`import_target_in`/
`has_type_symbol_in`/`export_assignment_target`/`visible_value_binding`/
`prepared_decl_bundle`/`prepared_type_decl`/`prepared_value_decl`) stay
explicitly OPEN in the disposition table — not implemented, not worked
around, clearly marked as waiting on the C1 phase-4/7 cutover. Every OTHER
still-open disposition-table row (the output-disposition bucket,
`active_session_view`, `semantic_query_memo`'s per-site audit, the
framework-surface split's implementation, `host_for_fact_tracer_install`'s
remaining sites, F11's three deferred methods) remains genuinely
independent of this blocker and should continue.
