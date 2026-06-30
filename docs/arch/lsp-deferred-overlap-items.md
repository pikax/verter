# LSP Deferred Items — Overlap with the semantic-db-overhaul Plan

**Context.** During the LSP issue-fixing workstream on `fix/lsp-provider-parity`, several user-reported fixes/improvements were found to overlap surfaces that the in-flight **semantic-db-overhaul** plan (branch `refactor/semantic-db-overhaul`, [`docs/arch/semantic-db-overhaul-unified-remaining-plan.md`](./semantic-db-overhaul-unified-remaining-plan.md)) is actively rewriting.

Per the **defer-on-overlap rule**, these are deferred until the owning plan item lands, to avoid building throwaway paths that diverge from the plan. This document tracks them so they are picked up at the right time. Items that were found to be **orthogonal** to the plan are NOT listed here — they are being fixed now.

---

## D1 — Context-aware in-tag completion (emits-derived events, conditional directives, unknown-component diagnostic)

- **User issues:** `@`-events appear on *every* element/component instead of being emits-derived (5c); directives should be conditional on the component's surface — `v-slot`↔slots, `v-model`↔models, `v-on`/`@`↔emits, `v-bind`↔props (6); poor/unhelpful error for an unknown/unimported component tag (5a).
- **Root cause:** the in-tag completion provider is a hand-built **static** generator — a hardcoded ~9-element `@event` array + ~15 `v-*` directives pushed onto every tag — at `crates/verter_lsp/src/features/completion.rs` (~589–658). Only the resolved-imported-component path derives events from `defineEmits`.
- **Why deferred:** the correct fix derives events/directives from the resolved **emits/slots/models** surfaces — exactly what plan **B.7** (resolver-routed completion) rewrites. Building a second emits/slots walker in the LSP now would be a throwaway the plan tears out (also violates the single-engine rule).
- **Owner:** plan **B.7**.
- **5b (unresolved-component short-circuit) is ALSO deferred here** (codex-architect verdict 2026-06-19). It was attempted as Block E1 but is NOT soundly doable now: `TemplateComponentUsage.import_source == None` is **ambiguous** (it means global-registered OR locally-defined OR truly-unresolved), and Verter has **no positive "unresolved" signal** today — component resolution against builtins / local script bindings / Options-API `components` / imports+barrels / global `GlobalComponents` only exists in the B.7 resolver. The E1 predicate would have **false-minimalized valid global/local components** (a regression), and a "narrow" version would be a no-op. Pick-up: B.7 emits an explicit `ComponentResolution::Unresolved { tag, span }`; gate the minimal completion on THAT, with the invariant *ambiguous ⇒ fall through to current behavior, minimal only on a positive unresolved signal*.
- **Pick-up:** when B.7 lands, derive in-tag completion items (events, conditional directives, props) from the shared resolver surface; native events from a typed DOM-event surface; and gate the unresolved-component minimal completion (5b) on the resolver's explicit unresolved state.

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

## D4 — Provider content-revision provenance for edit-producing responses

- **The gap:** edit-producing provider responses (rename, code-action, combined "fix all", completion auto-import edits) are parsed and byte-offset-converted against the provider's contents cache, which is NOT version-coupled to the exact content the provider computed the response against. Edits are now proven against **fresh post-response target content** — a targeted snapshot of only the edit's target files, taken AFTER the await, with each byte range validated through the bounds-checked position converter (fail-closed on an out-of-range position). That is strictly safer than the prior whole-map clone, but it is "proven against fresh post-response target content," NOT "proven against the provider-producing version." Under a mid-flight `update_file` between provider-compute and snapshot, a same-shaped range can survive validation while targeting newer text.
- **Why it is a pre-existing gap, not introduced here:** the IPC providers have always used the unversioned contents cache for edit-offset conversion. The existing revision fence — `TypeProviderAdapter::query_type_data`'s `expected_revision` check (`crates/verter_type_runtime/src/provider_adapter.rs`), which rejects a query whose `synced_revisions` entry no longer matches — covers ONLY the nav/hover/type-query path. The edit-producing `TypeProvider` trait methods (`get_rename_locations`, `get_code_actions`, `get_completion_details`, `resolve_completion` in `crates/verter_type_runtime/src/traits.rs`) take no `expected_revision` and are outside that fence.
- **Severity / mitigation today:** the bounds-checked converter fail-closes the dangerous out-of-range cases, so the worst residual is a **visible, undoable** misplacement at the editor's current line/col — never a silent bogus-byte write.
- **The durable fix (deferred):** cache entries carry a provider content revision; edit-producing requests record the revision/generation they were issued against; edit parsing resolves against that exact retained revision or fails closed. This is a multi-provider request/content-revision model spanning tsserver out-of-process, tsserver in-process (the extension provider), and tsgo. No provider revision/version field is added to the code yet — this entry tracks the model, not an implementation.
- **Related follow-up (insert-text-format):** the completion DTO (`crates/verter_type_runtime/src/protocol.rs` `Completion`) does not carry `insertTextFormat`, so a snippet-formatted `textEdit.newText` that is moved to the plain-insert fallback on a dropped range would insert raw snippet text. Carry the insert-text-format through the DTO and the LSP mapper as a follow-up so a snippet degrades to a plain string (or is suppressed) rather than leaking `$1`/`${…}` placeholders.

---

_Maintained by the LSP workstream. When a plan item above lands, implement the deferred work against the new surface and remove its entry here._
