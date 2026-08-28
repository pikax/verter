# Deferred: repeating re-entry over a Vue public-instance type's generic bounds

**Status:** DEFERRED — recorded, not repaired.
**Why deferred:** Verter's own typeinfo / semantic type resolver is out of scope for the
current effort (see the ratified scope directive: native type resolution is being taken off
the LSP path entirely, so resolver defects are not on the critical path).

---

**Audit verdict (2026-07-22): OUT-OF-SCOPE.** This is an internal semantic/typeinfo recursion defect, explicitly outside the binding scope.

## READ THIS FIRST — what this is NOT

This is **not** the cause of the language-server crash that was under investigation, and it
must not be re-chased as such.

The crash was a genuine non-terminating recursion in the LSP feature layer's cursor-context
classifier (`crates/verter_lsp/src/features/cursor_context.rs`), fixed separately; the fix
commit is titled *"stop the cursor-context classifier recursing on itself"*. That recursion
ran on the thread that polls `Server::serve` and exhausted a 64 MiB stack and a 1 GiB stack
alike.

The behaviour recorded *here* was observed on a **CPU-pool worker thread**, stayed **bounded
at 786 KiB of native stack**, and never overflowed. It was an early and wrong lead in that
investigation. It is recorded because it is real and wasteful, not because it is fatal.

## Symptom

While resolving an imported component's public surface, the shared cold-build dispatch
re-enters the same declaration's **type-parameter bounds** over and over: it walks the
parameter list from ordinal 0 to the last ordinal, in both `Constraint` and `Default`
position, then returns to ordinal 0 and repeats.

The declaration is Vue's public component-instance type,
`CreateComponentPublicInstanceWithMixins`, from the `@vue/runtime-core` type definitions —
a public framework type with a very wide generic parameter list whose constraints and
defaults reference one another and reference sibling helper types (`UnwrapMixinsType`,
`OptionTypesType`, `EnsureNonVoid`).

Measured in one debug-profile LSP session:

- **6667** entries to the shared cold-build helper in total.
- **825** of those lines named this one declaration.
- In the last 800 entries, **17 complete cycles** over the parameter list.
- A `memo_hit` was logged for the key *immediately before* almost every re-entry — the memo
  is hit and the work is redone anyway.
- Native stack on the affected worker peaked at **786 KiB** at `query_depth` 13. Bounded.

## Mechanism

Not fully established. What is known:

- The re-entries pass through the shared cold-build choke point,
  `crates/verter_session/src/project_semantic_dispatch/mod.rs:1834`
  (`execute_via_cold_build_helper`).
- The repeating key is `SemanticQueryKey::LowerLocator` with a `LocatorLoweringKey` whose
  `locator` is `DeclBody(TypeBodySlot { .. path: [TypeParamBound { ordinal: N, position: Constraint|Default }] })`
  and whose slot is the public-instance declaration's type slot. `N` sweeps the whole
  parameter list and then restarts.
- The `LowerLocator` cold build is
  `crates/verter_session/src/project_semantic_dispatch/locator_shape_binder.rs:187`;
  the memoized query is driven from
  `crates/verter_session/src/project_semantic_dispatch/locator_shape.rs:1086`.
- The interleaved `memo_hit` lines mean the *observed* re-entry is not simply a cold cache.
  Either the hit and the re-entry are on different keys that print alike at the log's
  truncation width, or a hit does not short-circuit the descent. **This was not resolved.**

The connected-query depth budget did not stop it; see
`docs/arch/future/semantic-dispatch-connected-depth-budget-reset.md`.

## Reproduction

Needs a real project: a Vue SFC that imports another component through a barrel and asks for
a definition/hover on the imported identifier, in a workspace large enough that the
component's public instance type is materialised. No synthetic reproduction was constructed.

Observed procedure:

1. Start the language server over a multi-package Vue workspace with
   `VERTER_LOG=debug`.
2. Open an SFC whose `<script setup>` imports a component from a barrel.
3. Issue `textDocument/definition` on the imported identifier.
4. Filter stderr for `execute_via_cold_build_helper` and group by
   `(merged_symbol_name, locator path)`.

Expected observation: repeated sweeps over `TypeParamBound { ordinal: 0..N }` for the
public-instance declaration, interleaved with `memo_hit` for the same key.

## Evidence

Measured with a throwaway probe committed only to the investigation branch
(`perf/inv-opus`, commits `1a34847dd` / `31ad96631`, reverted by `dc959bb80`).

The distribution over the last 800 cold-build entries was flat at 17 occurrences for each of
~24 distinct `(ordinal, position)` pairs of the same declaration — the signature of a
repeating sweep rather than a deepening descent.

Raw artifacts were written to a session-scoped scratch directory and are **not durable**;
the counts above are the whole of the evidence and are reproduced here so the record stands
alone.

## Proposed fix and falsifiable prediction

**Proposed fix (do not implement without the owner's direction):** a cycle guard on the
`LowerLocator` build keyed on `(ResolvedDeclSlotIdentity, AuthoredBodyLocator)` — the
content-free slot identity plus the locator path — so a body-shape lowering that is already
in flight for the same slot+locator on the same thread returns the in-flight sentinel
instead of re-entering.

**Falsifiable prediction:** with such a guard, on the same session, the cold-build entry
count for that one declaration falls from ~825 to at most one per distinct
`(ordinal, position)` pair (~48), and the total cold-build entry count for the request falls
materially from 6667. If the count does not fall, the re-entries are distinct keys and the
guard is aimed at the wrong identity — in which case the next question is why a `memo_hit`
is followed by a rebuild.

**Prerequisite:** resolve the memo question first. If a hit already returns a value and the
descent proceeds anyway, the defect is in the caller, not in the absence of a guard.

## Blast radius

- **If fixed:** less repeated work resolving imported component surfaces; a latency
  improvement on cold definition/hover over a component import, not a correctness change —
  *provided* the guard returns the same value the repeated work converges to. If the sweeps
  are load-bearing (each pass refining a partial), a naive guard would truncate them and
  change published component metadata. That risk is why this is not a drive-by fix.
- **If left alone:** wasted CPU on the component-import path, on a worker thread, bounded.
  No crash. Given native type resolution is leaving the LSP path, the cost may disappear
  with the call sites rather than needing a fix at all — evaluate that before investing.
