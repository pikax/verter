# Where this work stands, and what to do next

**Written at hand-off.** This directory is the consolidated forward plan. A future agent should be able to
pick up from here without reading a transcript.

Read in this order:

1. **This file** — the state of the world.
2. [`03-editor-engine-selection-rejection.md`](03-editor-engine-selection-rejection.md) — **read before touching
   engine selection.** A three-cycle attempt was rejected on architectural grounds. Do not re-attempt it.
3. [`01-gate-integrity-block.md`](01-gate-integrity-block.md) — **the next block.**
4. [`02-serving-order-architecture-block.md`](02-serving-order-architecture-block.md) — the block after it, which
   **owns the user's still-unfixed bug**.
5. [`04-open-decisions.md`](04-open-decisions.md) — two decisions that are the **user's**, not an agent's.
6. [`orchestration-rules-continuation.md`](orchestration-rules-continuation.md) — the orchestration-rules block's
   own hand-off (already landed).

Supporting material:

- [`../gate-integrity-ledger.md`](../gate-integrity-ledger.md) — every cut mechanism, with owner, resolution
  gate, and named acceptance tests. **The authority for what the gate-integrity block owes.**
- [`../../better-implementation/editor-typescript-engine-selection.md`](../../better-implementation/editor-typescript-engine-selection.md)
  — findings from the rejected block, salvaged. Five open items at §6.1–§6.5.
- [`../../better-implementation/editor-engine-attestation.md`](../../better-implementation/editor-engine-attestation.md)
  — the VS Code attestation analysis, salvaged.

---

## The user's bug — still open

The user's editor runs **TypeScript 7** (tsgo, via the VS Code *Native Preview* extension). Verter selects a
broken **TypeScript 6**, and their `.vue` files come back **"No Project."**

**It is not fixed.** The block that tried was rejected (see §3). The fix belongs to the **serving-order
architecture block**.

## What landed

| | |
|---|---|
| **Branch of record** | `fix/lsp-provider-parity` (note: the branch named `main` is **stale**) |
| **Tip** | `5c4693ab1` — *docs(\*): admit decisions on evidence and require gates to prove execution* |

That commit is **documentation and orchestration protocol only — no production code.** It establishes the
verification rules the next two blocks are gated on, sweeps a class of defective prescriptions out of the
protocol/skills/in-repo memories, and opens the gate-integrity ledger.

**Caveat — it landed with two of its own gates UNMET, and pretending otherwise would make it the first example of
its own thesis.** Its unprimed review reached **zero P0s across five consecutive rounds but never returned
APPROVED** — successive rounds kept finding finer wording/mechanism items in ~60 KB of prose — and it landed under
the doc-review round cap. The **independent post-land confirm never ran**: it was dispatched against the landed
tip and **stopped mid-flight, leaving a zero-byte output and no verdict**. A dispatched-then-killed gate produces
exactly what an omitted gate produces — nothing — so it is recorded as **UNRUN**, never as pending and never as
passed. **[`../gate-integrity-ledger.md`](../gate-integrity-ledger.md) → GI-16 owns the remedy**, and it is a
ruling for the user: ratify the landing (recording the confirm as *waived*), or run the confirm on the landed tip
and adopt its findings as a follow-up commit. It may not be closed by the agent that landed the commit.

## What was rejected

`block/1-min-repro-fix` (`746f01029`) — the editor-engine **selection** path. **Never merged. Kept as evidence.
Do not merge it.** Its production surface (`provider_decision.rs`, `editorEngine.ts`, `--editor-engine`,
`VERTER_EDITOR_TSGO_BIN`) is **absent from the tree**, so "abandon it" required no revert. See §3.

## The defect class that dominated this work — read it twice

**More than eight times, a surface reported success without having run.** *The verification layer is where this
hides, because nobody verifies the verifier.*

**One class, four faces — a thing that silently does not happen, and reports success:**

1. **A TEST that silently does not run.** `packages/vue-vscode/e2e/fixtures/single-project/node_modules` is
   **gitignored**; without it ~7 real-provider tests `return` early and score **PASS with zero assertions**.
   **Set `VERTER_REQUIRE_TSSERVER=1 VERTER_REQUIRE_TSGO=1`** to make the skip hard-fail.
2. **A RULE that silently cannot fire.** A wait-protocol rule forbade backgrounding — while scoping itself to a
   path that excluded the only thing that backgrounds. *The rule existed, was correct, and could not fire.*
3. **A PIN that silently does not bind.** The protocol pinned a stale model, the cast table pinned the wrong
   effort, and the template a manager *copies* pinned **nothing at all** ⇒ default model, default effort,
   confident verdict, no warning. **Verify the binding from the CLI's own output. Never trust a pin.**
4. **A PLANT that silently does not apply** — the worst, because it lives *inside the verification of the
   verification*. **"A plant that fails to apply reports a pass."** `perl`/`sed`/`grep` **all exit 0 on a
   non-match**.

**The universal check, nearly free:** *what would I observe if this control silently did not apply?* **If the
answer is "the same thing I observe now," it is not a control.** The cheapest concrete form: **revert the wiring
and re-run — if it still passes, it tests nothing.**

**Three rules were found non-implementable only by RUNNING them** (see `orchestration-rules-continuation.md`):
a Windows PID kill that no-ops; a tree-kill that reports `SUCCESS` while every child survives; and a banner
validator that rejected every real leg. ***A check whose failure modes are understood only by its author is not a
validator.***

**The recurring process failure: the half-applied fix.** Four separate times a defect was fixed in one file and
left live in another. **After every fix, grep the whole tree for the pattern you just corrected.**

## Ground truth you should not re-derive

- **True base failing set: 5 of 118 executed** real-provider tests (63 tsserver + 54 tsgo), properly provisioned.
  Names: completion ×2, hover, rename, a completion/edit race. **The number was quoted as 2, then 7, then 11
  before anyone measured it on a tree that could actually run the suite.** Do not quote a baseline you did not
  measure.
- A **provisioned, built base tree** exists at `D:/dev/personal/verter-base-c6f5` — reuse it rather than cold-rebuilding.
- **`CARGO_BUILD_JOBS=2` is mandatory** on this machine; MSVC `link.exe` dies with `0xc0000142` otherwise.
- **Canonical Rust gate:** `cargo nextest run --workspace` **plus** `cargo test -p verter_session --tests`. Bare
  `cargo test --workspace --tests` **silently skips ~4404 tests**.
- ~45 stale worktrees have accumulated (`git worktree list`). Cleanup is tracked, not urgent.
