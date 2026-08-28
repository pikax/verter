# Framework-Aware Import Placement — Design (DEFERRED)

## 1. Status

**DEFERRED — codex-architect-recommended design, NOT yet implemented.** This document
captures the authoritative recommended design so that the future implementer adopts it
rather than reinventing it. No code in this change implements it; this file is design-only.

**Pickup window:** the B.7 / U14 timeframe, or a dedicated narrow framework-adapter
substrate block. The work is a small, self-contained substrate repair that the later B.7
(candidate sources + auto-imports) and B.8 (organize-imports) blocks then *consume* — it is
not a B.7/B.8 deliverable in itself.

**Deferral rationale.** The design respects the unified plan's two governing constraints:

- **U14 is "re-wire, not build."** U14 re-wires the already-merged framework-adapter
  substrate onto the U11/U13 public surfaces; it explicitly forbids building new adapter
  machinery from scratch. The import-placement capability is *new* substrate, so it is
  sequenced as its own narrow repair adjacent to U14 rather than smuggled into the re-wire.
- **`verter_session` is the highest-regression surface.** The plan flags `verter_session`
  (and parse-artifact extensions, which touch cache invalidation) as the greatest regression
  risk. This work touches `verter_session::framework`, `verter_language::parse_artifact`, and
  the Vue/Svelte compiler producers — so it is captured precisely and kept narrow (parse-layout
  metadata + a registry capability + Vue/Svelte impls + LSP consumer re-wiring + guards), not
  expanded.

> Codex's own sequencing verdict is stronger than "defer wholesale": it recommends amending
> the plan *now* with a narrow substrate repair, on the grounds that near-term completion stays
> delegated to tsserver/tsgo and that delegated path must already place edits correctly for
> supported SFCs. This document records that recommendation faithfully; the decision to *schedule*
> it (now vs. at the B.7/U14 window) is the open sequencing question — see §7. Per the
> `verter_session`-confirmation rule, the actual edit to `verter_session` must be confirmed with
> the user before implementation.

## 2. Problem

Import placement today is **Vue-`<script setup>`-hardcoded AND string-based**, even though the
transport/dispatch layer underneath it is already framework-agnostic (typed carriers reach
`resolve_completion`). The placement layer is the lone framework-specific island:

- `crates/verter_lsp/src/type_provider/auto_import.rs`:
  - `resolve_script_import_anchor` (≈ lines 110–151) calls `scan_sfc_blocks` (a raw-source
    SFC scanner) and branches on `b.is_setup()` / `b.tag_name == "script"`.
  - The synthesized block is a literal string: `"<script setup lang=\"ts\">\n"` (line 144),
    `"<script setup lang=\"{lang}\">\n"` (line 141), `"<script setup>\n"` (line 143), with a
    literal `"</script>\n\n"` close tag (line 149).
  - `ScriptImportInsertionAnchor` (lines 45–93) is a Vue-only two-variant enum
    (`ExistingScriptSetup` / `CreateScriptSetup`) whose doc comments are explicitly
    "Volar parity" for `<script setup>`.
- `crates/verter_lsp/src/documents/sfc_scanner.rs`: `scan_sfc_blocks` (line 301), `SfcBlock`,
  `is_setup` (line 52 — `self.tag_name == "script" && self.attr("setup").is_some()`),
  `content_range` (line 30), `lang` (line 57). This is a raw byte scanner over the carrier
  source.
- `crates/verter_lsp/src/server/nav_features_completion_resolve.rs` (line 90) calls
  `resolve_script_import_anchor(&doc.source, …)` directly, with Vue/`<script setup>`-specific
  comments (lines 28–60).
- `crates/verter_lsp/src/server/sync_orchestration.rs` (lines 163–164) calls
  `resolve_script_import_anchor` and then matches on
  `ScriptImportInsertionAnchor::ExistingScriptSetup`, so component auto-import only ever targets
  an existing `<script setup>` block.

**Consequence.** A `.svelte` carrier (whose instance script is `<script lang="ts">`, never
`<script setup>`) and a plain-`<script>` Vue component get **no, or invalid, auto-import**: the
hardcoded path either fails to find an anchor or synthesizes a Vue `<script setup>` block into a
non-Vue / non-setup source. The transport already routes these carriers correctly; only
placement is wrong. This recreates exactly the per-framework-branching bug the framework-adapter
substrate exists to prevent.

## 3. Recommended Design (codex, faithful)

A **framework-neutral import-placement capability registered through
`FrameworkAdapterRegistry`.** The descriptor advertises the capability/status; the registration
carries the executable adapter implementation. Shared LSP completion-resolve only ever asks the
registry-selected framework adapter for an **original-source edit plan** — it never inspects
framework identity, file extension, tag names, or raw source text to select/find/create script
blocks.

### 3.1 The trait + outcome (new shared capability)

New module under the substrate: `crates/verter_session/src/framework/import_placement.rs`,
re-exported from `crates/verter_session/src/framework/mod.rs`.

```rust
pub trait FrameworkImportPlacement: Send + Sync {
    fn plan_import_placement(
        &self,
        cx: FrameworkImportPlacementCx<'_>,
        request: ImportPlacementRequest<'_>,
    ) -> ImportPlacementOutcome;
}
```

`ImportPlacementOutcome` is a typed, exhaustive result — **no silent fallback, no panic, no
dropped edit pretending success**:

```rust
enum ImportPlacementOutcome {
    Supported(ImportPlacementPlan),
    Partial { plan: ImportPlacementPlan, reason: ImportPlacementReason },
    Unsupported { reason: ImportPlacementReason },
    InvalidSource { diagnostics: Vec<_> },
}
```

The `ImportPlacementPlan` describes **original-source edits**, produced through `CodeTransform`
over the original SFC/carrier source and then converted by the LSP into `TextEdit`s. The
capability must NOT return "a tag string plus a byte offset" from LSP code — the LSP-side
synthesis of `<script setup …>` is exactly what is being removed.

> Open (codex named the trait/types but did not fully specify their fields): the precise field
> set of `FrameworkImportPlacementCx<'_>`, `ImportPlacementRequest<'_>`, `ImportPlacementPlan`,
> and `ImportPlacementReason`, and whether `InvalidSource.diagnostics` reuses
> `verter_language::LanguageDiagnostic`. The implementer derives these from the consumer call
> sites; this doc pins the *shape and contract*, not the exact struct fields. The cx must remain
> facts/carrier-only in spirit (it reads parsed carrier block regions + AST import spans; it does
> not resolve types).

### 3.2 Descriptor capability column

`crates/verter_session/src/framework/descriptor.rs` — add an explicit edit-capability status
column, e.g. `AutoImportPlacement`, using the existing `SUPPORTED / PARTIAL / UNSUPPORTED`
style already used for surface kinds. **Do not overload `FrameworkSurfaceKind`**; create a small
dedicated edit-capability status if one is needed. (The descriptor today carries
`supported_surfaces: &'static [FrameworkSurfaceKind]`, `carrier_language`, and the
`VirtualFileNaming` column — this adds one more capability column alongside those.)

### 3.3 Registry registration leg + completeness

`crates/verter_session/src/framework/registry.rs` — add an **optional `import_placement`
registration leg** on the registration row (alongside the existing optional carrier leg, synth,
api-projector, and script-fact providers). Registry completeness tests must require:

- `SUPPORTED` ⇒ a registered placement implementation is present.
- `PARTIAL` ⇒ a registered placement implementation is present.
- `UNSUPPORTED` ⇒ the absence stays **explicit** (no implementation, declared, never an
  accidental gap).

This mirrors the existing `framework_registry_complete` discipline (every wire tag maps to a
registered adapter or an explicit disposition row).

### 3.4 Neutral parse-artifact layout (the load-bearing addition)

`crates/verter_language/src/parse_artifact.rs` — the current `ScriptRegion` carries only the
**content** `span` plus `source_type` and `kind` (`ScriptRegionKind { Instance, Module,
Frontmatter }` already exists). Codex's verdict: that content-only span **is insufficient for
AST-correct block creation**. Extend the neutral layout with:

- script **content span** (already present as `ScriptRegion.span`);
- **open-tag span**;
- **close-tag span** (when present);
- **block span** (whole block including tags);
- **root ordering** (the source order of root-level regions, so a synthesized block lands at a
  parser-derived root insertion slot, not a guessed offset `0`);
- **script role** `Instance | Module | Frontmatter` (already present as `ScriptRegionKind` —
  reuse it; the role is what distinguishes Vue `<script setup>` (Instance) from plain
  `<script>` (Module), and Svelte instance (Instance) from `<script context="module">`
  (Module)).

> Open: codex says "Add neutral root/block layout data" without prescribing the exact struct
> name/shape. The implementer decides whether these new spans extend `ScriptRegion` in place or
> live in a sibling root-layout struct on `FrameworkParseCommon`. The contract is: **the data is
> framework-neutral, carrier-file-absolute, and complete enough that placement never reconstructs
> a tag or block boundary by raw string search.** Because this struct is part of the cached
> parse artifact, the implementer must check cache-invalidation / parser-version implications
> (the plan's named regression risk for parse-artifact extensions).

### 3.5 Producers populate the neutral layout

- `crates/verter_compiler/src/framework_common/vue_bridge.rs` — populate the new neutral
  script/root layout fields from Vue's parsed SFC nodes.
- `crates/verter_compiler/src/svelte/carrier.rs` — populate the same fields for Svelte instance
  and module scripts.
- `crates/verter_compiler/src/svelte/parser/template_ast.rs` — if needed, retain close-tag /
  block spans in `SvelteScript` so placement never reconstructs them by raw string search.

### 3.6 LSP delegates, and the legacy island is deleted

- `crates/verter_lsp/src/type_provider/auto_import.rs` — **remove `resolve_script_import_anchor`,
  remove `ScriptImportInsertionAnchor`, drop the dependency on `scan_sfc_blocks`.** Keep the
  provider edit *classification* (the `is_preamble_import_insertion` / mapped-vs-unmapped routing
  that decides *which* edits are unmapped preamble auto-imports), but delegate the unmapped
  preamble imports to the session/framework placement API for actual placement.
- `crates/verter_lsp/src/server/nav_features_completion_resolve.rs` — replace the local
  carrier-source placement with a call into the session placement capability; remove the
  Vue/setup-specific comments and behavior.
- `crates/verter_lsp/src/server/sync_orchestration.rs` — route component auto-import placement
  through the same capability. If create-script behavior is intentionally disallowed on that path,
  make it an **explicit request flag**, not an accidental limitation baked into a
  `matches!(…, ExistingScriptSetup)` filter.
- `crates/verter_lsp/src/documents/sfc_scanner.rs` — **leave for symbols / folding only**; it
  must not participate in semantic import placement.

This is a clean cutover with explicit legacy deletions (no dual path, no shim), consistent with
the repo's "delete the superseded code in the same change" rule.

## 4. Full Contract / Requirements

The implemented capability must satisfy every requirement below. These are the acceptance
contract, not suggestions.

1. **AST-driven, no string semantics.** Anchors come from **parsed carrier block regions plus
   AST import spans** — never raw tag scanning, never `starts_with("<script")`, never
   substring extension/name sniffing. This is the codebase's no-string-for-semantic-logic rule.
   Per its TWO-GATE escalation, if a requirement *appears* to need text manipulation inside the
   placement path, the implementer escalates **agent → codex-architect → user** rather than
   reintroducing a scanner; the fix is to lower the right parse data, not to pattern-match text.
2. **All script-block kinds.** Vue `<script setup>` (Instance) AND plain `<script>` (Module);
   Svelte instance `<script>` (Instance) AND `<script context="module">` / `<script module>`
   (Module); future frameworks (Astro frontmatter, etc.) added as new rows.
3. **Three placement states**, chosen by the adapter:
   - **Insert into the matching existing script block** (the role-correct block exists).
   - **Insert into an appropriate other existing block** only when framework semantics make that
     correct.
   - **Synthesize the framework-correct block when absent**, at a parser-derived root insertion
     slot — including the **template-triggered create** case (a completion accepted from the
     template with no script block yet creates the AST-correct block).
4. **Capability differences are explicit.** Differences between frameworks surface through the
   `SUPPORTED / PARTIAL / UNSUPPORTED` status machinery and the typed outcome — never as silent
   gaps or "it happened to do nothing."
5. **No favoritism.** One framework-neutral shared path. No per-framework hardcode in shared
   LSP/type-provider code. Vue is a registered adapter row like any other, not a privileged
   branch.
6. **Edits via `CodeTransform`, mapped to original-source coordinates.** The plan produces
   `CodeTransform` edits over the ORIGINAL carrier source; the LSP converts them to `TextEdit`s.
   No post-`build_string()` string surgery (the CodeTransform-is-single-source-of-truth rule).
7. **Degrade gracefully.** A genuinely unsupportable case returns a declared `UNSUPPORTED` (or
   `PARTIAL` / `InvalidSource`) outcome and **never crashes and never silently drops the edit.**

## 5. Edge-Case Checklist

The behavioral matrix the implementation (and its tests) must cover.

**Vue:**

- `<script setup>` only → insert into the setup block (after the last in-block import, else at
  content start).
- plain `<script>` only → role-correct decision: a template-triggered value import must NOT be
  blindly dropped into plain `<script>` if that does not expose the symbol; the adapter either
  creates the correct setup block, uses a semantically valid fallback, or returns
  `PARTIAL`/`UNSUPPORTED`.
- BOTH `<script setup>` and `<script>` coexisting → choose the role-correct block (setup for
  instance-scope value imports).
- existing imports present → append/merge with correct ordering relative to the existing import
  run.
- no script block at all → synthesize the framework-correct block at the parser-derived root
  slot.
- `lang="ts"` vs no `lang` → synthesized block mirrors the carrier's existing dialect rather
  than hardcoding `lang="ts"`.
- `src=` external script (`<script src="…">`) → degrade to a declared outcome (no in-place
  block to edit).

**Svelte:**

- instance `<script>` → insert there for template-triggered value imports.
- `<script context="module">` / `<script module>` only → must NOT silently place a value import
  in module script unless the requested symbol is genuinely module-scope-appropriate; otherwise
  synthesize the instance script or return `PARTIAL`/`UNSUPPORTED`.
- both instance and module present → role-correct selection.
- no script block → synthesize the instance `<script>` (NOT a Vue `setup` block — a hard negative
  assertion).

**General (every framework):**

- top-of-file comments / license headers / `@ts-nocheck` / directive prologues → insert AFTER
  them, not above.
- CRLF vs LF line endings preserved/normalized correctly.
- dedup vs an existing same-module import (extend the existing statement rather than emit a
  duplicate `import`).
- a genuinely unsupportable case → declared `UNSUPPORTED`, never a crash.

## 6. Critical Rule + Guard Intent

When implemented, this lands a **CRITICAL rule with a registered, discriminating guard in the
SAME change.** The R6 meta-guard
(`crates/verter_session/tests/g_misc0/critical_rules_have_guards.rs` →
`every_critical_rule_in_docs_has_registered_guard`) walks `CLAUDE.md` and every
`.claude/skills/*/SKILL.md` and FAILS any `(CRITICAL)` heading lacking a `CRITICAL_RULE_GUARDS`
registry row with at least one named guard — a prose-only rule does not pass the gate.

**CRITICAL rule text (codex, verbatim), to land in
`.claude/skills/framework-adapters/SKILL.md`:**

> Framework import placement is adapter-owned. Shared LSP/type-provider code may only request an
> original-source edit plan from the registry-selected framework import-placement capability. It
> must not branch on framework identity, file extension, tag names, or raw source text to select,
> find, or create script blocks. Anchors must come from parsed carrier block regions plus AST
> import spans. Synthesized script blocks must be produced by the framework adapter. Unsupported
> cases return explicit SUPPORTED/PARTIAL/UNSUPPORTED outcomes and must never crash or silently
> drop edits.

**Guards (codex), registered in
`crates/verter_session/tests/g_misc0/critical_rules_have_guards.rs` and the framework-adapters
`CRITICAL_RULE_GUARDS` table:**

- **`framework_import_placement_is_registry_dispatched`** — scans the shared LSP placement paths
  and FAILS on any of `scan_sfc_blocks`, `<script`, `</script>`, `script setup`, `.vue`,
  `.svelte`, `is_setup`, or a framework-name branch. *Intent:* prove the shared path holds no
  framework syntax knowledge and dispatches through the registry.
- **`framework_import_placement_capabilities_explicit`** — asserts every carrier framework
  descriptor declares an `AutoImportPlacement` status AND that the status matches registration
  presence (`SUPPORTED`/`PARTIAL` ⇒ registered impl; `UNSUPPORTED` ⇒ explicit absence). *Intent:*
  no accidental capability gaps.
- **`framework_import_placement_no_string_semantics`** — forbids raw tag scanning/synthesis in
  the neutral/shared modules; adapter-local block constructors may contain framework syntax
  literals ONLY under tests proving Vue and Svelte placement behavior. *Intent:* keep tag
  literals quarantined to adapter-local constructors that are test-proven.
- **`framework_import_placement_matrix_vue_svelte`** — a behavioral matrix test: Vue setup, Vue
  plain, Vue no-script; Svelte instance, Svelte module, Svelte no-script; malformed / external-src
  unsupported paths — INCLUDING a negative assertion that Svelte synthesized placement never
  emits `setup`. *Intent:* discriminating coverage (each case fails on the legacy hardcoded path,
  passes on the adapter path); not a trivially-passing stub.

## 7. Plan-Fit / Sequencing

This work crosses three plan areas:

- **U14 (framework-adapter substrate, `verter_session`).** It lives in
  `verter_session::framework` alongside the registry/descriptor/ctx that U14 re-wires. U14 itself
  is strictly "re-wire the already-merged substrate, NOT build it," and `verter_session` is the
  highest-regression surface (parse-artifact extensions touch cache invalidation) — so this
  capability is sequenced as its own narrow repair adjacent to U14, and any edit to
  `verter_session` is confirmed with the user first (per the standing confirmation rule).
- **B.7 — Completion / Hover / Signature-Help Semantics.** B.7's rescope-gate sub-surface
  inventory explicitly lists **"candidate sources + auto-imports."** When B.7 produces native
  auto-imports, it must **consume this placement primitive**, not invent a second placement path.
- **B.8 — Code Actions / Refactors / Organize Imports.** B.8 owns native organize-imports and
  `CodeTransform`-only edit generation; rename-file import updates / organize / extract read the
  project-model shape. B.8 must **reuse this same placement primitive** for the import-edit half,
  not reinvent script-block selection/synthesis.

Codex's explicit sequencing recommendation: **do not wait for B.7/B.8.** Because near-term
completion stays delegated to tsserver/tsgo (the U15 path that STAYS until B.7 reaches parity),
that delegated auto-import path must already place edits correctly for supported SFCs today —
so the shared placement primitive is needed before B.7/B.8 and is *consumed*, not produced, by
them. The risk (high-regression `verter_session`, parse-artifact cache invalidation) is real but
preferable to keeping framework semantics in the LSP consumer. Keep the change narrow:
parse-layout metadata + registry capability + Vue/Svelte implementations + LSP consumer
re-wiring + guards.

## 8. Where this is referenced from the unified plan

A one-line pointer to this document is added to the unified plan
(`docs/arch/semantic-db-overhaul-unified-remaining-plan.md`) inside the **U14 — Vue Framework
Adapter Re-Wire** block's *Perf-backlog cross-ref* bullet list — the existing index spot for
adjacent deferred framework-adapter deliverables (`D-I3`, `D-custom_elements`, `L-event-args`).

The B.7 block (§ "Completion / Hover / Signature-Help Semantics") and the B.8 block (§ "Code
Actions / Refactors / Organize Imports") should additionally point here when those blocks are
scheduled, since each *consumes* this primitive; this doc records that obligation, and the
plan-side B.7/B.8 cross-links are left for the implementer who schedules those blocks (no
speculative edits to those sections now).
