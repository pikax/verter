# Recovered Template-Data Fence — Design (for the deferred Block D "behavior A")

**Status:** DEFERRED big block (post-wave). Architect-verdict captured 2026-06-19. Behavior A of the auto-close work (`</`-completion on genuinely-unclosed / mid-edit templates) requires this fence; it must NOT land without it.

## Problem

To make `</`-completion work on an unclosed/mid-edit template, the auto-close work decoupled template-DATA extraction from the codegen gate in `crates/verter_compiler/src/compile/mod.rs`:

- **Before:** a recoverable parse error (`XMissingEndTag`, i.e. an unclosed tag) dropped ALL template processing — no data, no codegen.
- **Decoupled (un-fenced):** codegen stays gated on `!has_parse_errors` (correct — no render output on broken input), but `raw_template_data` is now extracted and **published into the shared analysis even on recoverable parse errors**. The parser also widens force-closed elements' content/span to EOF and flags `ElementNode.is_unclosed`.

**Regression (why a fence is mandatory):** recovered/partial template facts from a malformed document are now published into shared analysis **with no marker distinguishing them from complete facts**. Validity-sensitive consumers then emit *wrong* results on mid-edit docs (false-positive lints, wrong edits, wrong component-meta), not merely missing ones.

## Architect verdict — per-consumer safety on recovered/partial data

- **SAFE / intended:** closing-tag completion and narrow cursor-local features that explicitly reason about `is_unclosed` (`cursor_context::nearest_unclosed_enclosing`).
- **UNSAFE (must opt out / degrade):**
  - General completions (component/prop/event use `template.components` + child contracts; v-for scoped completions depend on element spans — an EOF-widened element can leak loop scope or offer the wrong component's props).
  - Diagnostics & lints (`diagnostics_bridge` passes `analysis.template` straight to linter + component diagnostics → false positives for props/models/slots/CSS/a11y/directive/security/SSR).
  - Code actions, rename, references, document-highlight (wrong recovered facts → wrong *edits*, not just missing).
  - Hover / definition / navigation (can point to wrong props/events/slots/CSS/refs from a malformed tree).
  - CSS analysis & MCP CSS tools (selector matching / bleed / class refs need complete structure).
  - **Component-meta, fallthrough, root inheritance (high-risk):** `component_meta_extract` consumes the shared template; root reachability / fallthrough can be cached & published as *exact* from a recovered tree.
  - **Cross-file optimization / render tree (high-risk):** `cross_file.rs` builds parent-child render edges from raw template analysis; partial data can omit spreads/dynamic bindings or invent const-looking props.
  - MCP / NAPI / WASM / project index / docs (expose analysis/lint/actions/selectors/summaries to external callers).
  - Linked-editing / folding / symbols / inlay / call-hierarchy (lower severity, still not contract-safe).

## Required design

Add an explicit fence on the shared template-data path:

```rust
RawTemplateData { recovered: bool, /* or is_partial */ ... }
TemplateAnalysisSnapshot { recovered: bool, ... }
```

- Mirror it in the **TS** shared-analysis type.
- Name `recovered` / `is_partial` (the compile path extracts data whenever `needs_tpl_data` even if `parsed.has_errors()`, not only a carefully-classified recoverable subset). If a stricter `has_recoverable_errors` is wanted instead, the compiler must first **classify** which parser errors are truly recoverable for shared facts (start with `XMissingEndTag`) and refuse shared publication for the rest.
- **Consumers treat `recovered == true` as:** allowed for closing-tag-completion / narrow `is_unclosed`-aware features; **blocked or degraded** for diagnostics, code-actions, rename/editing, component-meta/fallthrough/root-inheritance, cross-file optimization, project index, MCP authoritative reports; optional non-authoritative degraded UI for hover/symbol/folding/navigation.

## Net change

Pre-fix failed CLOSED (parse-error docs → no template facts). The fence keeps the intended mid-edit closing-tag-completion improvement while restoring fail-closed behavior for every authoritative consumer.

## Scope notes

- Behavior B of the auto-close work (`editor.formatOnType` moved under `contributes.configurationDefaults` for `[vue]`/`[svelte]` → proactive `</tag>` auto-insert on `>`) is **fully contained** — no decoupling/fence needed — and lands with this block (or earlier as a standalone quick win).
- The parked branch `fix/lsp-template-auto-close-tag` holds the behavior-A parser/decoupling work (`is_unclosed` + EOF widening + `cursor_context` Case B' + partial-replace + `/`-trigger scoping) to build on. It also carries a 1-line `verter_session/template_convert.rs` `is_unclosed` passthrough (user-approved) and two NITs to fix when landing: `TemplateElement` serde drops `is_unclosed` true→false; EOF widening overshoots `</template>` for inner elements (clamp to template-region end).
