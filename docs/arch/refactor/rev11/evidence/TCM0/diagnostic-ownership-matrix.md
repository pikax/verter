# TCM0 §4 — Diagnostic ownership matrix

Scope: charter item 4. Built from a direct inventory of the pre-TCM diagnostic architecture (every
citation below is file:line, independently verified) plus the wire mechanism the candidate package
actually ships (`DiagnosticDirectives`/`DiagnosticDirectivePolicy`, `package-lock-and-semantic-api.md`
§3). The governing rule, restated from the charter: **a generated diagnostic without a valid authored
projection stays visible with honest generated attribution — it is never remapped to a convenient false
position.**

## What exists today (baseline, so the matrix below is a stated delta, not a guess)

Two independent TypeScript-diagnostic paths exist right now, not one:

1. **LSP live path** — `TypeProvider::get_diagnostics` (`crates/verter_type_runtime/src/traits.rs:210`),
   two engine impls (`tsgo/ipc.rs:3378`, `tsserver/ipc.rs:3810`), merged with Verter's own diagnostics in
   `sync_coordinator.rs:1298-1430` and mapped back through `type_provider/merge/diagnostics.rs`.
2. **Standalone `verter-tsc` CLI checker** — `crates/verter_tsc/src/api_check.rs:210` (`typecheck`),
   independent suppression rules (`:136-156,396-489`).

Vue and Svelte are **not diagnostically symmetric today**: Vue has a 103-rule lint engine
(`crates/verter_diagnostics/src/rules/vue/*`) plus parse diagnostics; Svelte has **no lint-rule
directory at all** (`find crates/verter_diagnostics/src/rules -iname "*svelte*"` → empty) — only
parser/runtime-rejection diagnostics via `crates/verter_session/src/parse.rs:914-938`. Any matrix that
assumes symmetry does not match the code and must not be adopted.

Existing dedup/precedence mechanisms, narrow but real: `DiagnosticsSnapshot::merge_deduplicated`
(exact-duplicate drop, `verter_session/src/types.rs:2543-2551`), `same_mapped_diagnostic`/
`merge_mapped_diagnostic_metadata` (identical-mapped-range union,
`type_provider/merge/diagnostics.rs:123-163`), an explicit lint-code allowlist when no lint config is
present (`server_utils.rs:1788-1810`), and deterministic display ordering
(`sort_host_diagnostics`, `types.rs:2566-2578`). There is **no general N-source arbitration policy**
beyond these — everything else concatenates today.

## The matrix

| Diagnostic class | Current owner (pre-TCM) | New owner | Attribution rule | Suppression | Precedence vs. other classes | Dedup rule |
|---|---|---|---|---|---|---|
| **Compiler/checker (semantic + syntactic TS)** | Split: `TypeProvider::get_diagnostics` (LSP live) vs. `verter-tsc api_check.rs` (CLI) — two independent implementations today | `TypeScriptLspDirect` for the LSP live path (TypeScript answers directly against the mapped file and reports through the mapper's `DiagnosticDirectives` channel, not a Verter relay); the CLI path (`verter-tsc`) keeps its own oracle-session call (`VerterWithTypeSemanticOracle`) since it is a batch tool, not an LSP-attached client | position mapped via the mapper's own span-precise output — no Verter-owned remap step for this class once TCM2/TCM3 land | `DiagnosticDirectivePolicy` (`Ignore`/`Expect`, per-directive) is the ONLY mapper-level suppression channel; an unused `Expect` is itself reported as a diagnostic (confirmed in the upstream PR text) — Verter's content mapper implementation must honor this, not invent a second suppression channel | compiler diagnostics take precedence in span-collision cases over lint (matches today's de facto behavior, now made explicit) | exact-duplicate drop only (`merge_deduplicated`-equivalent) — the two current independent implementations (LSP vs CLI) must converge onto one, tracked as a TCM1/TCM2 deletion (see deletion-closure.md) |
| **Mapper parse/config diagnostics** (malformed `<script>`/`<template>`/tsconfig-shape issues) | `LanguageDiagnostic` (`verter_language/src/parse_artifact.rs:100-151`) → `HostDiagnostic` (`verter_session/src/parse.rs`, both Vue `:1505-1524` and Svelte `:914-938`) | `VerterNative` | attributed to the exact carrier source span the parser recorded — never to a generated position, since these diagnostics are about the SOURCE, not the projection | `blocks_compile: bool` split survives unchanged — Svelte's `strict_parse_errors` stay IDE-visible-but-non-blocking (`parse.rs:927-938`) | parse/config diagnostics take precedence over compiler diagnostics when both would fire on an unparseable region (a compiler diagnostic on unparseable content is not meaningful) | unchanged — `merge_deduplicated` already covers Vue's parse-time-vs-compile-time clone case (`virtual_file_pipeline.rs:3366-3394,3863-3882`) |
| **Directive diagnostics** (`v-if`/`v-for`/`v-model` syntactic lint) | `verter_diagnostics::rules::vue::*` (103 rule files, syntactic-only, no type info) | `VerterNative` — unchanged; these never touched TypeScript | span = the directive's own AST location | rule-level enable/disable, unchanged | independent of the compiler-diagnostic class (different spans: attribute value vs. expression content) | none needed — single producer |
| **Directive *expression* type errors** (e.g. `v-if="undefinedVar"`) | Not separately owned today — falls through to the compiler-diagnostic class once the expression is embedded in generated TSX | `TypeScriptLspDirect` (same as compiler/checker — no change in *mechanism*, only in *transport*, since the expression's span already round-trips through the mapper's per-segment span data) | mapped back through the mapper's span data, exactly like any other embedded expression — no directive-specific handling required, confirming the discovery's framing that directives are not a fifth diagnostic engine | none beyond the compiler class's own `DiagnosticDirectives` | same as compiler/checker | same as compiler/checker |
| **Framework diagnostics (Vue vs. Svelte asymmetry)** | Vue: full lint engine + parse. Svelte: parse/runtime-rejection only, no lint engine | `VerterNative` for both, unchanged split — TCM0 does **not** propose closing the Vue/Svelte lint-engine gap; that is an out-of-scope product decision, not an architecture-lock question | per-framework, as today | per-framework, as today | independent per framework (no cross-framework diagnostic ever collides on one carrier) | none needed |
| **Duplicate-class / generated-region diagnostics** | Three non-overlapping mechanisms: (a) proactive naming to avoid TS2300 in the Svelte IDE prelude (`svelte/ide/prelude.rs:986-992`); (b) CLI-only suppression of `DiagOrigin::Config` diagnostics against injected companions (`api_check.rs:136-489`); (c) LSP-side silent drop of any diagnostic whose TSX range can't map to carrier source (`merge/diagnostics.rs:95-103`) | (a) `VerterNative` (codegen discipline, unchanged); (b) folds into the oracle-session path, `VerterWithTypeSemanticOracle`; (c) **REPLACED** — a diagnostic in an unmapped/generated region must surface with **honest generated attribution**, per the charter's explicit rule, not be silently dropped as (c) does today | mechanism (c)'s silent-drop behavior is the one item this matrix flags as **non-compliant with the charter's own rule** and due for correction, not carry-forward: charter text is explicit that a generated diagnostic with no valid authored projection "stays visible with honest generated attribution... never mapped to a convenient false position" — today's code instead drops it entirely, which is neither "visible with honest attribution" nor "mapped falsely," but a third, uncharted behavior (silent loss) the charter's own rule does not sanction as a legal end-state | n/a — this is corrected, not preserved | generated-region diagnostics rank below carrier-mapped diagnostics for the SAME span (a generated one only surfaces when no carrier-mapped alternative exists) | n/a |
| **External-unit diagnostics** (`<script src>`/`<template src>`/`<style src>`) | `ExternalSourceRequest`/`ExternalBlockKind` (`verter_session/src/types.rs:1643-1678`); attribution split in `resolve_related_location` (`merge/diagnostics.rs:233-284`) between carrier-IDE paths (mapped through CodeTransform) and "every other target" (real file, real URI, never remapped onto the `.vue`/`.svelte` file) | unchanged split, `VerterNative` for the routing decision + `TypeScriptLspDirect`/`VerterWithTypeSemanticOracle` for the diagnostic content itself depending on which side of the split a given diagnostic falls | already correct today — an external file's diagnostics attribute to the external file's own URI, never the owning SFC; TCM0 finds no defect here | none beyond the owning class's own rule | external-unit diagnostics never collide with the owning SFC's diagnostics (disjoint URIs) | none needed |

## Precedence / dedup — the terminal ruling this matrix locks

Restated as one ordered rule set, since today's code has several narrow special cases but no single
stated policy (per the diagnostics investigation's finding: "no general N-source arbitration policy...
anything else simply concatenates"):

1. Parse/config diagnostics on unparseable content suppress compiler diagnostics on the same span (a
   compiler diagnostic about broken syntax is not independently meaningful).
2. Carrier-mapped diagnostics (any class that resolves to an authored source span) always rank above a
   generated-region diagnostic on the same or overlapping span.
3. A generated-region diagnostic with no authored counterpart on its span is never dropped and never
   remapped to a false position — it surfaces tagged as generated, per the charter's explicit rule. This
   is the ONE point where TCM0 records a required correction to current behavior (mechanism (c) above),
   not a preservation of the status quo.
4. Exact-duplicate diagnostics (identical severity+code+message+span+arguments) from two independent
   producers collapse to one, per the existing `merge_deduplicated` rule, extended to cover the
   TypeScript-diagnostic class once its two current implementations (LSP live vs. CLI) converge.
5. Identical-mapped-range diagnostics from different classes (e.g. a lint diagnostic and a compiler
   diagnostic that both resolve to the exact same carrier range) union their metadata (tags,
   related-information) rather than displaying twice, per the existing `same_mapped_diagnostic` rule.
6. Anything not covered by 1-5 concatenates, unchanged from today.

## Open item carried to TCM1/TCM2

The two-implementation compiler-diagnostic duplication (LSP live path vs. `verter-tsc` CLI path) is
named here and cross-referenced in `deletion-closure.md` — TCM0 records it as a required convergence,
not a decision it can execute itself (no production code changes in this block).
