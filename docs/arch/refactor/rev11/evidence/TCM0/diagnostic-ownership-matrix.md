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
| **Compiler/checker (semantic + syntactic TS)** | Split: `TypeProvider::get_diagnostics` (LSP live) vs. `verter-tsc api_check.rs` (CLI) — two independent implementations today | `TypeScriptLspDirect` for the LSP live path (TypeScript answers directly against the mapped file and reports through the mapper's `DiagnosticDirectives` channel, not a Verter relay); the CLI path (`verter-tsc`) is owned by `VerterWithTypeSemanticOracle` as a batch tool. **CORRECTED 2026-08-23:** this cell previously read "keeps its own oracle-session call … since it is a batch tool, not an LSP-attached client", which is false of the tree — `HostConfig::batch_typecheck()` keeps the SAME shared `VerterHost`/resolver/cache substrate as `lsp_interactive` (`crates/verter_session/src/types.rs:1189-1191`, in-source), and both paths share `verter_tsgo_api`, toolchain discovery and `Utf16LineIndex`. The real duplication is the wire-to-DTO band, not the session — see this file's closure section | position mapped via the mapper's own span-precise output — no Verter-owned remap step for this class once TCM2/TCM3 land | `DiagnosticDirectivePolicy` (`Ignore`/`Expect`, per-directive) is the ONLY mapper-level suppression channel; an unused `Expect` is itself reported as a diagnostic (confirmed in the upstream PR text) — Verter's content mapper implementation must honor this, not invent a second suppression channel | compiler diagnostics take precedence in span-collision cases over lint (matches today's de facto behavior, now made explicit) | exact-duplicate drop only (`merge_deduplicated`-equivalent) — the two current independent implementations (LSP vs CLI) must converge onto one, and that convergence is **owned by TCM3** — `docs/arch/refactor/rev11/rulings/ARCHITECT-RULING-2026-08-24-TCM0-DECISIONS.md` Q6 rules that TCM3 already owns it through its `TypeSemanticOracle`/`VerterWithTypeSemanticOracle` diagnostic contract and that no new block is authorized; until TCM3 lands, severity taxonomy, canonical positioning and unpositionable-diagnostic behaviour remain divergent across the two paths (see this file's "Required convergence" section and `OPEN-GAPS.md`'s `G-DIAGNOSTIC-CONVERGENCE`; it is not TCM1's or TCM2's) |
| **Mapper parse/config diagnostics** (malformed `<script>`/`<template>`/tsconfig-shape issues) | `LanguageDiagnostic` (`verter_language/src/parse_artifact.rs:100-151`) → `HostDiagnostic` (`verter_session/src/parse.rs`, both Vue `:1505-1524` and Svelte `:914-938`) | `VerterNative` | attributed to the exact carrier source span the parser recorded — never to a generated position, since these diagnostics are about the SOURCE, not the projection | `blocks_compile: bool` split survives unchanged — Svelte's `strict_parse_errors` stay IDE-visible-but-non-blocking (`parse.rs:927-938`) | parse/config diagnostics take precedence over compiler diagnostics when both would fire on an unparseable region (a compiler diagnostic on unparseable content is not meaningful) | unchanged — `merge_deduplicated` already covers Vue's parse-time-vs-compile-time clone case (`virtual_file_pipeline.rs:3366-3394,3863-3882`) |
| **Directive diagnostics** (`v-if`/`v-for`/`v-model` syntactic lint) | `verter_diagnostics::rules::vue::*` (103 rule files, syntactic-only, no type info) | `VerterNative` — unchanged; these never touched TypeScript | span = the directive's own AST location | rule-level enable/disable, unchanged | independent of the compiler-diagnostic class (different spans: attribute value vs. expression content) | none needed — single producer |
| **Directive *expression* type errors** (e.g. `v-if="undefinedVar"`) | Not separately owned today — falls through to the compiler-diagnostic class once the expression is embedded in generated TSX | `TypeScriptLspDirect` (same as compiler/checker — no change in *mechanism*, only in *transport*, since the expression's span already round-trips through the mapper's per-segment span data) | mapped back through the mapper's span data, exactly like any other embedded expression — no directive-specific handling required, confirming the discovery's framing that directives are not a fifth diagnostic engine | none beyond the compiler class's own `DiagnosticDirectives` | same as compiler/checker | same as compiler/checker |
| **Framework diagnostics (Vue vs. Svelte asymmetry)** | Vue: full lint engine + parse. Svelte: parse/runtime-rejection only, no lint engine | `VerterNative` for both, unchanged split — TCM0 does **not** propose closing the Vue/Svelte lint-engine gap; that is an out-of-scope product decision, not an architecture-lock question | per-framework, as today | per-framework, as today | independent per framework (no cross-framework diagnostic ever collides on one carrier) | none needed |
| **Duplicate-class / generated-region diagnostics** | Three non-overlapping mechanisms: (a) proactive naming to avoid TS2300 in the Svelte IDE prelude (`svelte/ide/prelude.rs:262-263` — the `Snippet as __VerterSnippet`/`Attachment as __VerterAttachment` aliased imports in `COMPONENT_RUNE_IMPORTS_AND_HEADER`; corrected 2026-08-24: the previous cite `:986-992` is inside the file's `#[cfg(test)] mod tests`, which opens at `:858` — it is the test ASSERTING the aliasing, not the production mechanism); (b) CLI-only suppression of `DiagOrigin::Config` diagnostics against injected companions (`api_check.rs:136-489`); (c) LSP-side silent drop of any diagnostic whose TSX range can't map to carrier source (`merge/diagnostics.rs:95-103`) | (a) `VerterNative` (codegen discipline, unchanged); (b) folds into the oracle-session path, `VerterWithTypeSemanticOracle`; (c) **REPLACED** — a diagnostic in an unmapped/generated region must surface with **honest generated attribution**, per the charter's explicit rule, not be silently dropped as (c) does today | mechanism (c)'s silent-drop behavior is the one item this matrix flags as **non-compliant with the charter's own rule** and due for correction, not carry-forward: charter text is explicit that a generated diagnostic with no valid authored projection "stays visible with honest generated attribution... never mapped to a convenient false position" — today's code instead drops it entirely, which is neither "visible with honest attribution" nor "mapped falsely," but a third, uncharted behavior (silent loss) the charter's own rule does not sanction as a legal end-state | n/a — this is corrected, not preserved | generated-region diagnostics rank below carrier-mapped diagnostics for the SAME span (a generated one only surfaces when no carrier-mapped alternative exists) | n/a |
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

## Required convergence — owned by TCM3, and divergent until TCM3 lands

The two-implementation compiler-diagnostic duplication (LSP live path vs. `verter-tsc` CLI path) is
named here and cross-referenced in `deletion-closure.md` — TCM0 records it as a required convergence,
not a decision it can execute itself (no production code changes in this block). It is **not** TCM1's or
TCM2's: `TCM1.md` forbids semantic-API clients, and `TCM2.md` assigns semantic-session ownership to
TCM3.

**Owner, settled.** `docs/arch/refactor/rev11/rulings/ARCHITECT-RULING-2026-08-24-TCM0-DECISIONS.md`
Q6: **TCM3 already owns this convergence** through its `TypeSemanticOracle` and
`VerterWithTypeSemanticOracle` diagnostic contract, and **no new block is authorized** for it. The
consequence the ruling states, and that this matrix therefore discloses rather than repairs: **until TCM3
lands, severity taxonomy, canonical positioning and unpositionable-diagnostic behaviour remain DIVERGENT
across the CLI and oracle paths.** That divergence is a disclosed present-tense fact about the tree, not
an unowned gap. See `OPEN-GAPS.md`'s `G-DIAGNOSTIC-CONVERGENCE` row.

## Closure, 2026-08-23: what the two compiler-diagnostic paths actually share (`G-DIAGNOSTIC-CONVERGENCE`)

The "Open item" section above records a required convergence between the LSP live path and the
`verter-tsc` CLI path, and `OPEN-GAPS.md`'s `G-DIAGNOSTIC-CONVERGENCE` row deliberately left its owner
unassigned. A source-level investigation of both paths has now been run. It does not settle the owner,
but it does something more useful: **it shows the duplication is not where this matrix says it is**, and
it finds the blocker named in the source itself.

### Correction: the matrix's premise about the CLI path is wrong

This matrix's compiler/checker row states that the CLI path *"keeps its own oracle-session call
(`VerterWithTypeSemanticOracle`) since it is a batch tool, not an LSP-attached client"*. That is not true
of the tree today. The two paths already share every layer the Shared Optimized Codebase rule is about:

| Shared layer | Evidence |
|---|---|
| `VerterHost` / resolver / cache substrate | `crates/verter_session/src/types.rs:1189-1191` states it in-source: `HostConfig::batch_typecheck()` *"Keeps the SAME shared `VerterHost` / resolver / cache substrate as [`Self::lsp_interactive`]"*. CLI builds it at `crates/verter_tsc/src/checker.rs:626`,`:730`. |
| tsgo `--api` client crate | Both depend on `verter_tsgo_api` (`crates/verter_tsc/Cargo.toml:33`, `crates/verter_type_runtime/Cargo.toml:26`) and both consume the same wire type `verter_tsgo_api::proto::types::Diagnostic`. |
| engine discovery / toolchain policy | `verter_tsgo_api::toolchain::discovery` — CLI `checker.rs:1082`, LSP `crates/verter_lsp/src/main.rs:847`,`:1057`. |
| IDE-companion codegen product | CLI `checker.rs:344` `CompileProduct::IdeCompanion`; LSP `crates/verter_lsp/src/documents/mod.rs:298` `CompileTarget::IDE \| TEMPLATE_DATA`. |
| UTF-16 offset primitive | `verter_span::Utf16LineIndex` — CLI `crates/verter_span/src/diag_source.rs:79-81`, LSP `crates/verter_type_runtime/src/tsgo/owned.rs:266`. |

Also worth stating plainly, because the matrix's "new owner" column reads as a description of code:
**`TypeScriptLspDirect` and `VerterWithTypeSemanticOracle` have zero occurrences anywhere under
`crates/`.** They are target vocabulary from this program's own documents, so no reading of the current
code can be justified by citing them.

### The duplication that is actually there

Both paths map the *same* wire type, through two independent functions with two divergent output
contracts:

| Concern | `verter-tsc` | LSP |
|---|---|---|
| wire `Diagnostic` → DTO | `crates/verter_tsc/src/api_check.rs:490 map_one` | `crates/verter_type_runtime/src/tsgo/owned.rs:367 map_api_diagnostic` |
| severity from `d.category` | `api_check.rs:514` — `1 ⇒ Error`, `0 ⇒ Warning`, **everything else dropped** | `owned.rs:387-392` — `1 ⇒ Error`, `0 ⇒ Warning`, `2 ⇒ Hint`, `_ ⇒ Info` |
| position contract | `api_check.rs:581 line_col_via_cache` → 1-based `(line, col)` | `owned.rs:377-383 byte_for_utf16` → byte offsets |
| carrier remap | `crates/verter_tsc/src/error_map.rs:25 map_tsc_position` (`oxc_sourcemap`, line/col) | `crates/verter_lsp/src/type_provider/merge/diagnostics.rs:53 tsx_range_to_carrier_range` (`ProviderPositionMapper`, bytes) |
| unpositionable diagnostic | **hard error** `api_check.rs:437-444` | **silently dropped** `merge/diagnostics.rs:99-106` |
| tags / related-information | absent | **neither named function populates these.** `map_api_diagnostic` hardcodes `tags: Vec::new(), related_information: Vec::new()` (`owned.rs:397-398`); the DTO fields are merely declared at `crates/verter_type_runtime/src/protocol.rs:612-628`. They are populated on the `--lsp` path at `crates/verter_type_runtime/src/tsgo/ipc.rs:1496` and `crates/verter_type_runtime/src/tsserver/ipc.rs:1364`. CORRECTED 2026-08-23 |
| dedup / metadata union | absent | `crates/verter_lsp/src/tsgo/composite.rs:533`, `merge/diagnostics.rs:113`,`:134` |
| Vue-JSX gap suppression | `crates/verter_tsc/src/checker.rs:1792`, called `api_check.rs:523` | absent |
| config-file diagnostics | `api_check.rs:373` | absent — the LSP never calls `get_config_file_parsing_diagnostics` |

So "converge onto one" has a concrete, checkable meaning that this matrix did not previously state:
**delete one of `api_check.rs:490` / `owned.rs:367` so a single function maps
`verter_tsgo_api::proto::types::Diagnostic` into a single DTO** carrying byte ranges and the four-value
severity taxonomy, with the CLI projecting to `(line, col)` and its two-value severity at its own reporter
boundary. **Corrected 2026-08-23:** an earlier revision listed "tags, related-information" among the
richer contract the surviving function carries. It does not — `owned.rs:397-398` hardcodes both empty, so
tags and related-information are a THIRD concern, populated only on the `--lsp` path
(`tsgo/ipc.rs:1496`, `tsserver/ipc.rs:1364`) and neither produced nor preserved by either named mapper.
Converging the two mappers does not by itself give the CLI tags or related-information; that is part of
the same `--api`-vs-`--lsp` parity gap identified as the blocker below
(`crates/verter_tsc/src/reporter.rs:74`,`:115`) rather than at the wire boundary. The divergent
unpositionable-diagnostic policies (hard error vs silent drop) must be reconciled in the same change;
note that the silent drop is independently flagged in this matrix's duplicate-class row as
non-compliant with the charter's own rule, so the reconciliation direction is already decided.

### The blocker is named in the source, and it is not a TCM concern

`crates/verter_type_runtime/src/tsgo/owned.rs:503-508`, in-source, states what stands in the way:

> the attached `--api` checker is the TYPECHECK / membership / reflection ORACLE …; promoting it to the
> sole user-facing diagnostics surface requires closing its per-carrier program parity with the `--lsp`
> program (the `vue`/JSX/tag/suggestion gaps) and is a full-DX-contract concern, not this provider's job.

`TsgoOwnedProvider::get_diagnostics` (`owned.rs:509`) accordingly delegates to `self.lsp.get_diagnostics(path)`
— the `--api` surface is deliberately *not* the diagnostics authority there. Partial convergence has
nonetheless already shipped on one lane: the SHARED composite runs `--api` in production and composes it
with an `--lsp` pull (`crates/verter_lsp/src/tsgo/composite.rs:279`,`:533`).

This is the finding that matters for ownership. The precondition for convergence is a **`--api`-vs-`--lsp`
program-parity gap in severity taxonomy, tags, related-information and suggestion diagnostics** — a gap
that exists today, is described in-source as a full-DX-contract concern, and would exist unchanged if the
TCM content-mapper program were cancelled tomorrow. It is not created by, blocked on, or resolved by
anything TCM1-TCM4 do: TCM1 forbids semantic-API clients, TCM2 assigns semantic-session ownership to
TCM3, and TCM3's own scope is the semantic *capability* plane, not the `--api`/`--lsp` parity of the
underlying tsgo programs.

**What TCM0 established, and what the ruling then decided.** TCM0's own contribution is the packet: the
convergence is scoped (one of two named functions is deleted), and its precondition — a
`--api`-vs-`--lsp` program-parity gap in severity taxonomy, tags, related-information and suggestion
diagnostics — is identified and cited in the source. TCM0 read that precondition as orthogonal to the
content-mapper program and therefore declined to name any TCM block as owner.

**That conclusion is superseded.** `docs/arch/refactor/rev11/rulings/ARCHITECT-RULING-2026-08-24-TCM0-DECISIONS.md`
Q6: TCM3 **already owns** the convergence through its `TypeSemanticOracle` and
`VerterWithTypeSemanticOracle` diagnostic contract, and **no new block is authorized** — the question is
not a program-level scoping decision awaiting an answer. The parity gap cited above remains a true finding
about the tree and an input TCM3 inherits; it is not a reason to leave the convergence unowned. Until TCM3
lands, severity taxonomy, canonical positioning and unpositionable-diagnostic behaviour **remain divergent
across the CLI and oracle paths** — disclosed here, repaired by TCM3.

### One further correction, for accuracy about the topology

`TypeProvider::get_diagnostics` (`crates/verter_type_runtime/src/traits.rs:210`) is a trait method with
**nine production implementations** across three wire protocols, not "an implementation". Describing it
as one half of a two-way duplication undercounts the real topology; the nine are enumerated in
`feature-ownership-ledger.md`'s own impl list.
