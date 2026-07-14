# The editor-engine selection path was REJECTED — read before touching engine selection

**Status:** rejected by an unprimed codex necessity review. **Do not re-attempt it. Do not merge
`block/1-min-repro-fix` (`746f01029`).** It is kept as evidence only.

The two-reopen breaker fired: **three fix cycles at the same seam**, each producing a correct-looking fix whose
defect reappeared one layer deeper. The ruling: **land none of the production selection path; fold the reported
bug wholly into the serving-order architecture block.**

**Nothing had to be reverted.** `provider_decision.rs`, `provider_decision_tests.rs`, `editorEngine.ts`,
`--editor-engine`, and `VERTER_EDITOR_TSGO_BIN` are **absent from the tree** — they existed only on the unmerged
branch. *Not landing it* **was** abandoning it.

---

## The root cause — and it is NOT the thing everyone reached for

The model **conflates three non-interchangeable axes**:

| Axis | Question it answers |
|---|---|
| **POLICY** | what *should* serve the IDE |
| **IDENTITY / PROVENANCE** | *which* editor session, Program, project binding, installation, generation |
| **FEASIBILITY** | what can actually attach or start |

> **"Tsgo family is not a serving identity."**
>
> An editor-owned warm Program, a workspace binary, an operator override, and an npm-cache binary are **not
> substitutes merely because all of them are tsgo.**

### The line that kills the obvious patch

> *"Even replacing the `bool` with a tier enum would leave the global startup-time family selection and
> independent-process substitution intact."*

The proximate defect was `DiscoveredEngines.tsgo_binary_available` — a **tier-less `bool`** that erased provenance
before the decision ran, so a tier-3 (editor-supplied) binary could satisfy `tsgo_can_serve()` over a *verified
workspace floor*. **A tier enum was the fix everyone proposed, and it does not work.** Do not reach for it.

## What was still broken in the tree when the review ran

- **The original defect was never fixed — only moved.** The client still always emits bundled `--tsdk`; the
  decision still derives its tsserver *bottom* from `fallback_tsserver_major`. *"Demoted from workspace floor to
  bottom, but capability is still consumed as preference."* **A capability ("a tsserver exists here") is still
  being read as evidence ("this workspace prefers tsserver").**
- **A fourth erasure sits one layer earlier:** `GatheredEngines` stores `Vec<String>`, erasing candidate authority
  — so **even the documented `VERTER_TSGO_BIN` operator override is merely "the first candidate" and can silently
  fall through.**
- **The decision test-suite explicitly asserted the compiler-substitution bug as correct.** The suite codified the
  defect.
- **The trust filter was dead code certified by passing tests.** The extension declares no
  `capabilities.untrustedWorkspaces`, so VS Code disables it in Restricted Mode ⇒ `isTrusted` is always `true`.
  And even if it were reachable, **server discovery has no trust input** and independently executes workspace
  `node_modules` binaries. **User-ratified: delete the plumbing and the claims. Do not ship it inert.**
  Untrusted-workspace support is a **future, explicit feature** with its own threat model — not leftover plumbing.
- **`probe_engine_version` uses blocking, unbounded `Command::output()`** on the startup path, invoked repeatedly
  on candidate retry, and the guard named `tsgo_discovery_spawns_no_subprocess` **only scans one function's body**.

## Trap: don't settle precedence by accident

The user **explicitly deferred final IDE engine precedence** out of that block — and the block silently settled it
anyway, in the editor's favour, while four docstrings and a guard test claimed it had not. A workspace pinning
`typescript@5.9` on a machine with Native Preview installed would have had its pinned compiler **silently replaced
by the editor's TS7**.

**Ratified intent:** the IDE's job is **editor parity**. **Build/CI parity belongs to `verter-tsc`.** Final
precedence is the **architecture block's** call and nothing else may settle it.

## SALVAGE — use these; do not repair the family lattice

`ProjectBinding` · `BoundProject` · `EngineIdentity` · eligibility facts · `decide_live`
(`verter_session/src/external_ts/live_decision.rs`).

**Do NOT preserve the extension-spawned duplicate engine as a transition path.**

## What the block did produce that is worth keeping

- The diagnosis above (a patch would never have found it).
- The **first baseline anyone actually measured**: 5 failures of 118 executed, 0 regressions.
- Proof that two of its own gates were **theatre** (raw source-text `.contains()` assertions — a violation of
  Verter's own no-string-based-semantic-logic rule; the client's entire mechanism could be gutted and the wire
  contract still reported `ok`, because the literal it grepped for also sat in a doc comment).
- The architecture block's **acceptance criteria**, written as falsifiable conditions.

Findings salvaged to
[`../../better-implementation/editor-typescript-engine-selection.md`](../../better-implementation/editor-typescript-engine-selection.md)
(five open items, §6.1–§6.5 — the largest being that **the tsserver half of the real-provider suite has never
executed in CI and is hiding six real failures**) and
[`../../better-implementation/editor-engine-attestation.md`](../../better-implementation/editor-engine-attestation.md).
