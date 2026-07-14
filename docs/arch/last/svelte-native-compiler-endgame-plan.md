# Svelte Native-Compiler Endgame — Plan & State Snapshot

**Snapshot date:** 2026-07-14
**Integration tip:** `506b493bad183092cedcf9e8111110c3f5f00076` (`feat/framework-adapters-clean`)
**Status:** 6 of 10 landing trains CONFIRMED. T3 just landed + confirmed.
**Origin:** frozen at `eed64c3c9` — local-only work; only the user pushes.

This is the durable resumable record of the frozen 10-train endgame for the native Svelte
compiler. Block-level detail lives in [`../svelte-native-compiler-plan.md`](../svelte-native-compiler-plan.md)
(the D-row design ledger). This file captures the train-level manifest, progress, the durable
rulings/insights from the confirmed trains, the debt ledger, the operating constraints, and the
exact resume point.

---

## 1. Frozen manifest (10 trains) + DAG

The supported-release manifest is a FROZEN finite train set with a FIXED denominator of **10**.
No feature train may be added without explicit user ratification (never report against a
denominator that can grow silently).

| Train | Scope | Status |
| --- | --- | --- |
| **T0** | 5l (prerequisite slice) | ✅ CONFIRMED |
| **R** | Refactor/rehome slice | ✅ CONFIRMED |
| **T1a** | (T1 split, part a) | ✅ CONFIRMED |
| **T1b** | (T1 split, part b) | ✅ CONFIRMED |
| **T2** | (compile/analyze surface) | ✅ CONFIRMED |
| **T3** | Style-options — I7 cssHash cache-identity + 5m essential `<svelte:options>` resolution + the `ComponentScopeFacts` component-name binder | ✅ CONFIRMED (this snapshot) |
| **T4** | Script-completion (5n + 5t) | ⏳ NEXT (STEP-0 pending) |
| **T5** | Reachability (Block I) | ⏳ remaining |
| **T6** | Quality-gates (Block 6 + 7) | ⏳ remaining |
| **T7** | Release-close (Block 13 subset) | ⏳ remaining |

**DAG:** `T0 → R → T1 → { T2 ∥ (I7 → T3) } → T4 → T5 → { T6 } → T7`
(T1 was split into T1a/T1b; I7 is the cssHash cache-identity seam that gated T3.)

**Progress:** confirmed **6/10** (T0, R, T1a, T1b, T2, T3). Remaining critical path: **T4 → T5 → T6 → T7**.

---

## 2. Resume point (exact next action)

**T4 STEP-0 — codex scope consult.** Front-load the design: run an unprimed codex-architect
scope consult over the 7 inputs staged in `/tmp/mom/T4/SCOPE-BRIEF.md` (held until T3 CONFIRMED —
now unblocked). Then write the T4 brief and dispatch the T4 implementation manager per the
MoM/CTO orchestration process (`.claude/skills/mom-cto-orchestration`, `.claude/skills/multi-agent-orchestration`).

Do NOT advance more than one unconfirmed train deep. An integration-confirm floor runs after
every five confirmed landing trains (last integration boundary tracking is in the `/tmp/mom` ledger).

---

## 3. T3 — what it delivered + why it took 19 rounds (durable insight)

T3 delivered: the **I7 cssHash cache-identity** seam (`svelte_css_hash_override` byte-exact gate
before the store-view read; cache non-determinism → `Determinism::Unverified` → Content→Stateless
fail-closed; server fails closed; correct R21 env-hash dimension), the **5m essential compile-options**
(`namespace`, `fragments` html+tree, `preserveWhitespace`, `preserveComments`, `discloseVersion`,
`name`, precedence), and the canonical **`ComponentScopeFacts` component-name binder**.

The `name`-option deconfliction (emit the component function name deconflicted exactly like
svelte's `Scope.generate`) drove 19 review rounds. The architecture converged through four ratified
rulings:

- **RULING A** (codex, user-ratified): build ONE compiler-owned authoritative scope binder
  (`component_scope_facts::build_component_scope_facts`), replacing three prior approximations;
  eliminate the redundant per-script reparse-walk.
- **RULING D** (dual-unprimed-codex, unanimous): replace the exclusion **blocklist** with a positive
  **`SvelteScopeProjection`** — a bounded O(n) pass over the sanctioned `reparse_module` program that
  mirrors svelte's `remove_typescript_nodes ∘ create_scopes` via an EXHAUSTIVE OXC-TS-AST match (no
  wildcard for any TS node kind), then binds the PROJECTED program with OXC `SemanticBuilder` so
  `SymbolFlags::is_value` is the complete selector. Completeness is enforced by a source-derived
  **bijection drift guard** (every svelte handler ⟺ one Verter classification; a new OXC/svelte
  variant trips it) — the mechanism that structurally ends the per-construct whack-a-mole.
- **RULING F** (dual-unprimed-codex, unanimous): the **root cause** of the long round count was
  **self-confirming tests** — the parity pins had been authored by reading Verter's own projection
  output rather than probing svelte, so a projection bug plus a matching wrong pin both passed green.
  The fix changes **provenance**: a **generated svelte-oracle corpus** (run pinned svelte@5.56.3
  offline → commit `{source, requested_name, official_emitted_name | reject_code}` → the hermetic
  Rust matrix asserts `derive_component_name` parity against the ORACLE-pinned outcome), mirroring
  the repo's existing `scripts/gen-svelte-parse-parity-corpus.mjs` → `svelte_parse_parity_matrix.rs`
  pattern. This surfaced + fixed 3 real bugs the self-confirming tests had hidden
  (`export * as ns` must reserve `ns`; `declare enum` and `export default class` are svelte
  hard-errors → reject axis).

**Three-bucket parity scoping (ZERO overclaim):** exact reserved-name parity is claimed only for
bucket-1 (constructs svelte COMPILES). Bucket-2 (svelte hard-errors: index-signature, ctor
param-property, decorator, value enum, value namespace) has no output → name-parity is vacuous →
reject axis / defensive-erase. Bucket-3 (`<T>x` angle-assertion, unparseable under `SourceType::tsx()`)
fail-closes. Buckets 2/3 are documented pre-existing debt, not chased.

**Durable testing principle (applies to all future conformance work):** conformance tests must be
**oracle-DERIVED** (generated from the official compiler), never projection-echoed. A pin copied
from Verter's own output is self-confirming and will hide bugs. Use the generated-corpus pattern.

---

## 4. Debt ledger (accepted category-4/5 — NOT release-blocking)

Recorded in `docs/arch/svelte-native-compiler-plan.md` D-rows; summarized here. None is a
supported-surface correctness/fail-open/invariant defect.

- **[cat-4] reject-parity** — index-signature / ctor param-property / accessor field / decorator are
  svelte hard-errors; Verter lacks an upstream reject gate (pre-existing; the projection defends
  scope regardless). Verter never mis-emits.
- **[cat-4] TS value-`enum`/`namespace`/`using`** — Verter fail-closes (behavioral parity: reject ⇔
  reject); only exact-diagnostic-code parity with svelte's `typescript_invalid_feature` is a gap.
- **[cat-4] `componentApi`** — Verter fails closed on any non-`5` value; exact error-code parity is
  a gap.
- **[cat-4/5] tsx-`<T>x` ambiguity** — the shared `reparse_module` uses `SourceType::tsx()` under
  which `<T>x` is JSX; Verter fail-closes the component. Dialect-aware reparse is out of scope
  (shared with the IDE scanners).
- **[cat-5] host `preserveComments`/comments compile-option** — the supported inline +
  `SvelteRuntimeOptions` surface is wired, correct, and golden-tested; routing the host compile-option
  bridge through the neutral framework-adapter carrier is a robustness improvement.
- **[cat-5] AST-aware primary D-47 guard** — `no_raw_import_specifier_walk_in_import_local_discharge_files`
  is a SECONDARY substring tripwire (per the Architecture Guard Rule, substring scanning cannot
  establish architectural compliance). It catches the direct `.specifiers` and visitor-based
  (`visit_import_declaration`) reintroductions but is not exhaustive; an AST-aware primary guard is
  the durable fix. Current code is genuinely D-47 compliant (import locals route through the shared
  `ClassifiedScriptImports` carrier).
- **[NIT] `parse_refusal` doc-comment** — its axis listing (preserveComments/discloseVersion) is
  prose-imprecise; the resolver behavior (fold as compile-options, fail closed on inline unknowns)
  is correct.

---

## 5. Operating constraints (MUST persist across sessions)

- **Origin frozen at `eed64c3c9` — NEVER push.** Only the user pushes. Local true-ff landings only;
  when the integration branch is checked out in the main worktree, move the ref via `git update-ref`
  from the train worktree and reconcile main with `git reset --hard`.
- **No `Co-authored-by` / attribution trailer** on any commit or PR.
- **No plan/phase/train/block vocabulary** in `crates/*/src/**` or test code, or in conventional
  commit messages. `docs/arch/` and `.claude/skills/` ARE the sanctioned homes for that vocabulary
  (this file included). Guard: `no_phase_archaeology_in_production_code` / `_in_general_test_code`.
- **Manifest denominator is 10 and FIXED.** Any critical-path growth needs explicit user ratification.
  Classify every discovery via the five-way scope-admission policy (blocking-defect / invariant-defect
  / required-acceptance-row → fold into owning train; unsupported-completeness → post-release
  fail-closed; optional-architecture → non-blocking).
- **Canonical Rust gate (8-step, landing-frozen tree):** `cargo nextest run --workspace` +
  `cargo test -p verter_session --tests` + `cargo clippy --workspace -- -D warnings` +
  `cargo fmt --all --check` + `node scripts/gen-svelte-goldens.mjs --check` +
  `node scripts/gen-svelte-goldens.mjs --conformance --check` +
  `node scripts/gen-svelte-codegen-corpus.mjs --check` + the name-parity corpus `--check` + the
  `no_phase_archaeology` / `no_oversize_files` / `tracked_paths_are_portable` guards. A timeout is
  NEVER a pass. Bare `cargo test --workspace --tests` SILENTLY SKIPS the verter_session integration
  suite — never use it as the sole Rust gate.
- **8GB-box discipline:** the full `cargo nextest run --workspace` wedges (listing-phase 0% CPU or
  memory-thrash) when there are leaked child processes or a cold build. Resolution: kill all stray
  `cargo|nextest|verter` processes → `cargo nextest run --workspace --no-run` to fully warm the
  binaries → `cargo nextest run --workspace --test-threads=1` on the clean+warm state (lists +
  completes serially, memory-light). Keep disk healthy (reclaim stale scratch if low).
- **Compiled-output conformance** is behavioral + structural/topology parity vs svelte@5.56.3, not
  raw-byte identity; cosmetic JS carrier formatting is waived; observable/source-authored names
  (like the `name` option output) are in-contract.

---

## 6. Orchestration process (how this is executed)

Driven as a CTO/MoM tier over `.claude/skills/mom-cto-orchestration` + `.claude/skills/multi-agent-orchestration`:
a pure orchestrator dispatches one implementation manager per train; every land is gated on a
three-review barrier (author-dependent lens mix — Claude author ⇒ 2 codex + 1 claude — over the
immutable cumulative tree, distinct lenses A/B/C), a §1a mutation-recipe pass (plant→RED→restore→GREEN,
sampling forbidden), the canonical gate on a rebased landing-frozen tree, then a separate
author-independent confirm manager (fresh gate + §1a re-execution + an unprimed codex adversarial
leg). Only `VERDICT:CONFIRMED` closes a train. Architecture forks go to codex (unprimed, best-on-merits,
breaking OK), user-ratified where they would grow the critical path. Live orchestration state
(append-only) lives under `/tmp/mom/_ledger/PROGRESS.md`; T3 artifacts under `/tmp/mom/T3/impl/`.

---

*This snapshot was saved at the user's request to pause the run. To resume: read this file +
`/tmp/mom/_ledger/PROGRESS.md`, then execute the §2 resume point (T4 STEP-0).*
