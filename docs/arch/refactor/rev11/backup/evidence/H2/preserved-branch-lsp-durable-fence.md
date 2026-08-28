# Preserved evidence — the LSP durable-fence branch

Work that closes a real defect in H2's surface was written before H2 opened, on a branch
with no DAG node, no ledger row and no authorization. An architecture ruling assigned it to
H2 as evidence rather than as a landing candidate. This file exists so H2's implementer
finds it instead of rediscovering the defect.

**Ruling:** [`LSP-DURABLE-FENCE-OWNERSHIP-2026-08-24`](../../rulings/ARCHITECT-RULING-2026-08-24-LSP-DURABLE-FENCE-OWNERSHIP.md).
Read it before reading the branch. It is not restated here.

## Where it is

- **Branch:** `block/lsp-durable-fence`, tip `7dac2b61453dee3c026f611053393f716e1fe52e`.
- **Worktree:** `<MACHINE_ROOT>/verter-lspfence`, a sibling of the main checkout.
  The developer's home prefix is normalised to `<MACHINE_ROOT>` here, matching the
  convention this evidence tree already uses — the literal root is what the
  `tracked_paths_no_machine_roots` guard rejects.
- **Base:** merge-base `2d84020bcc67eabee3a3285c75347b1f17d7a78d`, ten commits ahead.
- **Size:** 1,081 production insertions and 533 deletions across eleven files under
  `crates/verter_lsp/src/`, plus three test files and one skill doc.

**Do not delete this branch and do not remove that worktree.** They are the only copy of
this work. Nothing on trunk contains it.

## What the change does, mechanically

The durable carrier-publication gateway (`external_ts/carrier_sync.rs`) is made the single
producer of the bytes that get published, and the sole authority on whether they may be.

1. `PendingProviderReady` carries the gateway's own self-verified `(ide, api, pin)` triple,
   attached by a `with_verified()` builder at both `authorize()` sites. Every direct-open
   call site — sync coordinator, background drain, owner-loss drain, workspace scanner, and
   the four direct-open arms in `server/sync_orchestration.rs` — opens those bytes instead
   of its own independently captured locals, so a caller's earlier compile can no longer be
   published against the gateway's pin.
2. Self-verification is unified to run once before the membership / no-membership branch
   split. `CarrierSyncRequest` gains a `profile` field: a caller with a `CompileProfile` but
   no `DocumentRegistry` self-verifies through `req.host`, and `req.ide` is consulted
   verbatim only when neither is available.
3. A refusal from the fenced IDE-surface record aborts the publication. An IDE-attesting
   mint without an `IdeSurfaceFence` token returns `None` and the site requeues, rather than
   minting a `DirectOpen` receipt over unproven bytes.

## Status

Evidence, not a landing candidate. The ruling holds the design sound on its face and the
defect closure load-bearing, but rules that H2 re-derives the minimum fix; this exact
implementation is explicitly not sacred. Landing any of it needs its own authorization, a
rebase, the canonical gate, and a fresh review at the then-current tip.

Its two prior review traces both read `VERDICT: BLOCK`. Both are unbound — neither is in the
program's receipt corpus — and both are superseded by commits later in the branch that
answer them. They confer no approval in either direction: they are not a standing objection
H2 must clear, and they are not a review H2 may inherit.

The ruling grants H2 ownership of the question only. No ledger status moves and no
authorization row is created for it; H2 stays `LOCKED` with an unsatisfied predecessor set.
