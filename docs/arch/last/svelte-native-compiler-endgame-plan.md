# Svelte Native-Compiler Endgame — Plan & State Snapshot (self-contained resume record)

**Snapshot date:** 2026-07-14
**Integration tip:** `feat/framework-adapters-clean` HEAD — T3 squash `506b493bad183092cedcf9e8111110c3f5f00076` + this doc commit on top. T4 bases on the LIVE branch HEAD at resume time (not a pinned SHA).
**Status:** 6 of 10 landing trains CONFIRMED. T3 landed + confirmed; the run was paused here at the user's request.
**Origin:** frozen at `eed64c3c9` — local-only work; **only the user pushes.** Never run a network git op.

> **READ THIS FIRST — you are resuming on a FRESH machine.** All prior orchestration scratch
> (`/tmp/mom/_ledger/PROGRESS.md`, `/tmp/mom/T4/SCOPE-BRIEF.md`, `/tmp/mom/T3/impl/*`, the RELEASE-PROPOSAL
> scratch, the `verter-T3` worktree) lived under `/tmp` on the previous machine and **does NOT exist here.**
> This file is the durable replacement: everything machine-local has been inlined below. The other durable,
> in-repo sources you rely on are listed in §8. You bootstrap a fresh `/tmp/mom` ledger yourself (§7) and
> resume the MoM/CTO loop from §2. Do not go looking for the old `/tmp` files — reconstruct from here + git.

This is the durable resumable record of the FROZEN 10-train endgame for the native Svelte compiler.
Block-level (D-row) design detail lives in the in-repo ledger
[`../svelte-native-compiler-plan.md`](../svelte-native-compiler-plan.md) (292 KB, comprehensive, checked in).
This file captures the train-level manifest, progress + git anchors, the FULL inlined scope for every
remaining train, the durable rulings/insights from the confirmed trains, the debt ledger, the operating
constraints, the orchestration process, and the exact resume point.

---

## 1. Frozen manifest (10 trains) + DAG + git anchors

The supported-release manifest is a FROZEN finite train set with a FIXED denominator of **10**. No feature
train may be added without explicit USER ratification (never report against a denominator that can grow
silently). The manifest's original home was an ephemeral `RELEASE-PROPOSAL.md` scratch (now gone); **this
committed doc is its durable home**, cross-referenced to the design ledger's D-A ratification rows.

| Train | Scope (block IDs → §3/§4) | Status | Landed squash |
| --- | --- | --- | --- |
| **T0** | 5l prerequisite slice (default cssHash conformance) | ✅ CONFIRMED | in `git log` before `ebe8a789c` |
| **R** | Refactor/rehome slice | ✅ CONFIRMED | in `git log` before `ebe8a789c` |
| **T1a** | native `<style>` CSS scoping | ✅ CONFIRMED | `4e95d16b1` (feat(core): native Svelte `<style>` CSS scoping) |
| **T1b** | executable CSS coverage manifest + covering-array generator | ✅ CONFIRMED | `ca9a8bebf` |
| **T2** | legacy-mode client — `<slot>`, `createEventDispatcher`, value-wrap closure (5i) | ✅ CONFIRMED | `ebe8a789c` |
| **T3** | I7 cssHash cache-identity + 5m essential `<svelte:options>` + `ComponentScopeFacts` name binder | ✅ CONFIRMED | `506b493bad` |
| **T4** | **Script-completion — 5n + 5t** (§3) | ⏳ **NEXT** (STEP-0 pending) | — |
| **T5** | **Reachability — Block I** (§4.1) | ⏳ remaining | — |
| **T6** | **Quality-gates — Block 6 + Block 7** (§4.2) | ⏳ remaining | — |
| **T7** | **Release-close — Block 13 subset** (§4.3) | ⏳ remaining | — |

**DAG:** `T0 → R → T1 → { T2 ∥ (I7 → T3) } → T4 → T5 → { T6 } → T7`
(T1 was split into T1a/T1b; I7 is the cssHash cache-identity seam that gated T3.) The design ledger's
authoritative block execution order is `5a-5h → 5s → 5i-5m → 5n → I → 6/7/8/9 → 10/11 → RC → 12/14 → 13`;
the 10-train manifest is exactly the release-blocking prefix of that order (blocks 8/9/10/11/RC/12/14 and
the SSR server corpora are **POST-RELEASE**, not in this manifest).

**Progress:** confirmed **6/10** (T0, R, T1a, T1b, T2, T3). Remaining critical path: **T4 → T5 → T6 → T7**.
An integration-confirm floor runs after every five confirmed landing trains and at each milestone boundary
(the last integration boundary was at/around the T2→T3 grouping; re-check before T7 close-out).

To recover the earlier per-train squash SHAs on this machine: `git log --oneline --first-parent` on
`feat/framework-adapters-clean` — each confirmed train landed as ONE squash with a `feat(core): native
Svelte …` (or `feat(*)`) subject; the six above are the contiguous run ending at `506b493bad`.

---

## 2. Resume point (exact next action) — self-contained

**T4 STEP-0 — codex scope consult, then brief + dispatch the T4 manager.** The full T4 scope is inlined in
§3 (the ephemeral `/tmp/mom/T4/SCOPE-BRIEF.md` is reproduced there verbatim-equivalent). Steps:

1. **Bootstrap the ephemeral ledger** on this machine (§7): `mkdir -p /tmp/mom/_ledger /tmp/mom/T4/impl`;
   seed `/tmp/mom/_ledger/PROGRESS.md` with the §1 manifest/progress line and a "resumed from
   docs/arch/last on <date>" checkpoint. (This is throwaway working state, NOT committed — repo cleanliness
   rule: orchestration state lives in `/tmp/mom`, never in the repo.)
2. **STEP-0 codex scope consult (unprimed, best-on-merits).** Run ONE (or two neutral legs for the
   high-stakes TS-strip mechanism fork) `codex` architect consult over the §3 "STEP-0 consult inputs".
   Frame NEUTRALLY — ask "what is BEST", never prime a conclusion; a mis-framed prompt VOIDs the verdict.
   Persist the prompt + raw verdict + input-id/model/effort under `/tmp/mom/T4/`.
3. **Write the T4 impl-and-land brief** (self-contained, per `.claude/skills/multi-agent-orchestration`
   templates): 5n slices then 5t slices, co-trained; the two TS/import fail-closed gates flip to positives
   in the SAME change; official `lang="ts"` CLIENT corpus gates output; everything else stays fail-closed
   with an exact diagnostic; §1a discriminating recipes; land-with-feature CRITICAL-rule guards.
4. **Dispatch the T4 implementation manager** as an Agent sub-agent and drive the standard train lifecycle
   (§6): three-review barrier (Claude author ⇒ 2 codex + 1 claude, distinct lenses A/B/C) → §1a → the
   canonical 8-step gate on the rebased landing-frozen tree → squash-land (local true-ff, NEVER push) →
   separate author-independent CONFIRM manager. Only `VERDICT:CONFIRMED` closes T4 → 7/10.

Do NOT advance more than one unconfirmed train deep.

---

## 3. T4 (Script-completion) — FULL inlined scope brief

*(This reproduces the T4 scope frozen during T3 — the ephemeral `/tmp/mom/T4/SCOPE-BRIEF.md`, now durable.
Authority: design-ledger D-A ratification + the D-46 "Done when". Block scope lines cited into
`../svelte-native-compiler-plan.md`.)*

**T4 = two block IDs, co-trained (5n slices, then 5t slices):**

**5n — Script/module-item completion** (design ledger ≈ line 1281; deps 4, 5m=T3). Broaden the fail-closed
residual for non-import top-level module + instance script items beyond the current allowlist:
- arbitrary `<script module>` statements + exports (today: `<script module>` is IMPORT-ONLY, a non-import
  module item fails closed as `ModuleScriptItem`);
- instance-script statements beyond the supported rune/state/`bind:this`-local/function-pair allowlist —
  functions, classes, enums, plain locals read in non-interpolation positions;
- broad script-item lowering + source-order preservation.
- **BOUNDARY:** legacy reactive `$:` labels are owned by 5i (T2, CONFIRMED) — NOT 5n. Verify 5n's new
  instance/module admissions do NOT collide with the 5i-owned legacy `$:` / `export let` allowlist.

**5t — TypeScript script lowering + type-only import elision** (design ledger ≈ line 1282; D-46 ≈ line 1123;
deps 4, 5s [LANDED+CONFIRMED], 5n [intra-train, 5t rides AFTER 5n]). **RELEASE-BLOCKING** (D-A):
- strip TS annotations before runtime lowering (official svelte/compiler TS preprocessing);
- elide `import type` / per-specifier `type` (a fully-type-only import emits NO statement; a mixed import
  emits value members only);
- lower the TS-only constructs the plain-JS allowlist refuses;
- open the TS-wrapped lvalue/bind canonicalization the plain-JS gates fail-closed today.
- 5t **owns every source comment deferring to "the script-completion block (5t)"** — grep and discharge them.

**Acceptance rows / invariants:**
- **D-46 "Done when"** (ledger ≈ line 1123): (a) a `lang="ts"` component emits the official TS-stripped
  module; (b) a type-only import emits nothing, a mixed import emits value members only; (c) the TWO
  fail-closed gates convert to positives in the SAME change; (d) the official `lang="ts"` corpus gates output.
- 5n corpus: official module/instance script-item corpus.
- 5t corpus: official `lang="ts"` **CLIENT** corpus (TS-strip output + type-only elision). The SSR/server
  `lang="ts"` corpus is **POST-RELEASE** (Block 8 backfills 5a-5t SSR) — T4 is **client-only**.
- Manifest invariant: everything not admitted stays fail-closed with an exact diagnostic — never fail-open /
  miscompile. CRITICAL-rule guards land WITH the feature; zero fail-open in the supported surface; the T1b
  coverage-manifest registers any new cells; §1a discriminating recipes for every new correctness test.

**Current code touch-points (the fail-closed gates 5n/5t flip — verify line numbers on the live tree first):**

*5t:*
- TS parse gate: `crates/verter_compiler/src/svelte/runtime/parse_refusal.rs` (raises
  `UnsupportedSvelteRuntimeSurface::TypeScript` for `lang ∈ {ts,tsx,typescript}`; carries the `TODO … Owned
  by … 5t` marker).
- Type-only import gate: `crates/verter_compiler/src/svelte/runtime/client_surface_imports.rs`
  (`refuse("type-only import")` → `ScriptImport { construct: "type-only import" }`).
- Surface enum + codes: `crates/verter_compiler/src/svelte/runtime/unsupported.rs` (`TypeScript`,
  `ScriptImport`; codes `svelte-runtime-unsupported-typescript` / `-module-script-item` /
  `-instance-script-item`).
- TS-wrapped lvalue/widening (fail-closed today, 5t opens): `expr_emit.rs` (e.g. `$state(0 as number)`).
- Script body TS parse: `script_body_parse.rs` (`SourceType::ts()` already wired).

*5n:*
- Instance-script allowlist: `instance_items.rs` (`SupportedInstanceScriptItem`, `classify_*` →
  `InstanceScriptItem` fail-close).
- Module-script allowlist: `client_surface_script.rs` (`classify_script_items`, module classifier →
  `ModuleScriptItem`).
- Build entry: `client_surface.rs`, `client_compile.rs` (`SupportedClientIr::build`).
- Fail-closed tests to convert → positives: `client_tests.rs` (the `lang=ts` cluster, the `type-only import`
  case, the instance-script-item reject cases).

**STEP-0 codex scope-consult inputs (ask NEUTRALLY):**
1. Re-affirm labels: T4 = script-completion (5n+5t); manifest = T0,R,T1a,T1b,T2,T3,T4,T5,T6,T7 (10); no T8+.
2. TS-strip authority/mechanism: official svelte/compiler TS preprocessing (D-46) vs OXC TS-erasure
   (`SourceType::ts()` present)? How does it compose with the D-60 deferred expr-rewrite reparse + sourcemap
   fidelity (a T6 downstream concern)?
3. Exact TS-only construct set 5t must lower (`as` / `satisfies` / non-null `!`; enums; type params on
   fns/classes; the TS-wrapped bindable-member walk — the `expr_emit.rs` widening case).
4. 5n↔5i boundary integrity: T2's legacy value-wrap closure touched instance-script surfaces — confirm 5n's
   new instance/module admissions don't collide with the 5i-owned legacy `$:` / `export let` allowlist.
5. Corpus/SSR axis: confirm 5t's `lang="ts"` SERVER corpus is deferred (Block 8) → T4 client-only.
6. Two-gates→positives completeness (D-46): confirm the TS fail-closed sites are exhaustively
   `parse_refusal.rs` + `client_surface_imports.rs` (a grep also surfaced notes in `expr_emit.rs`,
   `cross_slot_redeclaration.rs`, reactive paths) so "flip both in the same change" is exhaustive.
7. Dep-gating is already satisfied (T3 CONFIRMED); base T4 on the live branch HEAD.

**Doc-hygiene flag (verify before briefing):** the design ledger's "5g-5n (incl 5s)" shorthand + the
execution-order string historically OMITTED 5t; the R-reconciliation "5g-5t" fix should be verified applied.

---

## 4. T5 / T6 / T7 — scope + design-ledger pointers

The in-repo design ledger `../svelte-native-compiler-plan.md` is the authoritative block spec — read the
named blocks there at each train's STEP-0. Concise scope:

### 4.1 T5 — Reachability (Block I)
Dead-code / reachability closure: statically-false `{#if}` branch removal and unused-generated-helper
cleanup are *safe* eliminations (design ledger §3 "safe/unsafe" table ≈ lines 652-661); the invariant is
**never** drop lifecycle/effect/helper calls unless the whole branch is unreachable, and never hand-roll DOM
in place of the official helper topology. Block I runs AFTER 5n (needs the completed script surface) and
BEFORE the quality-gate blocks. STEP-0 consult: confirm Verter's reachability analysis matches official
svelte's (branch-condition compile-time-constant detection; helper liveness) and that eliminations are
behavior + structure preserving per the Compiled-Output Conformance rule.

### 4.2 T6 — Quality-gates (Block 6 + Block 7)
- **Block 6** — sourcemap fidelity closure (composes with the D-60 deferred expr-rewrite reparse flagged in
  T4 consult input #2) + the conformance-comparator hardening tracked in ledger rows **D-17** (structural
  comparator completeness — the fail-closed fallback contract for new mjs-parseable node forms), **D-19**
  (semantic-comment golden oracle gap), **D-20** (typed `ConformanceSig` + re-printer dependency-deny).
- **Block 7** — the §7 perf metric + the ≤1.10× Svelte-vs-Vapor incremental regression gate (already runs at
  each 5a-5k landing; T6 closes it as a wired gate). Note the **real-world** perf-acceptance extension and
  the RC benchmark axis are POST-RELEASE (Block 12 / RC), NOT in this manifest.
STEP-0 consult: which D-17/D-19/D-20 rows are release-blocking vs deferrable post-release (each has a
codex-DEFER ruling recorded in the ledger); confirm the sourcemap closure scope.

### 4.3 T7 — Release-close (Block 13 subset)
The release-close checklist SUBSET that is release-blocking: zero correctness/invariant debt in the supported
surface, zero fail-open, exact fail-closed coverage outside it. Blocks 8/9/10/11/RC/12/14 are explicitly
POST-RELEASE and NOT part of T7. T7 requires a final **integration-confirm** (`VERDICT:INTEGRATION-CONFIRMED`)
across all 10 trains before close-out. A release-close history purge of scratch/report clutter is a
user-authorized destructive op requiring final user go-ahead at execution time. STEP-0 consult: enumerate the
exact Block-13 rows that are release-blocking vs the post-release remainder, and confirm the debt ledger (§5)
contains zero relabeled supported-surface defects.

---

## 5. T3 durable insight (rulings A/D/F) + debt ledger

### 5.1 What T3 delivered + why it took 19 rounds
T3 delivered: the **I7 cssHash cache-identity** seam (`svelte_css_hash_override` byte-exact gate before the
store-view read; cache non-determinism → `Determinism::Unverified` → Content→Stateless fail-closed; server
fails closed; correct R21 env-hash dimension), the **5m essential compile-options** (`namespace`, `fragments`
html+tree, `preserveWhitespace`, `preserveComments`, `discloseVersion`, `name`, precedence), and the
canonical **`ComponentScopeFacts` component-name binder**.

The `name`-option deconfliction (emit the component function name deconflicted exactly like svelte's
`Scope.generate`) drove 19 review rounds, converging through four ratified rulings:

- **RULING A** (codex, user-ratified): build ONE compiler-owned authoritative scope binder
  (`component_scope_facts::build_component_scope_facts`), replacing three prior approximations; eliminate the
  redundant per-script reparse-walk.
- **RULING D** (dual-unprimed-codex, unanimous): replace the exclusion **blocklist** with a positive
  **`SvelteScopeProjection`** — a bounded O(n) pass over the sanctioned `reparse_module` program that mirrors
  svelte's `remove_typescript_nodes ∘ create_scopes` via an EXHAUSTIVE OXC-TS-AST match (NO wildcard for any
  TS node kind), then binds the PROJECTED program with OXC `SemanticBuilder` so `SymbolFlags::is_value` is
  the complete selector. Completeness is enforced by a **source-derived bijection drift guard** (every svelte
  `remove_typescript_nodes` handler ⟺ one Verter classification; a new OXC/svelte variant trips it) — the
  mechanism that structurally ends the per-construct whack-a-mole.
- **RULING F** (dual-unprimed-codex, unanimous): the **root cause** of the long round count was
  **self-confirming tests** — parity pins had been authored by reading Verter's OWN projection output rather
  than probing svelte, so a projection bug + a matching wrong pin both passed green. The fix changes
  **PROVENANCE**: a **generated svelte-oracle corpus** (run pinned svelte@5.56.3 offline → commit
  `{source, requested_name, official_emitted_name | reject_code}` → the hermetic Rust matrix asserts
  `derive_component_name` parity against the ORACLE-pinned outcome), mirroring the repo's existing
  `scripts/gen-svelte-parse-parity-corpus.mjs` → `svelte_parse_parity_matrix.rs` pattern. This surfaced +
  fixed 3 real bugs the self-confirming tests had hidden (`export * as ns` must reserve `ns` → `ns_1`;
  `declare enum` and `export default class` are svelte hard-errors → reject axis).

**Three-bucket parity scoping (ZERO overclaim):** exact reserved-name parity is claimed only for bucket-1
(constructs svelte COMPILES). Bucket-2 (svelte hard-errors: index-signature, ctor param-property, decorator,
value enum, value namespace) has no output → name-parity is vacuous → reject axis / defensive-erase.
Bucket-3 (`<T>x` angle-assertion, unparseable under `SourceType::tsx()`) fail-closes. Buckets 2/3 are
documented pre-existing debt, not chased.

**Durable testing principle (applies to ALL future conformance work, T4-T7 included):** conformance tests
must be **oracle-DERIVED** (generated from the official compiler), NEVER projection-echoed. A pin copied from
Verter's own output is self-confirming and will hide bugs. Use the generated-corpus pattern
(`scripts/gen-svelte-*-corpus.mjs` → hermetic Rust matrix with a `--check` freshness gate).

### 5.2 Debt ledger (accepted category-4/5 — NOT release-blocking)
Recorded in `../svelte-native-compiler-plan.md` D-rows; summarized here. None is a supported-surface
correctness / fail-open / invariant defect.

- **[cat-4] reject-parity** — index-signature / ctor param-property / accessor field / decorator are svelte
  hard-errors; Verter lacks an upstream reject gate (PRE-EXISTING; base compiles them too; the projection
  defends scope regardless). Verter never mis-emits.
- **[cat-4] TS value-`enum`/`namespace`/`using`** — Verter fail-closes (behavioral parity: reject ⇔ reject);
  only exact-diagnostic-code parity with svelte's `typescript_invalid_feature` is a gap.
- **[cat-4] `componentApi`** — Verter fails closed on any non-`5` value; exact error-code parity is a gap.
- **[cat-4/5] tsx-`<T>x` ambiguity** — the shared `reparse_module` uses `SourceType::tsx()` under which
  `<T>x` is JSX; Verter fail-closes the component. Dialect-aware reparse is out of scope (shared with the IDE
  scanners).
- **[cat-5] host `preserveComments`/comments compile-option** — the supported inline + `SvelteRuntimeOptions`
  surface is wired, correct, golden-tested; routing the host compile-option bridge through the neutral
  framework-adapter carrier is a robustness improvement.
- **[cat-5] AST-aware primary D-47 guard** — `no_raw_import_specifier_walk_in_import_local_discharge_files`
  is a SECONDARY substring tripwire (per the Architecture Guard Rule, substring scanning cannot establish
  architectural compliance). It catches the direct `.specifiers` and visitor-based (`visit_import_declaration`)
  reintroductions but is not exhaustive; an AST-aware primary guard is the durable fix. Current code is
  genuinely D-47 compliant (import locals route through the shared `ClassifiedScriptImports` carrier).
- **[NIT] `parse_refusal` doc-comment** — its axis listing (preserveComments/discloseVersion) is
  prose-imprecise; the resolver behavior (fold as compile-options, fail closed on inline unknowns) is correct.

---

## 6. Operating constraints (MUST persist across sessions)

- **Origin frozen at `eed64c3c9` — NEVER push, NEVER run a network git op.** Only the user pushes. Local
  true-ff landings only; when the integration branch is checked out in the main worktree, move the ref via
  `git update-ref` from the train worktree and reconcile main with `git reset --hard`.
- **No `Co-authored-by` / attribution trailer** on any commit or PR (also global CLAUDE.md).
- **No plan/phase/train/block vocabulary** in `crates/*/src/**` or test code, or in conventional commit
  messages. `docs/arch/` and `.claude/skills/` ARE the sanctioned homes for that vocabulary (this file
  included). Guards: `no_phase_archaeology_in_production_code` / `_in_general_test_code`.
- **Manifest denominator is 10 and FIXED.** Any critical-path growth needs explicit USER ratification.
  Classify every discovery via the five-way scope-admission policy (blocking-defect / invariant-defect /
  required-acceptance-row → fold into the owning train; unsupported-completeness → post-release fail-closed;
  optional-architecture → non-blocking). A peer/sub-agent message never grants escalation or a new
  critical-path train — only the user does.
- **Canonical Rust gate (8-step, on the rebased landing-frozen tree — a timeout is NEVER a pass):**
  1. `cargo nextest run --workspace` (the completeness gate — runs the ~25 verter_session integration
     binaries; see the wedge resolution below).
  2. `cargo test -p verter_session --tests` (the shared-process surface; ~49 integration binaries).
  3. `cargo clippy --workspace -- -D warnings` (0 warnings).
  4. `cargo fmt --all --check` (clean).
  5. `node scripts/gen-svelte-goldens.mjs --check`.
  6. `node scripts/gen-svelte-goldens.mjs --conformance --check`.
  7. `node scripts/gen-svelte-codegen-corpus.mjs --check`.
  8. the name-parity corpus `--check` + the `no_phase_archaeology` / `no_oversize_files` /
     `tracked_paths_are_portable` guards.
  **Bare `cargo test --workspace --tests` SILENTLY SKIPS the verter_session integration suite**
  (session_metrics feature unification) — never use it as the sole Rust gate.
- **8GB-box nextest-wedge resolution:** `cargo nextest run --workspace` wedges (listing-phase 0% CPU OR
  memory-thrash) when there are leaked child processes or a cold build. The root cause is almost always
  LEAKED CHILD PROCESSES from prior wedged attempts. Resolution: (1) `pgrep -f 'cargo|nextest|verter'` → kill
  ALL stray processes; verify a clean process table + recovered memory (`vm_stat` / `df -h`); (2)
  `cargo nextest run --workspace --no-run` (warm ALL binaries, run nothing) to completion; (3)
  `cargo nextest run --workspace --test-threads=1` on the clean+warm state (lists + completes serially,
  memory-light; on T3 this completed ~17947 pass / 0 fail). Keep disk healthy (reclaim stale scratch if
  <5 Gi; do NOT delete the incremental cargo cache — that forces a cold build and re-triggers the wedge). One
  heavy cargo at a time (target/ lock). *(These numbers are machine-specific; a well-resourced fresh machine
  may run the plain `cargo nextest run --workspace` without the wedge — try it first, fall back to the
  resolution only if it wedges.)*
- **Compiled-output conformance** is behavioral + structural/topology parity vs svelte@5.56.3, NOT raw-byte
  identity; cosmetic JS carrier formatting is waived; observable/source-authored names (like the `name`
  option output) are in-contract.
- **TDD is mandatory**; every new correctness test lands with a §1a mutation recipe (plant → RED → restore →
  GREEN + an unplanted control); sampling is forbidden at confirm.

---

## 7. Orchestration process (how this is executed) + fresh-machine bootstrap

Driven as a CTO/MoM tier over the in-repo skills `.claude/skills/mom-cto-orchestration` +
`.claude/skills/multi-agent-orchestration` + `.claude/skills/testing` (READ THESE — they are the full
manual). A pure orchestrator (CTO) dispatches ONE implementation manager per train and NEVER writes code;
every land is gated on:
- a **three-review barrier** — 3 independent blind reviewers over the immutable cumulative train tree, one
  assigned lens each: (A) semantic parity / oracle validity / coverage-completeness; (B) architecture /
  typed-IR ownership / fail-closed / rule integrity; (C) host integration / caching / source maps / runtime
  behavior / regression blast radius. Author-dependent mix: a **Claude author ⇒ 2 codex + 1 claude**. Iterate
  consolidate → one comprehensive fix commit → re-review until a FINAL clean 3/3 over the complete cumulative
  diff;
- a **§1a mutation-recipe pass** (author-independent; every recipe plant→RED→restore→GREEN, sampling
  forbidden);
- the **canonical 8-step gate** (§6) on the rebased landing-frozen tree;
- a **squash-land** (local true-ff, NEVER push);
- a separate **author-independent CONFIRM manager** (fresh gate + §1a re-execution + an unprimed codex
  adversarial leg). Only `VERDICT:CONFIRMED` closes a train.

Architecture forks go to `codex` (unprimed, best-on-merits, breaking OK), USER-ratified where they would grow
the critical path. Dispatch mechanism default = the **Agent tool** (fresh sub-agent per role; a blocking call
returns the report, a background call notifies on completion). Reviewer / §1a / confirm roles run on the
highest available model at max effort; persist prompt + report + input-id + model + effort for every
gate-bearing dispatch.

**Fresh-machine bootstrap (do this before dispatching T4):**
1. `pnpm install` in `/Users/carlosrodrigues/Documents/dev/verter` (native + TS deps). Confirm the branch is
   `feat/framework-adapters-clean` at the §1 tip and `git status` is clean.
2. Recreate the throwaway ledger: `mkdir -p /tmp/mom/_ledger /tmp/mom/T4/impl`; write an initial
   `/tmp/mom/_ledger/PROGRESS.md` checkpoint (manifest 10 · confirmed 6/10 · active none → T4 · remaining
   T4,T5,T6,T7 · integration = live HEAD · origin frozen eed64c3c9 · never push · resumed from
   docs/arch/last).
3. Create the T4 worktree OUTSIDE the repo (`git worktree add ../verter-T4 -b mom/T4-script-completion`) at
   STEP-0 completion; remove it after land + confirm.
4. The prior `verter-T3` worktree does not exist on this machine — nothing to clean up.

The live orchestration ledger is append-only under `/tmp/mom/_ledger/PROGRESS.md` (throwaway, not committed).
This doc + the in-repo design ledger + git history are the ONLY durable record.

---

## 8. Durable resources map (what a fresh agent must read)

All in-repo (checked in) — available on any fresh checkout:
- **This file** — train manifest, progress, T4 full scope, T5/T6/T7 scope, rulings, debt, constraints, resume.
- `docs/arch/svelte-native-compiler-plan.md` — the 292 KB D-row design ledger (block specs, D-A ratification,
  D-46 5t "Done when", D-17/D-19/D-20 comparator debt, D-47/D-60/D-63, the safe/unsafe reachability table,
  the block execution order). **Authoritative block spec — read named blocks at each train STEP-0.**
- `.claude/skills/mom-cto-orchestration/` (+ `reference/PROTOCOL.md`, `LANDING-PROTOCOL.md`,
  `CHECKPOINT-PROTOCOL.md`, `WAIT-PROTOCOL.md`) — the CTO tier manual.
- `.claude/skills/multi-agent-orchestration/` (+ `references/templates.md`) — the implementation-manager +
  brief/reviewer/fix/consult templates.
- `.claude/skills/testing/`, `.claude/skills/framework-adapters/`, `.claude/skills/compiler-codegen/` — test
  patterns, the framework-adapter substrate, the two-codegen-path + conformance rules.
- `CLAUDE.md` — the CRITICAL architecture rules (Compiled-Output Conformance, Framework Adapter Substrate,
  typed-IR-only resolver, etc.) that every train must honor.
- `scripts/gen-svelte-*.mjs` — the oracle-corpus generators (goldens, conformance, codegen corpus,
  name-parity, parse-parity) with `--check` freshness gates; the RULING-F pattern for all new conformance
  tests.
- `git log --first-parent` on `feat/framework-adapters-clean` — the confirmed-train squash SHAs.

**Gone on a fresh machine (do NOT look for these):** `/tmp/mom/_ledger/PROGRESS.md`,
`/tmp/mom/T4/SCOPE-BRIEF.md` (reproduced in §3), `/tmp/mom/T3/impl/*` (rulings summarized in §5), the
ephemeral `RELEASE-PROPOSAL.md` scratch (manifest now in §1), the `verter-T3` worktree.

---

*Saved at the user's request to pause the run and make it resumable on a fresh machine. To resume: read this
file top-to-bottom + the §8 in-repo sources, bootstrap per §7, then execute §2 (T4 STEP-0). Drive train-by-
train to T7 close-out; never push; keep the denominator at 10 unless the user ratifies growth.*
