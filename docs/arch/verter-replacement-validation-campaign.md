# Verter Replacement Validation Campaign

> Status: PLAN / CHARTER (committed so any agent or contributor can understand the intent). The detailed tooling + methodology design (Phase 0) is being produced by a dedicated design effort and will extend §5–§6 below; an un-primed codex-architect verdict will be folded in on ratification.

## 1. North star

Make Verter a **confident drop-in replacement** for, and provably better + faster than:

- **Volar** — the Vue IDE language tooling (LSP features over `.vue`/`.svelte`).
- **vue-tsc** — Vue SFC type-checking.
- **`@vue/compiler-sfc`** (the official Vue compiler) — SFC → runtime render-function compilation.

"Drop-in" means: a user can swap Verter in for any of the three and get **at least** equivalent behavior — and in the many cases where the incumbents are wrong or slow, strictly better. The groundwork exists; this campaign extends it to a wide, evidence-backed level of confidence.

## 2. Validation corpus (LOCAL ANALYSIS INPUTS ONLY)

Verter is exercised against real-world projects to find DX/type/build deviations:

Seven real-world Vue codebases of varied shape (apps, a component library, a Nuxt project), enumerated only in the gitignored local analysis config (`.analysis/`) — never named in committed artifacts (see §4.3).

**These projects are analysis inputs, never dependencies of the repo.** See the Hermetic Extraction rule (§4). Their on-disk paths live only in a **gitignored local analysis config**; they MUST NOT appear in any committed code, fixture, test, or commit message.

## 3. Three workstreams

Each workstream is a **comparison against a reference**. The reference is a *baseline, not ground truth* — Verter must be **correct** even where the reference is wrong (§4.2).

| # | Workstream | Replaces | "Correct" means | Reference baseline |
|---|---|---|---|---|
| **A** | **DX / IDE** | Volar | For every supported mapped TS/JS expression position, Verter's IDE features behave like the equivalent standalone `.ts`/`.tsx`/`.jsx` program, mapped back to the SFC — on **both tsserver and tgo**. Features: diagnostics, hover, definition/type-definition, references, rename, completion + resolve, signature help, document highlights, semantic tokens, inlay hints, and code-actions whose edits map exactly. | Volar |
| **B** | **TSC** | vue-tsc | Verter reports the correct type-error / diagnostic **set** for the project's SFCs. | vue-tsc (Volar SFC→TS) |
| **C** | **build** | `@vue/compiler-sfc` | Verter's compiled render functions are **functionally identical** to the official output — same runtime behavior. Formatting, variable names, and structure may differ and that is acceptable; we deliberately do **not** make the output textually identical. | official `@vue/compiler-sfc` |

Symptoms we hunt: incorrect or missing diagnostics, wrong / unfound imports, wrong hover or inferred types, missing fallthrough props, generic-instantiation errors, functional divergence in compiled output.

## 4. Methodology

### 4.1 The closed loop (per deviation)

```
run project through Verter (workstream A/B/C)
  → detect a deviation vs the reference
    → REDUCE to a generic, minimal, vendored repro   (Hermetic Extraction — §4.3)
      → root-cause
        → fix in the LOWEST reusable shared owner layer
          → add a discriminating regression test (the generic repro)
            → retest generically (RED→GREEN)
              → re-run the originating project to confirm the deviation is gone
```

### 4.2 References are not ground truth — classify every deviation

Each deviation is classified before any change:

- **Verter-bug** → fix it (closed loop above).
- **Verter-correct / reference-wrong** → assert Verter's correct behavior + **document the intentional divergence**; do NOT "fix" Verter to match a wrong baseline.

Known reference imperfections to encode up front:

- **Volar / vue-tsc**: mishandles `inheritAttrs: false` fallthrough (Verter must expose **all** component props **and** the recursive first native-child element's attributes to descendant components — see the Fallthrough / Root Inheritance CRITICAL rule); has **generics** quirks (generic SFC props instantiation).
- **`@vue/compiler-sfc`**: has its own bugs; where found, Verter must be correct and the deviation is documented (not reproduced).

### 4.3 Hermetic Extraction (CRITICAL — extends Testing-Hermeticity)

- The corpus projects are **never** referenced in committed artifacts. A deviation found in a project is **reduced to a generic minimal repro** — a vendored fixture and/or a unit test — with no project name, path, or proprietary content.
- Corpus paths live in a **gitignored local analysis config** (e.g. under `.analysis/`), consumed by the validation tooling at analysis time only.
- A test/fixture/fix that references a corpus path is a violation (mirrors `external_corpus_paths_not_present_outside_gated_tests`).

## 5. Tooling (designed in Phase 0; extends existing infra — do not reinvent)

Three comparison harnesses + an extraction pipeline, built on what already exists (the real-provider test harness, the dx-harness, the compile-parity infra, `CompileTarget::IDE`/`VDOM`/`Vapor`):

- **IDE comparison harness** (workstream A): drive Verter's LSP (tsserver + tgo) over project files at sampled mapped positions; capture results; diff vs the expected `.ts`-equivalent / Volar; classify deviations.
- **TSC comparison harness** (workstream B): run Verter's typecheck + vue-tsc over a project; normalize + diff the diagnostic sets; classify (Verter-correct vs Verter-bug).
- **Build equivalence harness** (workstream C): compile with Verter + the official compiler; compare **functional** behavior of the render functions (semantic/behavioral, not textual); document deviations.
- **Hermetic extraction pipeline**: turn a project-specific deviation into a generic vendored repro + a discriminating regression test.

> The concrete harness/pipeline architecture is detailed by the Phase-0 design effort and folded into this section on ratification.

## 6. Phasing, prioritization, and dispatch

- **Phase 0 — methodology + tooling design** (codex-validated) and a feasibility recon. **DONE** — see the Phase-0 Design Detail (§D) below.
- **Phase 1 — recon**: run the tooling over 2–3 representative projects to validate the loop and surface the first issue wave (extracted generically).
- **Phase 2 — scale**: cover all 7 projects × 3 workstreams; one fix-manager per confirmed deviation; gated by independent review + confirm.

**Prioritization** (user-set): Phase 1 lands the **P1-PIPE** foundation first (local-analysis config + corpus loader + producer-side redaction + widened leak guard + the 5-class deviation ledger), then the workstreams in **IDE → TSC → build** order. (Codex had suggested TSC-first; the user prioritizes DX/IDE — it is the headline goal and the in-flight barrel/import-resolution + import-matrix work already feeds it. TSC second, build last.)

**MoM dispatch**: the CTO decomposes into managers (recon, per-workstream analysis, hermetic-extraction, fix). Every landed fix passes independent dual review + a confirm gate. Architecture decisions are codex-owned. Corpus projects are analysis inputs only (§4.3).

**Serial execution (after Phase 0):** exactly one landing-bound block is active at a time — each lands and passes its independent confirm before the next starts. Only isolated, non-semantic harness plumbing may land in parallel. Pre-existing hermeticity violations already in committed code (hardcoded private paths flagged during the Phase-0 recon) are remediated when **P1-PIPE** lands the widened leak guard.

## 7. Definition of done

Verter is a confident drop-in replacement when, across the corpus and the generic repros derived from it:

- **A/DX**: supported IDE features match the equivalent `.ts` program on both providers, with all deviations either fixed or documented as reference-wrong.
- **B/TSC**: Verter's diagnostic set is correct (matching vue-tsc except where vue-tsc is wrong + documented).
- **C/build**: Verter's output is functionally identical to the official compiler, with every deviation documented.
- All findings are captured as generic, vendored, discriminating regression tests — no corpus references in the tree.

---

# Phase-0 Design Detail (extends §5–§6; codex-validated)

> This part is the dedicated Phase-0 design effort the charter (§5, §6, §8) points to:
> the existing-tooling inventory, the three concrete harness designs, the hermetic
> extraction pipeline + local-analysis-config mechanism, the empirical feasibility recon,
> and the MoM decomposition. It is produced read-only (no production code changed) and
> left uncommitted for CTO ratification. The codex-architect verdict is folded into §D.8.

## D.1 Inventory of existing tooling (reuse-first)

A read-only inventory (three parallel recon agents) established that **most of the
comparison substrate already exists**. The campaign is overwhelmingly *composition +
extension*, not green-field. Reuse-first is mandatory (Shared Optimized Codebase rule).

### D.1.1 DX/IDE — REUSABLE: the dx-harness + dx-baseline differential stack

- `crates/verter_dx_baseline/` — standalone Rust binary, the **sole provider owner**
  (spawns tgo/tsserver against Verter-emitted `.vue.tsx`), newline-delimited JSON protocol
  (`Hello`/`Open`/`SyncArtifacts`/`Query`/`Diagnostics`); fail-closed on missing
  providers / stale artifacts / map-absent.
- `packages/dx-harness/src/` — the TS orchestration:
  - `MaterializedWorkspace` (immutable fixture copy + test-anchor strip),
    `createMaterializedWorkspace()` / `disposeMaterializedWorkspace()`.
  - `baseline/bridgeClient.ts` — client to `verter_dx_baseline`.
  - `collectors/` — nine event collectors (completion, hover, definition, auto-import,
    diagnostics, churn, latency, logs, recovery); per-edit sampling with quiescence gates;
    JSONL emission.
  - `differential/{completion,hover,definition,diagnostics}.ts` + `projection.ts`
    (TSX↔Vue source-map projection) + `outcome.ts` (agreement/divergence classification)
    + `vueSemanticValidity.ts` (the semantic-oracle dimension).
  - `semantic-oracle/` — curated `.ts` gold-standard fact extractors + anchor→byte
    resolution; the engine for "behave like the equivalent standalone `.ts` program".
  - `report/` — `dx-events.jsonl`, `dx-summary.json`, `DX-FINDINGS.md`,
    `baseline-manifest.json`, JUnit reconciliation; **S0–S4 severity ladder**.
  - `scenario/` — YAML editing-sequence descriptors (probe method, mapping policy,
    confidence, semantic dimension); `loadScenarioCorpus()` reads
    `corpusScenariosDir()` / `corpusFixturesDir()` (= `packages/dx-harness/fixtures/hermetic`).
  - Sweep entry: `test/dxCorpusSweep.run.test.ts`, gated
    `describe.skipIf(!DX_LSP_BIN || !DX_BASELINE_BIN)`.
- `crates/verter_lsp/src/test_harness.rs` — `real_provider_test!` emits **two** variants
  (tsserver, tgo); `TestSessionBuilder` (`.fixture` / `.open_fixture_file` /
  `.open_virtual`); getters `hover_text`, `definitions`, `definition_locations`,
  `references`, `prepare_rename`, `rename_edits`, `completion_labels`, `document_symbols`,
  `signature_help`. Per-feature suites under `crates/verter_lsp/src/real_provider_tests/`.
  tgo discovery `find_tsgo_binary_canonical`; tsserver `find_tsserver` + `find_node`
  (`crates/verter_type_runtime/src/discovery.rs`). `VERTER_REQUIRE_TSGO` /
  `VERTER_REQUIRE_TSSERVER` force CI fail-closed; else graceful skip.

**Gaps (DX):** (1) **no Volar baseline** — the dx-baseline owns tgo+tsserver; the
differential today is Verter-vs-provider-on-TSX (the standalone-`.ts` oracle), not
Verter-vs-Volar. (2) **No external-project loader** — the sweep is hardwired to committed
hermetic fixtures; `DX_HARNESS_EXTERNAL_CORPUS` is a **reserved-but-inert** env name
(asserted *unset* in `hermeticFixtures.test.ts`; NO `src/` consumer) — the loader behind
it must be built. (3) Single-root-per-session workspace model (monorepo multi-root partial).

### D.1.2 TSC — REUSABLE: `verter-tsc` + the dual-tool diff script

- `crates/verter_tsc/` — the CLI: `load_tsconfig` (`tsconfig.rs`) → `VerterHost::upsert`
  per `.vue` → `generate_public_api_stubs` (cross-component `.vue.ts` stubs) →
  `generate_all_tsx` (`CompileTarget::IDE` + `TSX`, **inline base64 source maps**) →
  tgo/tsc subprocess → `reporter.rs::parse_tsc_output` → `error_map.rs::map_tsc_position`
  (remap TSX line/col → `.vue`). Defaults to tgo, falls back to tsc; `--use-tsc` forces
  tsc. Distributed via `packages/verter-tsc/`.
- `scripts/integration-test/diagnostics.mjs` — the **dual-tool comparison harness**:
  runs BOTH `vue-tsc` and `verter-tsc`, `parseTypeScriptDiagnostics` (per tool),
  `normalizeTypeCheckArtifacts` (schema `verter.typecheck-diagnostics.v1`),
  `buildDiagnosticDiff` → `{ shared, vue_only, verter_only }` (schema
  `verter.typecheck-diagnostics-diff.v1`), `buildReviewQueue`.
- `crates/verter_tsc/tests/diagnostics.rs` — fixture E2E (intentional-error `.vue` set at
  `crates/verter_tsc/tests/fixtures/diagnostics/`, pinned TS code + line/col, asserts no
  `.tsx` path leakage). `vue-tsc` is in the lockfile; real projects pin their own 3.2.x.

**Gaps (TSC):** (1) the dual-tool diff is an ephemeral CI script, not a corpus-wide
classified harness; (2) `verter-tsc` discovers tgo by walking up from the **project** dir
→ a cross-drive project silently drops to the project's `tsc` (slower / possibly different
TS major) — the harness must pin the checker per run; (3) auto-import environments
(Nuxt / `unplugin-auto-import`) need their generated ambient `.d.ts` fed to verter-tsc to
match vue-tsc — a harness-fidelity requirement (§D.3.3), distinct from a Verter bug.

### D.1.3 build — TO BUILD (pattern exists): the Vue compile-parity oracle

- **The PATTERN exists for Svelte:** `crates/verter_compiler/tests/svelte_oracle_harness.rs`
  (feature `svelte-oracle`) + the shared module `crates/verter_compiler/src/svelte_oracle.rs`
  — defines `NormalizedGolden` + `topology_diff`, the exact "functional, not byte" model
  (§D.1.4). Feature-gated out of the canonical run; re-derives a fresh normalized topology
  from the **pinned live official compiler** and `topology_diff`s against committed
  goldens, asserting zero divergence.
- **Official Vue compiler available:** `@vue/compiler-sfc` 3.5.34 is a real dependency,
  `require()`-able; `parse()`/`compileScript()`/`compileTemplate()` work today (verified).
  A version-pinned shim marker exists at `packages/dx-harness/vendor/shims/@vue/compiler-sfc/`.
- **`CompileTarget`** (`crates/verter_compiler/src/compile/types.rs`): bitflags —
  `BUNDLER = STYLE|SCRIPT|TEMPLATE` (runtime render functions, `template/code_gen/vdom/`),
  `IDE = TSX` (`ide/template/`); the two codegen paths are independent (Two Template
  Codegen Paths CRITICAL rule). Reusable corpus: 100+ `.vue` at
  `crates/verter_session/tests/component_meta_audit_corpus/fixtures/`.

**Gap (build):** there is **no Vue build-parity oracle** — no `@vue/compiler-sfc`
reference render function, no Vue `NormalizedGolden`, no `topology_diff` over VDOM
render-function structure. This is the one genuine net-new harness, built by **cloning the
Svelte oracle pattern** (§D.4.3).

#### D.1.4 `NormalizedGolden` — the concrete "functional equivalence" definition

The Svelte `NormalizedGolden` (directly adaptable to Vue VDOM output):

```
NormalizedGolden {
  slug, backend, oracle_version,
  imports:        Vec<ImportRow { source, kind, names }>,            // import SET + shape
  export_default: Option<ExportDefault>,                             // export shape
  helper_sequence: Vec<String>,                                      // ordered helper-call topology
  helper_set:      Vec<String>,                                      // helper families present
  helper_counts:   BTreeMap<String, u32>,
  templates:       Vec<TemplateSkeleton { factory, html, flag }>,    // node skeleton + patch flag
  css:             CssTopology { present, hash, code },
}
```

`topology_diff(expected, actual) -> Vec<TopologyDivergence>` compares **identity +
structure + helper-call topology** — NOT bytes. This is exactly "build functional
equivalence": two render functions are equivalent iff they import the same helper families,
call them in the same sequence with the same counts, produce the same template-node
skeletons with the same patch flags, and the same scoped-CSS topology — regardless of
variable names, whitespace, or formatting. **We must NOT make the compilers textually
identical** — only functionally so (charter §3.C).

### D.1.5 Hermetic infrastructure — REUSABLE

- `external-corpus` Cargo feature (`crates/verter_session/Cargo.toml`) gates tests reading
  `.integration-tests/repos/...`; guard `external_corpus_paths_not_present_outside_gated_tests`
  (`crates/verter_session/tests/architecture_guards.rs`) fails the suite on a non-gated
  reference; `EXPECTED_CORPUS_MIN` floor guards corpus drift.
- `.gitignore` already ignores `/target/`, `node_modules`, `.feedback/`, `.claude/...`,
  and **`.integration-tests/`** (the established gitignored external-corpus home, with
  `repos/` + `tarballs/` locally).

## D.2 Local-analysis-config mechanism

A single gitignored config is the SOLE place real project paths live:
`.analysis/projects.local.json` (under a new gitignored `.analysis/` dir, or an entry
inside the established `.integration-tests/` — CTO picks one root at ratification). Shape:

```jsonc
{
  "schema": "verter.analysis-projects.v1",
  "checkerBin": "<abs path to a pinned tgo/tsc for apples-to-apples TSC runs>",
  "projects": [
    {
      "id": "proj-a",                // OPAQUE id — used in ALL generic outputs / ledger
      "root": "<abs path>",          // gitignored-only
      "tsconfig": "<abs path or null>",
      "kind": "vite | nuxt | lib",
      "ambientDts": ["<abs path to generated auto-import .d.ts>"],   // harness fidelity
      "vueTscBin": "<abs path or null>",  // prefer project-pinned vue-tsc
      "workstreams": ["ide", "tsc", "build"]
    }
  ]
}
```

Wiring:
- **TS side (dx-harness + TSC diff):** implement the inert `DX_HARNESS_EXTERNAL_CORPUS`
  hook — when set (to the config path), `loadScenarioCorpus()` and the TSC diff loader
  read `.analysis/projects.local.json` instead of the committed hermetic fixtures. Default
  (env unset) stays byte-identical to today; the `hermeticFixtures.test.ts` assertion that
  the env is unset by default is preserved.
- **Rust side (build oracle + Rust corpus runs):** gate behind the existing
  `external-corpus` feature; the run reads the same config. The canonical run
  (`cargo nextest run --workspace` + `cargo test -p verter_session --tests`) never reads it
  and never references a project path.

Only the **opaque `id`** ever appears in JSONL events, summaries, the ledger, or any
generic output. The `id → root` mapping lives only in the gitignored config.

## D.3 Hermetic extraction pipeline

### D.3.1 The Hermetic Extraction Contract (extends charter §4.3 + Testing-Hermeticity)

> The 7 projects are **local analysis inputs only**. No project path, name, source
> excerpt, identifier, or verbatim diagnostic message may ever appear in committed code,
> fixtures, tests, commit messages, doc, or the ledger. Every deviation found against a
> real project is reduced to a **generic minimal vendored repro** before any fix, test, or
> assertion lands. The repro is authored from scratch to exhibit the *class* of the
> deviation — it must not embed any byte of the originating project.

Enforcement (incorporating codex Required #4 — producer-side redaction + widened guard):
- Project paths live only in §D.2's config; the existing guard blocks
  `.integration-tests/repos/...` literals in non-gated tests.
- **Redaction at the producer boundary.** Every harness redacts at OUTPUT-PRODUCTION time
  — before any value is written to JSONL, summary, ledger, snapshot, or log. A real path,
  identifier, or verbatim message never reaches a file unredacted; the opaque `id`
  substitution and message-shape redaction happen in the emitter, not in a post-hoc scrub.
- **Widened guard scope.** The new guard `analysis_config_paths_never_committed` scans NOT
  ONLY source files but every committed artifact class: generated goldens/snapshots, source
  maps (the `sources`/`sourcesContent` arrays — a prime leak vector), the deviation ledger
  + its JSON sidecar, committed logs/benchmark output, and any committed `.d.ts`. It flags
  any literal under the configured local-analysis root prefix, any absolute-path shape
  (drive-letter / home-dir patterns embedded by TypeScript / Vite / Nuxt), and the config
  file itself appearing tracked. (Lands with the pipeline; until then the rule names it and
  the gap is tracked in feedback.)
- **Leak vectors explicitly covered** (codex §4): logs, CI artifacts, snapshots, panic/stack
  messages, temp filenames, source-map `sources`, embedded absolute paths, generated `.d.ts`
  comments, private lockfile/package names, and copied diagnostic text in ledger notes.
- Every campaign regression test cites the generic repro fixture it characterizes, never an
  external path; ledger evidence is always generic (TS code + node kind + a hand-authored
  minimal snippet + message *shape*), never a project excerpt.

### D.3.2 Per-project run normalization

Each project is run through Verter AND the reference; results normalized into a comparable
shape, then diffed and classified:
- **TSC:** normalize each diagnostic to `(vue_file_relpath, line, col, ts_code,
  message_shape)` where `message_shape` is the TS message *template* (identifiers redacted)
  — never verbatim. Diff keyed `(relpath, line, col, ts_code)` → `shared | verter_only |
  vue_only`.
- **IDE:** for a set of mapped expression positions sampled by **anchor kind** (not source
  text), collect each provider-backed feature from Verter-LSP and from the standalone-`.ts`
  oracle, project back through the source map, diff via `differential/*`.
- **build:** compile each `.vue` with Verter `CompileTarget::BUNDLER` and with
  `@vue/compiler-sfc`, normalize both into `NormalizedGolden`, `topology_diff`.

### D.3.3 Auto-import harness fidelity (the biggest TSC correctness risk)

Nuxt / `unplugin-auto-import` / `unplugin-vue-components` projects rely on generated
ambient `.d.ts`. Both tools MUST run with the **same tsconfig**, the **same pinned
checker**, AND the project's generated ambient `.d.ts` declared to verter-tsc — else a
`TS2304` ("unresolved auto-imported name") appears for Verter that vue-tsc (with the
project's `*.d.ts`) does not see, and a harness gap gets misclassified as a Verter bug. The
config carries `ambientDts`; the harness asserts it is fed identically.

### D.3.4 Two oracles — mapping parity vs semantic lowering correctness (codex Required #3)

The Carrier IDE TS Surface Principle is the bar: a supported mapped expression position
behaves like the equivalent standalone `.ts/.tsx` program. But there are **two distinct
correctness questions**, and conflating them causes **oracle collapse** — the failure mode
codex flagged as the single biggest false-conclusion risk: the standalone-`.ts` oracle is
built from Verter's OWN SFC→TSX projection, so a lowering that is *wrong but internally
self-consistent* would pass every provider-parity check and be falsely declared correct.

The design therefore separates two oracles, run independently:

- **Oracle 1 — provider/mapping parity (intrinsic, no external truth).** Given Verter's
  emitted TSX, does each provider-backed feature behave identically whether the query enters
  via Verter-LSP (mapped back through the source map) or directly on the projected TSX (the
  dx-baseline provider)? This proves the LSP layer + source-map round-trip are faithful — it
  does NOT prove the lowering is semantically right. This is what the existing dx-harness
  `semantic-oracle/` measures.
- **Oracle 2 — SFC→TSX semantic lowering correctness (independent ground truth).** Does the
  SFC actually have the Vue/TS semantics Verter projects? This is asserted by **independent,
  hand-authored minimal repros** that state the EXPECTED Vue/TS semantics directly (e.g.
  "this prop is required and of type `boolean`", "this `v-for` item is `T`", "this slot prop
  is `{ row: T }`") via `@ts-expect-error` / positive type assertions in a curated `.ts`/
  `.vue` pair — NOT by equality against Verter's own projection. A position passes only when
  BOTH oracles agree.

Classification, when Verter and the references differ:
- Verter matches Oracle 2 (independent semantics) AND mapping parity holds, reference does
  not → **`REFERENCE_WRONG`** (assert Verter, document).
- Verter violates Oracle 2 → **`VERTER_BUG`** (lowering or resolution defect).
- Mapping parity fails but lowering is right → **`VERTER_BUG`** (LSP/source-map defect),
  distinct owner layer (`verter_lsp`) from a lowering bug.
- A defect in the harness/oracle itself (a wrong gold descriptor, a sampling error, a stale
  fixture, a provider-config mistake) → **`HARNESS_BUG` / `ORACLE_GAP`** (codex Required #1)
  — fix the harness, never the compiler.
- Genuinely ambiguous → **`UNDECIDED`** → escalate to codex with the spec/source-of-truth.

Deviation classes (charter §4.2, expanded; the 5-class taxonomy): `VERTER_BUG` (fix),
`REFERENCE_WRONG` (document + assert Verter, never regress — e.g. Volar `inheritAttrs:false`
fallthrough or generics quirks, official-compiler bugs), `INTENTIONAL_DEVIATION` (documented
behavioral choice — e.g. Verter's native-only `models`/`acceptedProps`/`rootReachability`/
`fallthroughSurface` API, or a tighter patch flag), **`HARNESS_BUG`/`ORACLE_GAP`** (an
infrastructure defect — fix the harness), `UNDECIDED` (blocks until ruled). The Fallthrough /
Root Inheritance CRITICAL rule is the fallthrough authority: `inheritAttrs:false` ⇒ no
inherited surface; single native root ⇒ intrinsic attrs minus declared props/events minus
consumed bindings; single component root ⇒ recursive propagation through the child's full
public surface; conditional branches ⇒ exact union; `class`/`style` never consumed.

### D.3.5 The deviation ledger + closed loop

A machine-readable ledger (`docs/arch/followups/replacement-deviations.md` + a JSON
sidecar):

```
{ id, workstream(ide|tsc|build),
  class(VERTER_BUG|REFERENCE_WRONG|INTENTIONAL_DEVIATION|HARNESS_BUG|UNDECIDED),
  generic_repro_fixture (REQUIRED before any landing),
  reference, oracle_ruling, owner_crate,
  regression_test (for VERTER_BUG and HARNESS_BUG), disposition, status,
  // anti-motivated-classification fields, REQUIRED for every non-VERTER_BUG ruling:
  independent_repro, source_of_truth (named spec / Vue runtime / TS behavior),
  reviewer_approval, locking_assertion }
```

**Anti-motivated-misclassification rule (codex Required #1 / §6):** a `REFERENCE_WRONG`,
`INTENTIONAL_DEVIATION`, or `HARNESS_BUG` ruling — anything that does NOT result in a Verter
fix — must carry all four: (1) an independent reproduction, (2) a named source-of-truth (TS
spec, Vue runtime behavior, or codex ruling — never "because Verter says so"), (3) explicit
reviewer approval, and (4) a regression assertion that LOCKS the intended behavior so it
cannot silently drift. This closes the loophole where a real Verter bug is waved off as
"reference is wrong".

Closed loop per deviation (charter §4.1, made concrete): **detect** (opaque id) → **reduce**
to a generic minimal vendored fixture (hermetic gate, no project bytes) → **reproduce
generically** (RED, discriminating: fails pre-change, passes post-change) → **classify** via
the two oracles (§D.3.4), escalating `UNDECIDED` to codex → **fix** (`VERTER_BUG` in the
lowest reusable owner crate, or `HARNESS_BUG` in the harness) with the regression/locking
test in the same change → **re-run generic repro** (GREEN) → **re-run the project** to
confirm. `verter_session` edits require explicit user confirmation; non-obvious owner-layer
choices route through codex.

## D.4 Harness designs (concrete)

### D.4.1 IDE comparison harness

Extend the dx-harness, do not rebuild it.
- **Corpus loader:** implement the `DX_HARNESS_EXTERNAL_CORPUS` hook (reads §D.2 config);
  materialize each project via `MaterializedWorkspace` (immutable; project never mutated);
  default run unchanged.
- **Position sampling:** sample mapped positions by **anchor kind** — interpolations
  (`{{ }}`), each directive-expression family (`v-if`/`v-for`/`v-bind`/`:`/`v-on`/`@`/
  `v-model`/`v-slot` + dynamic args), and script identifiers — exercising the full Carrier
  IDE TS Surface, not a text grep.
- **Two oracles (§D.3.4):** Oracle 1 (provider/mapping parity) = the dx-baseline provider
  (tgo AND tsserver, tgo-preferred) on the projected TSX, compared against Verter-LSP's
  mapped result; Oracle 2 (independent semantic correctness) = hand-authored minimal repros
  asserting the expected Vue/TS semantics directly. A position passes only when both agree —
  this defends against oracle collapse. Run every feature: diagnostics, hover,
  definition/type-def, references, rename, completion + resolve, signature help, document
  highlights, semantic tokens, inlay hints, mappable code-actions, AND incremental-edit
  behavior (the LSP under a churn sequence, via the existing churn collector).
- **Volar — MANDATORY captured evidence (codex Recommended #5):** a `VolarProvider` in the
  dx-baseline spawns `@vue/language-server`, normalized to the same shape, and its result is
  captured for EVERY sampled real-project position. Volar deltas are never automatic ground
  truth and never auto-classify a Verter bug — but every material Verter↔Volar delta gets an
  explicit, human-reviewed, oracle-driven classification (so incumbent-compatibility is
  measured, not ignored). This makes Volar a required evidence column, upgraded from the
  earlier "optional triage-only" framing.
- **Output:** existing JSONL + `DX-FINDINGS.md` + S0–S4 ladder, each finding carrying an
  opaque id and feeding the ledger. Fail-closed: unmapped synthetic regions / framework
  tokens / unmappable edits return framework-native or nothing — never mis-mapped.

### D.4.2 TSC comparison harness

Promote `scripts/integration-test/diagnostics.mjs` from an ephemeral CI script to a
corpus-wide classified harness.
- **Inputs:** §D.2 config (root, tsconfig, pinned checker, pinned vue-tsc, ambient `.d.ts`).
- **Runs:** `vue-tsc --noEmit` (project-pinned) and `verter-tsc --noEmit` with the SAME
  pinned checker + the project's ambient `.d.ts` (harness fidelity §D.3.3).
- **Normalize + diff:** reuse `normalizeTypeCheckArtifacts` / `buildDiagnosticDiff` →
  `{ shared, vue_only, verter_only }`, keyed `(relpath, line, col, ts_code)`, message
  redacted to shape. `verter_only` and `vue_only` are both candidate `VERTER_BUG`s until
  classified.
- **Sourcemap-leak gate:** any diagnostic remapping to a synthetic `.vue.ts`/`.tsx` path
  instead of `.vue` is itself a `VERTER_BUG` (the Options-API remap miss is the first known
  instance — §D.6).
- **Output:** review queue → ledger; opaque ids only.

### D.4.3 build comparison harness — TWO oracles (structural + runtime)

Functional equivalence is NOT defined by topology alone (codex Required #2). Topology is the
fast structural *diagnostic* oracle; a deterministic runtime DOM/update oracle is the actual
equivalence *authority*. A `.vue` is functionally equivalent iff it passes BOTH.

**Oracle A — structural topology (diagnostic, full-corpus, cheap).** Clone the Svelte oracle
pattern for Vue:
- **New module** `crates/verter_compiler/src/vue_compile_oracle.rs` reusing
  `NormalizedGolden` / `topology_diff` / `TopologyDivergence` from `svelte_oracle.rs`
  (extract the shared diff engine if schemas diverge; else reuse directly).
- **Reference half:** a Node script (mirroring `scripts/gen-svelte-goldens.mjs`) invokes
  pinned `@vue/compiler-sfc` (`compileScript` + `compileTemplate`, BUNDLER-equivalent
  options) per `.vue`, emitting the reference `NormalizedGolden`.
- **Candidate half:** Verter `CompileTarget::BUNDLER` output normalized into the same schema.
- **Diff:** `topology_diff(reference, candidate)`. A `TopologyDivergence` is a candidate
  `VERTER_BUG` unless ruled `REFERENCE_WRONG`/`INTENTIONAL_DEVIATION`/`HARNESS_BUG`. But
  topology AGREEMENT is necessary, not sufficient — it does not by itself certify behavior.
- **Schema must capture (beyond the Svelte axes), so behaviorally-significant differences are
  not normalized away:** expression-binding shape, helper *arguments* (not just helper names),
  hoist/cache-handle semantics, directive runtime payloads (the resolved directive call +
  modifiers), slot scope capture, event-modifier semantics, `v-once` / `v-memo` markers,
  dynamic-component (`<component :is>`) resolution, ref handling, and whitespace/comment mode.
  An SFC compiled for SSR vs client is a distinct golden (separate `backend`).

**Oracle B — runtime DOM/update equivalence (authority, curated fixtures).** A deterministic
runtime harness executes BOTH the Verter-compiled and the `@vue/compiler-sfc`-compiled
component (same Vue runtime, same inputs) and compares: initial rendered DOM, the DOM after a
scripted sequence of prop/state updates, emitted events, and slot output. Equivalence is
behavioral identity of rendered output + update effects — the ground truth for "functionally
identical render functions". Two topology-identical functions that diverge behaviorally are
caught here; two topology-divergent functions that render identically are ruled equivalent
here (a topology divergence with a passing runtime check is downgraded to
`INTENTIONAL_DEVIATION`, not a bug). This is the curated-fixture oracle; it need not cover the
whole corpus, but MUST cover the coverage-gate matrix below.

**Coverage gates (codex Recommended #7) — explicit curated fixtures required:** SSR vs client
output, scoped-CSS id derivation + topology, slots (incl. scoped/named/dynamic), the full
directive set + runtime payloads, cache/hoist behavior, `v-once`, `v-memo`, dynamic
components, transition/teleport/suspense, and Nuxt-style macro transforms. Each gate is a
runtime-oracle fixture; the Phase-1 gate (§D.5) asserts the matrix is populated.

**Gating:** feature-gated (`vue-compile-oracle`), out of the canonical run, like
`svelte-oracle`. Committed Vue goldens (Oracle A) + the runtime fixtures (Oracle B) are a
drift gate against the pinned official compiler; the campaign corpus runs Oracle A over the
external projects (opaque ids) under the external-corpus gate, with Oracle B run on any
topology divergence that needs a behavioral ruling.

## D.5 Decomposition & prioritization (MoM dispatch)

Two phases (mirrors charter §6), each a MoM-orchestrated block set. The CTO dispatches one
**manager** per workstream; each runs `/multi-agent-orchestration` over its blocks; the CTO
adds the cross-workstream integration gate, codex-owned classifications, and the hermetic
audit gate. All worktrees fork `fix/lsp-provider-parity`; no `verter_session` edits without
user confirmation.

### Phase 1 — recon (2–3 projects: one plain Vue+Vite, one Nuxt w/ auto-imports, one lib)

Goal: stand up the three harnesses + the extraction pipeline against the recon projects,
produce the first classified ledger, validate the hermetic pipeline end-to-end.
- **Manager P1-PIPE (foundation, gates the rest):** local-analysis-config +
  `DX_HARNESS_EXTERNAL_CORPUS` loader (TS) + external-corpus run wiring (Rust) + the
  producer-side redaction layer + the widened `analysis_config_paths_never_committed` guard
  (artifacts/snapshots/source-maps/ledger, §D.3.1) + the 5-class ledger schema (incl.
  `HARNESS_BUG` + the anti-motivated-classification fields, §D.3.5). Delivered first.
- **Manager P1-TSC:** promote the dual-tool diff to the corpus-wide classified harness;
  ambient-`.d.ts` fidelity; sourcemap-leak gate. **Fastest first signal** (verter-tsc
  already runs end-to-end against real projects).
- **Manager P1-BUILD:** clone Svelte → Vue compile oracle; pin `@vue/compiler-sfc`; commit
  a first golden set from the vendored corpus; run over the recon projects.
- **Manager P1-IDE:** external-corpus loader for the dx sweep; anchor-kind position
  sampling; full feature matrix over the standalone-`.ts` oracle on both providers;
  (optional) `VolarProvider` triage column.

Phase-1 gate: each workstream produces a classified ledger over the recon projects; the
hermetic audit passes (no project bytes/paths/names in any committed artifact — source,
golden, source map, ledger, or log, per §D.3.1); the build coverage-gate matrix (§D.4.3 —
SSR/client, scoped CSS, slots, directives, cache/hoist, `v-once`/`v-memo`, dynamic
components, Nuxt macro transforms) is populated with runtime-oracle fixtures; both build
oracles and both IDE oracles are wired (no oracle-collapse single-oracle shortcut); and codex
ratifies the classification methodology against a sample of real
`UNDECIDED`/`REFERENCE_WRONG`/`HARNESS_BUG` rows. Prioritization: **TSC first** (cheapest
end-to-end signal), **build second** (deterministic pattern-clone), **IDE third** (richest,
most infrastructure, the Volar evidence wiring); the pipeline foundation precedes all three.
(This refines charter §6's "DX/IDE and TSC lead": TSC leads outright; IDE's heavier infra
trails build in the recon phase.)

### Phase 2 — scale (7 projects × 3 workstreams)

Goal: run all 7 projects through all three harnesses, drive the closed loop to convergence,
produce the permanent ledger (every `VERTER_BUG` closed by a landed fix + discriminating
regression test on a generic repro; every `REFERENCE_WRONG`/`INTENTIONAL_DEVIATION`
documented + asserted).
- Same three workstream managers, now corpus-wide.
- **Manager P2-FIX (cross-cutting):** owns the closed loop — per `VERTER_BUG`,
  reduce→repro→root-cause→fix-in-owner-crate→regression-test→re-run, gated on dual review
  (independent reviewer + codex); per `HARNESS_BUG`, the same loop but the fix lands in the
  harness, not the compiler. **Landing discipline (codex Recommended #6):** semantic /
  compiler / shared-lowering fixes land **serially** (one landing-bound impl block at a time,
  per standing policy) to avoid rebase churn; **isolated harness-only plumbing** (collectors,
  loaders, normalization, the guard, ledger tooling) MAY land in parallel since it does not
  touch shared semantic code.
- **CTO integration gate:** after each fix batch, re-run affected harnesses (both oracles per
  workstream), run the canonical Rust gate + `pnpm test`, confirm no regression, verify the
  ledger against git state (trust-but-verify).

Prioritization within Phase 2: triage by the S0–S4 ladder (correctness-breaking first),
then by class (`VERTER_BUG` before documentation rows), then by blast radius
(lowest-owner-crate fixes that close a whole class first — the comprehensive-audit pattern).

## D.6 Feasibility recon — result (EMPIRICAL)

The closed loop **physically runs end-to-end today** (verified hermetically on this Windows
11 machine):
- `verter-tsc.exe`, `verter-lsp.exe`, `verter_dx_baseline` are built and fresh.
- **TSC loop, vendored fixture** (vue-bearing `node_modules`): exit 1, 1.25s, 68 intended
  type errors, **66/68 remapped to `.vue`**. The 2 misses leak to a synthetic `.vue.ts` —
  an **Options-API codegen sourcemap-remap gap** (first known `VERTER_BUG`, mapping class).
- **TSC loop, a real ~11-file plain-Vue+Vite project:** exit 1, 3.56s, **no hang**, 13
  diagnostics, **all 13 mapped to `.vue`**. First generic deviation classes: `TS2304`
  unresolved auto-imported names (composables/macros not declared to the checker — partly
  harness fidelity §D.3.3, partly a candidate Verter gap), `TS2339` property-not-exist,
  `TS2367` always-true comparison.
- **Oracles reachable:** `@vue/compiler-sfc` 3.5.34 `parse()` works (build); `vue-tsc`
  reachable (6.0.3 via npx; projects pin 3.2.x) (TSC); tgo at `node_modules/.bin/tsgo.cmd`
  + tsserver via discovery (IDE).

Tooling gaps surfaced (all addressed in §D.4): (1) no implemented external-corpus driver
(the `DX_HARNESS_EXTERNAL_CORPUS` hook is inert; verter-tsc has no project-set / cross-drive
config); (2) the Options-API sourcemap-remap miss; (3) no Vue build-parity oracle (Svelte
pattern to clone); (4) no Volar baseline in the dx-baseline. None is a blocker — the loop
runs and the first concrete deviations are in hand. Hermeticity: the recon left `git
status` empty, wrote no project path/name/source into the repo, described all deviations
generically.

## D.7 Risks & unknowns

- **R1 — Auto-import fidelity (TSC).** The single biggest correctness risk: without feeding
  the project's generated ambient `.d.ts` to verter-tsc identically to vue-tsc, `verter_only`
  `TS2304` noise swamps the signal and risks misclassifying a harness gap as a Verter bug.
  Mitigation: §D.3.3.
- **R2 — Volar baseline cost/value.** Wiring Volar is non-trivial and Volar is not ground
  truth. Mitigation: standalone-`.ts` oracle primary; Volar optional triage-only; gate the
  build on a codex value judgment.
- **R3 — Build "functional equivalence" boundary.** Topology alone cannot define equivalence
  (codex Required #2): topology-identical functions can differ behaviorally, and
  topology-divergent ones can be identical. The `NormalizedGolden` axes must normalize away
  benign differences (names, formatting, semantically-irrelevant ordering) while catching real
  structural divergences. Mitigation: the **runtime DOM/update oracle (§D.4.3 Oracle B)** is
  the equivalence authority — topology is only the fast structural diagnostic; a topology
  divergence with a passing runtime check is `INTENTIONAL_DEVIATION`, and topology agreement
  never alone certifies behavior. Reuse Svelte's proven axes, extended with the
  behaviorally-significant axes (helper args, hoist/cache, directive payloads, slot scope,
  modifiers, `v-once`/`v-memo`, SSR/client).
- **R4 — verter-tsc cross-drive checker drift.** Silent fallback to a project's `tsc` makes
  runs non-comparable. Mitigation: config pins `checkerBin`; the harness asserts checker
  identity per run.
- **R5 — LSP process-exit / teardown.** `verter-lsp` does not terminate promptly on the LSP
  `exit` notification (known background-teardown issue). Any harness spawning `verter-lsp`
  must force-kill after a graceful attempt (the dx-baseline / stdio-smoke pattern already
  does). Harness-robustness requirement, not a campaign blocker.
- **R6 — Reference version pinning.** vue-tsc, `@vue/compiler-sfc`, tgo, tsserver all move.
  Pin each version in the config + doc; a reference upgrade is a deliberate ledger event,
  not silent drift.
- **R7 — Project scale / perf.** Larger Nuxt projects may stress timing. Mitigation: Phase 1
  sizes 2–3 projects first; the recon showed small projects complete in single-digit
  seconds.
- **R8 — Hermetic leakage via messages.** A verbatim TS message or identifier in the
  ledger/test would violate the contract. Mitigation: message-shape redaction (§D.3.2/3) +
  the `analysis_config_paths_never_committed` guard + the pre-land checklist.

## D.8 Codex-architect validation

The codex architect (un-primed, neutral prompt; codex 0.141.0, read-only) reviewed this
methodology + tooling design for production-readiness. **Verdict: SOUND-WITH-REQUIRED-CHANGES.**
The required changes are folded into the sections above (cross-referenced inline below); the
full verbatim verdict follows.

### Required changes (blocking — all folded into the design above)

1. **Add `HARNESS_BUG` / `ORACLE_GAP` to the deviation taxonomy** — without it, real
   infrastructure defects get mislabeled as Verter or reference behavior. *(Folded into
   §D.3.4 / §D.3.5: a fifth class.)*
2. **Build functional equivalence must NOT be defined by topology alone** — add a runtime
   DOM/update comparison for curated fixtures. Two render functions can be topology-identical
   yet behaviorally different (expression binding, helper arguments, hoist/cache, directive
   runtime payloads, slot scope capture, event-modifier semantics, `v-once`/`v-memo`, dynamic
   component resolution, ref handling, whitespace/comment modes, transition/teleport/suspense,
   SSR vs client), and topology-divergent output can be behaviorally equivalent. *(Folded into
   §D.4.3: topology is the structural diagnostic oracle; a runtime DOM oracle is the equivalence
   authority.)*
3. **Separate IDE provider/mapping parity from independent validation of SFC→TSX semantic
   lowering** — the standalone-`.ts` oracle is built from Verter's OWN TSX projection, so a
   wrong-but-self-consistent lowering would pass ("oracle collapse"). Require independent
   minimal repros that assert the expected Vue/TS semantics, not only equality against
   Verter's projection. *(Folded into §D.3.4: the two-oracle split + the independent-semantics
   repro requirement.)*
4. **Redact at output-production time** and scan committed **generated artifacts, snapshots,
   source maps (`sources`), logs, panic/stack traces, and ledgers** for path/source leaks —
   not only source files. *(Folded into §D.3.1: producer-boundary redaction + the widened
   guard scope.)*

### Recommended changes (non-blocking — adopted)

5. **Volar comparison is mandatory captured evidence** for every sampled real-project position
   (human-reviewed, oracle-driven classification), never automatic ground truth. *(Adopted into
   §D.4.1: Volar promoted from "optional" to "mandatory evidence column".)*
6. **Allow parallel landing for isolated harness-only plumbing; keep semantic/compiler fixes
   serial.** *(Adopted into §D.5 Phase 2.)*
7. **Explicit fixture coverage gates** for SSR, slots, directives, cache/hoist, `v-once`,
   `v-memo`, scoped CSS, and Nuxt ambient types. *(Adopted into §D.4.3 + §D.5 Phase-1 gate.)*

### Verbatim verdict

> **Review**
>
> 1. Overall methodology is sound as a validation campaign, but it does not "prove" drop-in
>    replacement status. It can establish high confidence across chosen projects and feature
>    surfaces. The biggest false-conclusion risk is **oracle collapse**: using Verter-derived
>    TSX as the oracle can declare Verter correct when the SFC-to-TSX lowering is wrong but
>    internally self-consistent. Defense: separate "provider/mapping parity" from "Vue semantic
>    lowering correctness," and require independent minimal repros that assert the expected
>    Vue/TS semantics, not only equality against Verter's own projection.
> 2. Using standalone `.ts/.tsx` behavior as the primary IDE oracle is the right architectural
>    call for semantic truth. Volar is an incumbent, not ground truth. However, demoting Volar
>    must not mean ignoring incumbent compatibility. Volar diffs should be mandatory captured
>    evidence for all sampled real-project positions, but classification should remain
>    human-reviewed and oracle-driven. Users will compare against Volar, so every material Volar
>    delta needs an explicit classification.
> 3. `topology_diff` is a good starting oracle but is not sufficient as a definition of
>    functional equivalence. Two render functions can be topology-identical and still differ
>    behaviorally through expression binding, helper arguments, hoist/cache behavior, directive
>    runtime payloads, slot scope capture, event modifier semantics, `v-once`, `v-memo`, dynamic
>    component resolution, ref handling, whitespace/comment modes, transition/teleport/suspense
>    handling, and SSR/client divergence. Conversely, topology-divergent output can be
>    behaviorally equivalent if helpers are arranged differently but compute the same result. I
>    would require at least a second build oracle: execute compiled components in a deterministic
>    runtime harness and compare rendered DOM/updates for curated cases. Keep topology as the
>    structural diagnostic oracle, not the full equivalence definition.
> 4. The hermetic discipline is strong but not airtight. Missing leak vectors include logs, CI
>    artifacts, snapshots, panic messages, temp filenames, source-map `sources`, absolute paths
>    embedded by TypeScript/Vite/Nuxt, generated `.d.ts` comments, lockfile/package names from
>    private projects, stack traces, benchmark output, and copied diagnostic text in issue/ledger
>    notes. The guard should scan all generated artifacts and committed snapshots, not only source
>    files, and harness output should be redacted at the producer boundary.
> 5. Feeding generated ambient `.d.ts` files is the correct fidelity requirement. TypeScript sees
>    those declarations in the real project, so the harness must too. It does not mask a Verter
>    bug unless Verter claims responsibility for discovering or generating those declarations.
>    Adjudication should be explicit: if the project's normal typecheck environment includes the
>    `.d.ts`, omission is a harness gap; if Verter's CLI/LSP is intended to replace the
>    integration that discovers them, failure to load them is a Verter integration bug.
> 6. The taxonomy is almost complete, but it needs one more class: **HARNESS_BUG / ORACLE_GAP**.
>    Without it, real infrastructure defects will get mislabeled as Verter or reference behavior.
>    To avoid motivated classification, require each non-`VERTER_BUG` ruling to include:
>    independent reproduction, named spec/source-of-truth, reviewer approval, and a regression
>    assertion that locks the intended behavior.
> 7. Phase decomposition is reasonable. TSC-first is correct because it gives cheap, deterministic
>    signal and exercises shared TSX lowering. Build-second is right once the Vue oracle exists.
>    IDE-third is acceptable because it is the most expensive and most sensitive to provider noise.
>    "Fixes land serially" is wise for shared compiler/lowering changes, but could bottleneck
>    independent harness work. Use serial landing for semantic/compiler fixes; allow parallel
>    landing for isolated harness plumbing.
> 8. I would block execution until the design adds these invariants: independent build runtime
>    oracle, explicit HARNESS_BUG classification, producer-side redaction, source-map path leak
>    checks, and a clear separation between "Verter virtual-code provider parity" and "Vue
>    semantic correctness." I would also add coverage requirements for SSR, Nuxt-generated types,
>    macro transforms, scoped CSS, slots, directives, dynamic components, and incremental LSP
>    edits.
>
> **VERDICT: SOUND-WITH-REQUIRED-CHANGES**
>
> 1. **Required:** Add `HARNESS_BUG` / `ORACLE_GAP` to the deviation taxonomy.
> 2. **Required:** Do not define build functional equivalence solely by topology; add runtime
>    DOM/update comparison for curated fixtures.
> 3. **Required:** Separate IDE provider/mapping parity from independent validation of
>    SFC-to-TSX semantic lowering.
> 4. **Required:** Redact at output production time and scan committed generated artifacts,
>    snapshots, source maps, logs, and ledgers for path/source leaks.
> 5. **Recommended:** Make Volar comparison mandatory evidence, but never automatic ground truth.
> 6. **Recommended:** Allow parallel harness-only changes while keeping semantic fixes serialized.
> 7. **Recommended:** Add explicit fixture coverage gates for SSR, slots, directives, cache/hoist,
>    `v-once`, `v-memo`, scoped CSS, and Nuxt ambient types.
