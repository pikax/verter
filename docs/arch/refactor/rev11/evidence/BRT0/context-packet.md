# BRT0 — landing context packet

The context this block was executed under, recorded before worktree teardown.

## Dispatch

- **Charter:** [`../../charters/BRT0.md`](../../charters/BRT0.md).
- **Ratified findings:** the `RT-1`, `TR-1` and `BND-2` rows in
  [`../BF3/dispositions.md`](../BF3/dispositions.md), and the `TR-1` section of
  [`../BF3/disposition-ruling.md`](../BF3/disposition-ruling.md). Not reclassified,
  renamed, re-owned, or substituted.
- **Base:** `program/architecture-lock` @ `dd84e5fa2`.
- **Worktree:** a dedicated `git worktree` on branch `block/brt0`, created from the base
  above and never shared with another block.

## Scope split executed

The three owned findings do not share one surface, and one of them collides with a
concurrently running block.

| item | executed here | reason |
|---|---|---|
| `TR-1` | YES | `crates/verter_wasm/src/lib.rs` + `crates/verter_napi/src/lib.rs`; no overlap with any concurrent block |
| `BND-2` (Rollup/non-Vite inline) | YES | `packages/unplugin/src/index.ts`; no overlap |
| `RT-1` | YES — in a SECOND pass | held while a concurrently running block owned its correction site, then executed after that block landed and this branch was rebased onto it |

The block landed in two passes. The first landed `TR-1` and `BND-2`; `RT-1` was held
because its correction site was the active file of a concurrently running block, and the
carrier-classification change alters which responses reach that block's in-flight
transaction construction. The second pass rebased onto that block's landing and executed
`RT-1` against its corrected shape — which matters, because `RT-1` is what makes a Svelte
refusal REACHABLE on the batch route for the first time, and those refusals now travel
through the landed typed outcome rather than the deleted string-scanned path.

## Binding constraints applied

- A wrong output is a bug, not an error path: no production guard, typed refusal,
  withhold path, retraction, or runtime tracking artifact was added — see
  [`../BF3/maintainer-standing-ruling-bugs-and-types.md`](../BF3/maintainer-standing-ruling-bugs-and-types.md)
  and [`../BF3/maintainer-product-ruling-no-error-on-bad-output.md`](../BF3/maintainer-product-ruling-no-error-on-bad-output.md).
- Types waived for the program; no type-correctness work opened. The two public
  return-type declarations touched are the declared shape of the contract this block
  settles, not type-correctness work.
- Bugs found outside the owned findings: captured as a skipped characterization test
  naming its production owner, not fixed.
- Every gating assertion proven RED before the correction and GREEN after, with the
  planted mutations proven present, unique and new before any planted run was trusted.

## Review seats

External CLIs only, run sequentially. Prompts neutral, per-finding evidence citation
required, uncited claims discarded.

| seat | agent | prompt |
|---|---|---|
| adversarial | `codex exec -m gpt-5.6-sol` @ high | prove the tests can fail (plant/red/green at first pass), then attack the contract choice, the swallow surface, the map's correctness, the deleted coverage, and the added assertions |
| conformance | `grok` 4.6 @ xhigh, explicit default-to-BLOCK | decide whether the code, not just the test, meets each exit criterion literally; prove non-zero selection; identify lost coverage |
| scope consult | `codex exec -m gpt-5.6-sol` @ high | falsification-shaped: does deferring the adjacent source-map defect violate a named invariant, and rule the disposition |
