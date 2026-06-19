# LSP Deferred Items — Overlap with the semantic-db-overhaul Plan

**Context.** During the LSP issue-fixing workstream on `fix/lsp-provider-parity`, several user-reported fixes/improvements were found to overlap surfaces that the in-flight **semantic-db-overhaul** plan (branch `refactor/semantic-db-overhaul`, [`docs/arch/semantic-db-overhaul-unified-remaining-plan.md`](./semantic-db-overhaul-unified-remaining-plan.md)) is actively rewriting.

Per the **defer-on-overlap rule**, these are deferred until the owning plan item lands, to avoid building throwaway paths that diverge from the plan. This document tracks them so they are picked up at the right time. Items that were found to be **orthogonal** to the plan are NOT listed here — they are being fixed now.

---

## D1 — Context-aware in-tag completion (emits-derived events, conditional directives, unknown-component diagnostic)

- **User issues:** `@`-events appear on *every* element/component instead of being emits-derived (5c); directives should be conditional on the component's surface — `v-slot`↔slots, `v-model`↔models, `v-on`/`@`↔emits, `v-bind`↔props (6); poor/unhelpful error for an unknown/unimported component tag (5a).
- **Root cause:** the in-tag completion provider is a hand-built **static** generator — a hardcoded ~9-element `@event` array + ~15 `v-*` directives pushed onto every tag — at `crates/verter_lsp/src/features/completion.rs` (~589–658). Only the resolved-imported-component path derives events from `defineEmits`.
- **Why deferred:** the correct fix derives events/directives from the resolved **emits/slots/models** surfaces — exactly what plan **B.7** (resolver-routed completion) rewrites. Building a second emits/slots walker in the LSP now would be a throwaway the plan tears out (also violates the single-engine rule).
- **Owner:** plan **B.7**.
- **Done now (non-overlapping slice):** the unresolved-component short-circuit (5b) — return minimal completion for a tag that resolves to nothing — is LSP-local and is being implemented as Block E1.
- **Pick-up:** when B.7 lands, derive in-tag completion items (events, conditional directives, props) from the shared resolver surface; native events from a typed DOM-event surface.

## D2 — Update imports on file rename/move (`workspace/willRenameFiles`)

- **Feature:** when a `.vue`/`.svelte`/`.ts` file is renamed/moved, rewrite import specifiers in referencing files (`workspace/willRenameFiles` → `WorkspaceEdit`).
- **Status today:** `will_rename` capability absent (`crates/verter_lsp/src/capabilities.rs:206`; only `did_create`/`did_delete` wired). VS Code auto-forwards once advertised — no extension change needed. Infra exists and is LSP-reachable: `WorkspaceRead::reverse_deps_for`, `preferred_specifier`, `compute_relative_path`. Buildable entirely in `verter_lsp` with **zero `verter_session` edits** (only additive need is a source-literal span, obtainable via `ModuleReference.expr_span`).
- **Why deferred:** plan **§B.8** explicitly owns "rename-file import updates" (reads the future `N0` project-model; sequenced as the **last** LS block). It is a new feature surface B.8 will own; building it now risks divergence.
- **Owner:** plan **§B.8**.
- **Stop-gap option (if needed before B.8):** a thin, disposable LSP-only `will_rename_files` handler using the existing reverse-dep + specifier-recompute infra, built so B.8 can re-point it at `N0`.

## D3 — `TypeLocation` offset-kind / `Range`-over-wire protocol change (refs/rename/code-actions)

- The find-references **cross-file line-0 bug** (Block H) is being **fixed now** via the architect-cleared *safe subset* (host/VFS-readback in the merge layer mirroring the landed definition fix + TSGO `get_references` per-target content lookup) — **no wire change**; it front-runs the correctness rule L-B wants ("no fake external `Range::default()`").
- **Deferred part:** the plan's **L-B** item (flagged **STOP for user sign-off**) may later adopt an explicit `TypeLocation` offset-kind enum / `Range`-over-wire protocol change. That protocol work stays L-B's. If/when L-B adopts it, the safe-subset readback code is simplified or deleted — it is never semantically divergent (the current `TypeLocation` contract already documents byte offsets).
- **Owner:** plan **L-B** (protocol/offset-kind only; the observable correctness fix is landing now).

---

_Maintained by the LSP workstream. When a plan item above lands, implement the deferred work against the new surface and remove its entry here._
