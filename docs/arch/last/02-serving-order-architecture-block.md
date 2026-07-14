# Block 2 — Serving-Order Architecture (owns the user's bug)

**Status:** ratified in principle; runs **after** the gate-integrity block.
**This block owns the user's still-unfixed "No Project" bug.** It lands as **one cutover**, not a patch series.

Start by reading [`03-editor-engine-selection-rejection.md`](03-editor-engine-selection-rejection.md). A previous
attempt was rejected on architectural grounds, and its diagnosis is this block's foundation.

---

## Ratified product intent — the serving order

1. **Editor runs tsgo** ⇒ **attach to and enhance that editor-owned TypeScript program.** (Do not spawn a rival.)
2. **Editor runs tsserver** ⇒ **activate Verter's TypeScript plugin inside that tsserver.**
3. **Neither available, or verified attachment fails** ⇒ **run a pinned, verified, locally cached managed tsgo.**
4. **Preserve server-side feasibility checks and late failover.**
5. **`verter-tsc` stays a one-shot Batch consumer** of the shared compiler/project/type substrate — it must **not**
   inherit editor or LSP lifecycle machinery.

**Per the ratified process rules, this block begins with a product-intent statement and a user-visible acceptance
matrix. Mechanism design cannot begin until the intended authority and fallback order are explicit.**

## Build on the salvaged substrate — do NOT repair the family lattice

Use **`ProjectBinding`**, **`BoundProject`**, **`EngineIdentity`**, eligibility facts, and **`decide_live`**
(`verter_session/src/external_ts/live_decision.rs`).

**Do NOT preserve the extension-spawned duplicate engine as a transition path.**

## Two milestones — do not conflate them

This distinction fooled three separate readings in a row. Each time, the reader mistook a **transport proof** for a
**serving proof**.

| | |
|---|---|
| **M1 — Interim SHARED reachable** | The existing relay overlay activates under the intended configuration and produces proven results. **This is NOT step 1.** |
| **M2 — Editor-session reuse complete** | VS Code's **actual tsgo session** runs through the relay; Verter attaches its API session to **that same Program**; **successful steady-state attachment runs NO duplicate semantic engine**; managed tsgo starts **only as fallback**. |

**M2 is the product goal.**

### THE DISCRIMINATOR

> **Is a duplicate semantic engine running? If yes, we are not attached** — no matter what the plumbing proves.

**Production today spawns an independent second tsgo.** So today the honest answer is *not attached*.

**What exists:** a `verter-relay-shim` whose **transport is real and proven** (live tests serve a real tsgo through
it; the shim **is** staged into the VSIX). **What does not exist:** anything *serving through* it. SHARED is
currently **unreachable** — `verter.typeProvider`'s enum has no `shared-tsgo` value, `auto` explicitly stays OWNED
behind a `TODO(follow-up)`, and `planSharedTsgo` never receives the Native Preview path.

**Wiring those three gaps buys M1. It does not buy M2.** Green transport tests prove the shim **works**; they
prove nothing **serves through** it.

## Acceptance criteria — all CORRECTNESS-REQUIRED, all falsifiable

From the unprimed architectural ruling. These are the block's safe-to-land conditions:

- **Editor identity proven live** — the result carries the **editor session generation + bound project**, and a
  **process-level test proves NO duplicate semantic tsgo after attachment.**
- **Editor-tsserver parity real** — the plugin proven active **inside the editor's** tsserver.
- **Fallback ordering observable and bounded** — managed tsgo starts **only after verified attach/plugin failure**;
  **every handshake, probe and child has a timeout + kill/reap.** (See the gate-integrity block: the naive kill is
  a proven no-op that reports success.)
- **Provenance typed through construction** — **no family-wide availability boolean and no untyped path list may
  authorize a different source.** Operator overrides retain **explicit failure semantics** (today they can silently
  fall through, because candidates are a bare `Vec<String>`).
- **Tests discriminate the actual seam** — the original Native Preview / "No Project" repro must **FAIL at base and
  PASS at tip**, asserting **Program / session identity — NOT the `Tsgo` enum**. **No raw source-text guards.**
- **Real-provider verification non-vacuous** (see the fixture-provisioning trap).
- **Trust claims match deployment.**

## Constraints

- **IDE intent is editor parity.** Build/CI parity belongs to `verter-tsc`. **This block owns final engine
  precedence** — it is the only thing that may settle it.
- **The LSP is editor-agnostic.** `editors/helix` and `editors/nvim` are **shipped**; Zed, Lapce and JetBrains are
  planned. **Server decisions must be portable.** Editor-specific knowledge lives in the **client** and enters the
  server only as neutral facts an arbitrary LSP client could also supply. Ask of any signal: *could a Neovim plugin
  produce this?* If not, the server must not depend on it. **Editors are the same axis as frameworks: one
  substrate, a reference client, no privileged path.**
- **Do not touch `--tsdk` casually.** It is the root capability-as-evidence defect **and it is still live** — but
  removing it naively re-breaks the JS-only class through the other door (`raised` goes false, so the feasibility
  guard stops firing). Fix the *conflation*, not the symptom.

## Tracked, with gate and owner

- **R5** — shipped `editors/helix` (hardcodes tsgo) and `editors/nvim` (accepts auto/tsgo/tsserver/off, defaults
  tsgo); `verter-editor-client` consumers are clamped to `{tsgo, off}`. **Gates the next release of each affected
  editor client.**
- **R6** — the tsgo spawn gate is **not** the predicate that decides whether a `.vue` file gets a project, so
  **"No Project" still reaches a real class of users.** **Gates the first-class IDE milestone**, and any claim that
  config-less "No Project" behaviour is solved.
