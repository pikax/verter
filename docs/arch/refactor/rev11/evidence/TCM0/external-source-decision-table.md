# TCM0 §6 — External-source decision table

Scope: charter item 6. Built from a direct, file:line-verified inventory of how each source shape is
handled TODAY (`Part B` of the diagnostics/external-source investigation) — the table below states the
model going forward and whether it changes from today.

Legend for "model": **TS-owned** (TypeScript's content-mapper protocol owns it directly — no Verter
transform involved beyond identity marking); **content-mapped** (Verter's content mapper produces a
transform, TypeScript projects through it); **Verter-owned** (never touches TypeScript — Verter's own
analysis end to end); **unsupported/fail-closed** (the shape has no legal model and activation for it
must fail closed, per the charter's own vocabulary).

| # | Source shape | Current owner (file:line) | Model going forward | Changed from today? |
|---|---|---|---|---|
| 1 | Inline `<script>`/`<script setup>` (Vue), Svelte `<script>` | Own OXC parse, one shared program per SFC (`verter_session/src/parse.rs:1543-1596`; Svelte tokenizer `svelte/parser/tokenizer.rs:140,225,359,965-966`) | **content-mapped** — this is the core case the content-mapper protocol exists for | No — same model, new transport (mapper `transform()` replaces the relay) |
| 2 | Inline `<template>` (Vue) / Svelte markup | Own template/markup parser → neutral fact model → codegen-projected TSX expressions (`verter_semantic::analysis::template`; `svelte/template_facts.rs`) | **content-mapped** for the projected-expression content; directive-level lint stays **Verter-owned** (unchanged — never touched TypeScript) | Partially — the projected-expression half moves transport; the lint half is untouched |
| 3 | Inline `<style>` (Vue) / Svelte `<style>` | Own CSS-syntax analysis (`verter_session/src/parse.rs:1526-1533,899-901`; `verter_diagnostics::rules::css::*`) | **Verter-owned** — never was and never becomes TypeScript's concern | No |
| 4 | Vue custom blocks (`<docs>`, `<i18n>`, etc.) | Opaque raw text, no parse (`compile/types.rs:455-460`, `VirtualNodeKind::Custom`) | **Verter-owned**, unchanged — opaque pass-through, zero diagnostic ownership for content, block-order lint only | No |
| 5 | Svelte module script / snippets / runes | Module script parsed + IDE-checked; snippets IDE-checked but runtime-unsupported (`svelte/carrier.rs:1255-1259`); runes recognized via IDE prelude ambient decls | **content-mapped** for the IDE-checked halves (module script, snippet IDE surface); **unsupported/fail-closed** for the runtime-unsupported halves, unchanged | No — same split, new transport for the content-mapped half |
| 6 | `<script src="...">` | `ExternalSourceRequest`/`ExternalBlockKind::Script`, forward scheduler dependency (`verter_session/src/types.rs:1643-1678`, `host_executor.rs:449-458`) | **TS-owned** — once the referenced file is a real on-disk `.ts`/`.js` file, TypeScript's own project resolution already covers it without any Verter transform; Verter's role narrows to registering the dependency edge, not projecting content | Yes — the external file no longer needs Verter's own read-as-artifact path for its *content*; Verter still owns the dependency-edge bookkeeping |
| 7 | `<template src="...">` | Same `ExternalSourceRequest` type, already supported (`types.rs:1645-1678`, tested at `framework_common/vue_bridge.rs:3237`) | **content-mapped, model NOT YET PROVEN** — the external file's content still needs the same template→TSX transform as an inline template, so it stays on the mapper path, just sourced from a different file, but the steering permits an external unit to be content-mapped only under model 2 ("independently content-mapped under a proven project/context contract" — §11). Diagnostic ownership IS already proven (`diagnostic-ownership-matrix.md`'s external-unit row: the external file's own URI, never the owning SFC's). The independent transform-input identity, project identity, and configuration identity are NOT yet proven — see `OPEN-GAPS.md`'s `G-TEMPLATE-SRC-PROJECT-CONTEXT-CONTRACT` row. | Open — same transport model as today, but the project/context contract model-2 requires is not yet supplied |
| 8 | External `<style src="...">` / imported stylesheets | Same `ExternalSourceRequest` type (`types.rs:1645-1678`) | **Verter-owned**, unchanged — CSS never touches TypeScript regardless of inline vs. external | No |
| 9 | Imported Svelte/Vue component assets, CSS Modules | Shared framework-adapter virtual-file-naming surface (`framework/descriptor.rs:106-133`); CSS Modules Vue-only (`ide/mod.rs:121,150`) | **content-mapped** for the imported-component surface (it resolves through the same cross-file declaration machinery as any other content-mapped carrier); **Verter-owned** for CSS-module class-name facts | No — same split |
| 10 | Supplemental outputs (secondary generated files per component) | `VirtualFileNaming` (`framework/descriptor.rs:106-133`): `ide`, `import_surface`, `testing_api_suffix` (Vue-only), `sidecar_suffixes`, `declaration_surface` | **content-mapped**, using the protocol's own native `SupplementalOutput` field (`package-lock-and-semantic-api.md` §3) — this is a direct, purpose-built replacement, not an approximation: the upstream protocol was explicitly designed for "multiple TypeScript files from a single source" (its own example is multiple Astro script blocks) | Yes — today's supplemental-output naming convention (companion-suffix files) is superseded by the protocol's native multi-output support from ONE `transform()` call, collapsing several separately-materialized virtual files into one mapper response |
| 11 | Multi-unit helpers (intra-file `Fragment`/`SourceUnit` assembly) | `assembly/fragment.rs`, `assembly/source_unit.rs` — combines script+template+style logical units of ONE SFC into one assembled artifact | **Verter-owned**, unchanged — this is exactly the machinery that PRODUCES the one content-mapper `transform()` output; it does not itself become TypeScript's concern, it feeds the mapper | No |

## Decision rule this table locks

An external/multi-unit shape gets **content-mapped** when its content requires a Verter-owned transform
before TypeScript can type-check it (template expressions, snippet IDE surfaces, imported-component
surfaces). It gets **TS-owned** only when the referenced file is already valid, unmodified
TypeScript/JavaScript that TypeScript's own project resolution reaches without any transform (plain
`<script src="./foo.ts">`). It stays **Verter-owned** when TypeScript has no legitimate opinion on the
content at all (CSS, opaque custom blocks, directive syntax lint). Nothing in this table introduces a
new "unsupported" class beyond what is already unsupported today (Svelte runtime-rejected constructs) —
TCM0 does not use this deliverable to unilaterally drop support for anything currently working.
