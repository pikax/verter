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

`csc-web`, `babylon`, `avava`, `nexus-ui`, `judis-app`, `spotqa/frontend`, `nuxt-ui`.

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

- **Phase 0 — methodology + tooling design** (codex-validated) and a feasibility recon. *(in progress)*
- **Phase 1 — recon**: run the tooling over 2–3 representative projects to validate the loop and surface the first issue wave (extracted generically).
- **Phase 2 — scale**: cover all 7 projects × 3 workstreams; one fix-manager per confirmed deviation; gated by independent review + confirm.

**Prioritization**: DX/IDE and TSC lead (shared IDE-TSX + provider foundation, and what the in-flight barrel/import-resolution + import-matrix work already feeds); build runs as a parallel track.

**MoM dispatch**: the CTO decomposes into managers (recon, per-workstream analysis, hermetic-extraction, fix). Every landed fix passes independent dual review + a confirm gate. Architecture decisions are codex-owned. Corpus projects are analysis inputs only (§4.3).

## 7. Definition of done

Verter is a confident drop-in replacement when, across the corpus and the generic repros derived from it:

- **A/DX**: supported IDE features match the equivalent `.ts` program on both providers, with all deviations either fixed or documented as reference-wrong.
- **B/TSC**: Verter's diagnostic set is correct (matching vue-tsc except where vue-tsc is wrong + documented).
- **C/build**: Verter's output is functionally identical to the official compiler, with every deviation documented.
- All findings are captured as generic, vendored, discriminating regression tests — no corpus references in the tree.
