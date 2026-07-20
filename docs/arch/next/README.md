# `docs/arch/next/` — post-merge program backlog

This directory holds the **forward-looking program plans** recovered and rehomed after the
release-clean merge. Nothing here is landed capability: these are the ordered backlog and
design authorities the project executes **after** this release train. Current shipped scope
is documented in [`../release-state.md`](../release-state.md), not here.

## Index

- **[`semantic-db-overhaul-unified-remaining-plan.md`](semantic-db-overhaul-unified-remaining-plan.md)**
  — the unified typeinfo-parity + cache-runtime/scheduler remaining-work backlog (16 blocks
  `U0`–`U15`; the orchestrator's critical-path phasing is P1→U2, P2→U6, P3→TAIL, P5→NAV). The
  stated **target is full TypeScript-checker-grade parity — an honest multi-person-year scope,
  NOT landed**; a green 362-row ledger proves wiring/coverage, not `tsc`/`tsgo` semantic parity.
- **[`01-gate-integrity-block.md`](01-gate-integrity-block.md)** — the ratified next block: make
  every gate **prove it executed** (tree-derived surface manifest, mutation controls, the
  `gate_contract_integrity` guard, GI-4 promotion). Its debt authority is
  [`../gate-integrity-ledger.md`](../gate-integrity-ledger.md).
- **[`04-open-decisions.md`](04-open-decisions.md)** — the three user rulings owed before the
  gate-integrity block lands (GI-14 attestation-vs-proof, GI-15 red-baseline-vs-green-gate,
  GI-16 ratify-or-re-run the rules-commit governance gate).
- **[`cache-admission-closure-design.md`](cache-admission-closure-design.md)** — implementer-ready
  design for closing the shared-cache admission poison **class** (one instance is closed; the
  class is open) via an unforgeable `CacheabilityProbe`; mandate is **audit, not patch**.

Supporting backlog docs in this directory:

- **[`deferred-cleanup-debt.md`](deferred-cleanup-debt.md)** — cleanup debt deferred from the
  release-clean review (C3: deprecated `verter_workspace` graph re-exports).
- **[`lsp-pending.md`](lsp-pending.md)** — the LSP-branch-owned items the release-clean full-green
  gate still awaits.
- **[`vue-inline-template-runtime.md`](vue-inline-template-runtime.md)** — Verter's SFC→JS runtime
  does not emit the official `inlineTemplate: true` production topology (setup-returned render
  closure); behaviorally equivalent, production-parity + perf feature deferred from the Vue
  conformance-goldens program.

## Provenance caveats (read before trusting a detail)

- **Recovered snapshots.** These plans were recovered from a handoff record; they describe branch
  states at recovery time and may **lag the committed tree** on stages that have since landed. Where
  a plan and a committed design doc disagree on landed state, the committed doc wins — e.g. the
  session hot-prepared layer (`HotPrepared*`) is **DELETED** in this tree (see
  [`../parselower-design.md`](../parselower-design.md)), even though the unified plan still describes
  it as unwired scaffolding.
- **Companion doc-set not fully rehomed.** The unified plan and the cache-admission design reference a
  wider recovered doc-set (e.g. `native-typeinfo-parity*.md`, the design-gate lock docs,
  `single-engine-cutover-state.md`, `shared-engine-crash-fix-design.md`, `verification-traps.md`,
  `03-editor-engine-selection-rejection.md`). Those are **not** committed here; prose path-mentions to
  them are historical pointers into the recovered handoff, not in-tree links.
