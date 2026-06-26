# Performance Work — Handoff to the Semantic-DB Overhaul (provenance pointer)

> **Status (2026-06-17): ABSORBED.** The remaining LSP / compiler performance work
> is now owned by §4 **`UP — LSP / Compiler Performance Backlog`** (plus the
> cross-ref'd U-blocks) of
> [`semantic-db-overhaul-unified-remaining-plan.md`](./semantic-db-overhaul-unified-remaining-plan.md).
> This file is retained ONLY for historical track / branch / SHA provenance — every
> spec, file anchor, and gate now lives in the master plan. Do NOT implement from
> this file; read the UP block + the owning U-block's scope bullet.

Every per-item `lsp-perf/*` branch + its "ready" SHAs are **absent from this repo**
(only the retired `origin/lsp-perf/integration{,-overhaul}` ever existed). No
cherry-pick is possible: every remaining item is implemented from its SPEC in the
master plan, and every branch / commit name below is a **historical pointer to
intent + tests, never a load-bearing cherry-pick source**. The three UP.1 items
instead cite their REAL refactor-history commits (their diffs landed in this
integration).

## Provenance — where each absorbed slice now lives

| Track / item | Disposition | Home in the master plan | Source branch (retired) + commit (historical) |
|---|---|---|---|
| **DX** harness | absorbed (non-gated) | n/a (test-infra, already on tree) | — |
| **Non-gated D** (12 `perf(core)` codegen) | absorbed | n/a (already on tree) | `lsp-perf/d-compiler-perf` (retired) |
| **Non-gated L** (goto-def/type-def, auto-import, member recovery, source-owned hovers/event-args) | absorbed | n/a (already on tree) | `lsp-perf/l-lsp-bugs` (retired) |
| **B124** (shared script+macro parse) | absorbed | n/a (already on tree) | (retired) |
| **G8.2** scheduler supersede reverse-indices | **LANDED ✅** | UP.1 | refactor commit `24532fbca` (source branch retired) |
| **G9.1** workspace realpath memo | **LANDED ✅** | UP.1 | refactor commit `a4333b753` (source branch retired) |
| **G14.1** `Arc<PositionMapper>` read-path share | **LANDED ✅** | UP.1 (cross-note U13/U14) | refactor commit `0f2e19fc1` (source branch retired) |
| **Track C** ungated (LSP-local) | spec-only (branches retired) | UP.2 | `lsp-perf/c-item{5,11,12,13a,14,23,2}`, `lsp-perf/c-lsp-perf @ 6c2b4dd19` (retired) |
| **F1** host-owned retained-close | spec-only (branch retired) | U11 (UP.G index) | `lsp-perf/session-completion @ 23c3dc260` (retired) |
| **C host-API 16–20** | spec-only (never landed) | U11 (UP.G index) | (HELD; no branch) |
| **B-typeinfo cache 1–11** | spec-only (branch retired) | U3 / U10 / U8 / U12 (UP.G index) | `lsp-perf/b-typeinfo-perf @ fe0059597` is the G9.1 realpath memo — NOT the typeinfo work (retired) |
| **D-I3 / custom_elements** | spec-only (branch retired) | U14 (UP.G index) | `lsp-perf/d-compiler-perf` (retired; I3/custom_elements not on it) |
| **D-F4** CSS scoped-hash cutover | spec-only (branch retired) | UP.D (gov-flagged) | `lsp-perf/d-compiler-perf` (retired) |
| **F2** resolved-type read-set fact-scoping | spec-only (branch retired) | U3 (spec-drift corrected: `ResolvedTypeCacheDb` EXISTS at `host_construction.rs:961`) | `lsp-perf/f2-resolved-type-cache @ 4171597b7` (retired) |
| **Track L** gated (L-A storm, L-C.2 ownership, L-B TypeLocation, L-event-args) | spec-only (no branch) | UP.M / UP.G / U14 (L-C.2 + L-B homeless, STOP-for-sign-off) | (HELD; no branch) |
| **C15** ranged provider updates (CodeTransform hard-order) | spec-only (no branch) | UP.D (gov-flagged) | (HELD; no branch) |

Off-tree source specs (historical, on a retired machine — NOT in this repo):
`<scratch>/orch/lsp/PERF-PLAN-LSP-v7.txt`, `<scratch>/orch/lsp/GATED-ITEMS-C.md`,
`<scratch>/orch/lsp-bugs/GATED-ITEMS-L.md`, `<scratch>/orch/lsp-bugs/PLAN-L-v2.txt`. The
broader multi-track integration model is preserved in
[`perf-lsp-orchestration-plan.md`](./perf-lsp-orchestration-plan.md) (also historical).
