# Editor TypeScript-engine selection — findings, and what is deliberately not settled

Produced while fixing a reported bug: the user's editor runs TypeScript 7 (tsgo, via the
VS Code "TypeScript Native Preview" extension), Verter selected a bundled TypeScript 6, and
their `.vue` files came back "No Project".

The bug is fixed. Everything below is a correctness finding that the fix surfaced and did
**not** address — recorded here with an owner and a gate, so that nothing is silently
absorbed and nothing is silently deferred.

---

## 1. `--tsdk` is consumed as evidence, not capability — the root defect

**Owner: architecture block. Gate: before any further change to engine selection.**

The editor client **always** passes `--tsdk`, defaulting to the extension's bundled
TypeScript (`extension.ts`: *"Always pass --tsdk: user setting → bundled TypeScript"*).
The server turns that path into `fallback_tsserver_major` via `find_tsserver(tsdk, …)`, and
`decide_auto` then reads it as a **preference**:

```rust
let bottom = if matches!(fallback_tsserver_major, Some(5 | 6)) || has_composite {
    Tsserver
} else {
    Tsgo
};
```

`--tsdk` says *"a tsserver exists here"* — a capability. It is being read as *"this
workspace prefers tsserver"* — evidence. The consequences:

- For **every** VS Code user with no TypeScript of their own, `fallback_tsserver_major` is
  `Some(6)`, so `bottom` is **always** `Tsserver`.
- The documented `bottom = Tsgo` branch (*"no tsserver preference applies; tsgo is the
  default"*) is therefore **unreachable in VS Code**. It can only fire for a client that
  omits `--tsdk`.
- The editor-engine attestation exists largely to climb back over a floor that the client's
  own `--tsdk` manufactured.

**The minimal separation** is to split the two roles: preference = the verified floor, else
`has_composite`; availability = `fallback_tsserver_major`, used **only** as the fallback
target.

```rust
let preferred = floor.unwrap_or(if has_composite { Tsserver } else { Tsgo });
let selected  = preferred.max(evidence.unwrap_or(preferred));
match selected {
    Tsgo if tsgo_can_serve() => Tsgo,
    Tsgo => match (floor, fallback_tsserver_major.is_some()) {
        (Some(Tsgo), _) => Unavailable,  // a real TS7 workspace: tsserver is WRONG, not degraded
        (_, true)       => Tsserver,     // serve the tsserver we HAVE
        (_, false)      => Unavailable,  // nothing runnable
    },
    Tsserver => Tsserver,
}
```

**A trap, which will bite whoever does this.** Flipping the default *alone* re-breaks the
JS-only class through a different door. `preferred = Tsgo` → cannot serve (no configured
project) → but `raised` is now **false** (the base was already `Tsgo`), so the no-stranding
guard does not fire, and the user is left with no provider — **with no observation
involved at all**. `fallback_tsserver_major` must be retained as the fallback *target*.

Do both, and no-stranding stops being a case analysis and becomes structural: the selection
is a join over the **feasible sublattice**, and `raised` disappears from the decision.

**Open question for the architect:** `has_composite → Tsserver` may itself be a *capability*
limit ("tsgo cannot handle solution-style tsconfigs") rather than a preference. If so it
belongs in `tsgo_can_serve()`, preference collapses to `floor.unwrap_or(Tsgo)`, and every
tsserver outcome becomes uniformly *"tsgo cannot serve; serve the tsserver we have."*

---

## 2. Final engine precedence: editor parity vs. project parity

**Owner: architecture block. Gate: before the editor-attach work lands.**

This block deliberately does **not** settle it. The engine handoff
(`VERTER_EDITOR_TSGO_BIN`) sits **below** the workspace `node_modules` tier, which is all
the reported case requires — a workspace with no tsgo of its own. A workspace that owns an
engine resolves **byte-identically to base** (pinned by
`the_editor_engine_supplies_only_what_the_workspace_lacks`).

The unanswered question: when a workspace pins `typescript@7.0.3` and the editor runs the
Native Preview extension's dev build, which serves the IDE? Editor parity says the editor's;
project parity says the project's. They disagree, and the answer is not this block's.

---

## 3. SHARED / relay editor-attach — proven plumbing, nothing serving through it

**Owner: architecture block.**

This is the most misread area in the codebase, so it is stated as two milestones. Optimistic
readings of it have now survived three separate reviews (a docstring, a summary, and a
"three wires" framing of my own). The same question killed all three:

> **If a duplicate semantic engine is running, we are not attached — no matter what the
> plumbing proves.**

### What is TRUE today

- The relay shim + SHARED provider machinery **work**, against a real tsgo, non-vacuously.
  Six live tests pass under `VERTER_REQUIRE_TSGO=1`: carrier injection, diagnostics
  composition, reconnect / no-split-brain, carrier-leak prevention, project-reference
  closure (`crates/verter_lsp/tests/shared_provider_live.rs`).
- The shim **is** staged into the VSIX (`packages/vue-vscode/stage-bin.mjs`). The
  `/host-session` skill's note that it is "not yet in the build scripts (C6)" is **stale** —
  correct it there.

### What is NOT true

- **`shared-tsgo` is not a selectable setting.** `verter.typeProvider`'s enum is
  `["auto","tsgo","tsserver","extension","off"]`.
- **`auto` — the default — never engages SHARED.** `establishSharedTsgo` returns
  `NO_SHARED_TSGO` for any non-tsgo-routing provider, behind an explicit `TODO(follow-up)`.
- **`planSharedTsgo` never receives `nativePreviewExtensionPath`**, so it cannot resolve the
  engine inside the editor's extension — the exact tier the OWNED path needed and got. With
  NP installed, no tsdk configured, and no tsgo in `node_modules`, it resolves nothing and
  fails closed to OWNED.

### The two milestones — do not conflate them

1. **Interim SHARED reachable.** The existing overlay activates under the intended
   configuration and produces proven results. This is what the three gaps above buy. **It is
   not the goal.**

2. **Editor-session reuse complete.** VS Code's **actual** tsgo session runs through the
   relay; Verter attaches its API session to **that same Program**; successful steady-state
   attachment runs **no duplicate semantic engine**; a managed OWNED tsgo starts **only** as
   a fallback.

**Milestone 2 is the product goal.** Milestone 1 is an interim overlay.

**What the live acceptance-gate test (`packages/vue-vscode/src/d1AcceptanceGate.spec.ts`) does and does not establish.** It proves the shim is armed, the
transport carries carriers and diagnostics, and OWNED results are correct through the
composite. It does **not** prove that anything *serves through* the editor's session:
production currently spawns an **independent second tsgo**. Six green tests against a real
engine prove the transport works. They prove nothing about attachment.

---

## 4. Smaller findings, with owners

| Finding | Owner | Gate |
| --- | --- | --- |
| `main.rs` falls back to tsgo when a tsserver spawn fails without re-entering `decide` — the decision authority is bypassed. **Pre-existing** (base `c6f50174d` has the same control flow; the block added `decide` but did not change this edge). | architecture block (owns the authority) | with the `--tsdk` split |
| `verter.mcp.lintPreset` is read into `--mcp-lint-preset` but is in no restart list — the server keeps a stale value. Same class as the tsdk watcher gap; this block does not read it. | MCP owner | next release |
| `find_tsserver_workspace_only` walks 10 ancestors, so a TypeScript in a home directory can become the "verified workspace-owned" floor. | architecture block | with the `--tsdk` split |
| Native Preview's `MF` consent gate (its private `workspaceState["…useWorkspaceTsdk"]`) is unreadable by us, so a workspace-scoped tsdk is honoured where NP would ignore it. The safety half (workspace trust) **is** replicated. | architecture block | with editor-attach |
| NP's `fy`/`RT`/`ET` resolution (package.json-driven platform sub-package; `.code-workspace`-relative roots; the nightly extension id; Windows `\\?\` long paths) is approximated, not ported. | architecture block | with editor-attach |
| A configuration change during initial server startup is dropped (`scheduleServerRestart` returns while `server` is undefined), so a `useTsgo` flip mid-startup can leave a stale attestation. | LSP client owner | next release |
| **The VS Code E2E lane is dead.** `build-vscode-e2e` is `if: false` ("DISABLED — flaky") and `vscode-e2e` *needs* it, so no Extension-Host test can gate anything. Re-enabling it is pre-existing work. | CI owner | before any Extension-Host coverage is claimed |
| **Coverage gap, opened deliberately here.** An `editor-tsgo-observation` E2E suite was written and then **removed**: it could only run in the dead lane above, and it did not discriminate (reverting the `extension.ts` wiring left it green). No end-to-end assertion now covers the **actual spawned argv + environment** — that the launched server really receives `--editor-engine=tsgo` and `VERTER_EDITOR_TSGO_BIN`. The pure decision, the two shipped gates, the tsdk table, the restart policy, the cross-language wire contract and the precedence invariant are all covered non-vacuously by unit tests in executing CI jobs; only the process-launch seam is not. | LSP client owner | when the E2E lane is re-enabled |

---

## 5. The one thing to carry forward about testing this

The 531,441-configuration differential proves the gate model is **self-consistent with an
oracle transcribed by hand from Microsoft's shipped bundles**. It cannot prove the
transcription is right: if it is wrong, both sides agree and the test passes.

A mirror of someone else's private, unversioned internals is a **lagging indicator by
construction** — it can only tell us they changed *after* they changed, and only if CI has
that version. No amount of testing removes that.

Cheap mitigation (do this): a freshness gate that reads the **shipped bundles** and asserts
the literal data (NP's `_O` table, its `OO` comparator, `RO`'s six tiers, `AF`'s probe
paths; the built-in's `Cb` disjunction and extension-id list) matches what the code encodes
— the `typeinfo_proto_ts_freshness` pattern this repo already uses. It kills the class that
actually bit us, which was a table-reading error.

But the real mitigation is architectural, and it is item 1 above:

> **The correct response to "we can't fully test the mirror" is to shrink what depends on
> it, not to test it harder.**

Once `--tsdk` stops manufacturing a tsserver floor, the reported user is served tsgo with
**no attestation at all**, and the mirror's remaining job is the single honest case — where,
if it is wrong, the failure is *"we keep serving the workspace's own TypeScript"*: a safe
wrong answer, not a broken one.

## 6. Open items this change did NOT close

### 6.1 The tsserver real-provider suite has NEVER run in CI — and it is failing

`.github/workflows/ci.yml` `rust-test` (:185-221) and `rust-coverage` (:223-260) set `VERTER_REQUIRE_TSGO=1`
and prewarm tsgo, but they **never set `VERTER_REQUIRE_TSSERVER`, never `pnpm install`, and never build the
TS packages**. `@verter/typescript-plugin/dist/` is therefore absent, and `find_tsserver()` returns `None`
before any test body runs — so **every `*_tsserver` real-provider test silently early-returns in CI**.

This is not theoretical. A full local workspace run with a BUILT tree and a real tsserver shows **six
`*_tsserver` real-provider tests failing** (code-action ×3, hover ×2, rename ×1). They have been invisible
because the only lane that would execute them does not.

The asymmetry is the point: the tsgo half of the suite is REQUIRED to run; the tsserver half is free to
vacuously pass. A gate that cannot fail is not a gate.

**Do not "fix" this by simply adding `VERTER_REQUIRE_TSSERVER=1`** — that turns CI red immediately on six
pre-existing failures. It needs its own dispatch: build the TS packages in the Rust jobs, require tsserver,
and fix (or explicitly quarantine, with named owners) the six failures in the same change.

The `tsserver_plugin_probe` added here is the loud half of the fix: where a tsserver IS present but the
plugin cannot load, the harness now fails hard instead of serving silently-empty diagnostics, no rename
edits, and false `TS2307`. It does not, and cannot, make CI run a suite CI never asks for.

### 6.2 Engine-setting changes are dropped mid-startup and mid-restart

A watched engine change arriving while `client.start()` is in flight is lost (`server` still undefined);
one arriving during an existing restart is lost too (`restartLS` early-returns on `restarting`). The server
then keeps the PREVIOUS engine indefinitely. Watching a setting and dropping its notification is the same
defect as not watching it, one layer down. Correct fix is restart-generation tracking (record the pending
generation even during startup/restart; restart until the launched generation is current) — a lifecycle
change, not a patch, so it is deliberately NOT shimmed here.

### 6.3 The SHARED shim is reused across engine-affecting restarts

The shared launch is created once and reused on every restart, so an engine-path change restarts the owned
server on the NEW engine while the shim still runs the OLD one — diagnostics composed from two TypeScript
versions. **Currently latent**: `shared-tsgo` is not in the `verter.typeProvider` enum and `auto` never
selects it, so nothing serves through the shim in production. It becomes live the moment the relay is wired,
which is exactly when it will be hardest to see. Needs shared-launch teardown/re-establish on engine change.

### 6.4 The Native Preview attestation is still a TRANSCRIPTION, not a test

The claim that Native Preview holds a per-workspace *consent* state (separate from workspace trust) gating
whether it actually uses a workspace-scoped tsdk COULD NOT BE ADJUDICATED. If true, Verter can attest tsgo
and hand over a workspace engine while the editor is really running its bundled one — a FALSE attestation.

`.d1a-engines/np-0604/package/` is the npm PLATFORM BINARY package (`@typescript/native-preview-win32-x64`):
stdlib `.d.ts` plus `lib/tsgo.exe`, and **zero editor-side code**. It is not the VSIX and cannot answer the
question. Resolving this needs the real installed extension bundle.

Until then the two shipped Microsoft gates this change reproduces are a **transcription of minified code**,
pinned by no test against the shipped artifact. That is the standing liability of this whole approach
(see §5) and it is now a concrete, named open question rather than a general worry.

### 6.5 The untrusted-workspace tsdk gate is currently COSMETIC — and the real exposure is server-side

Read this before trusting the client-side trust filter.

1. **The gate is unreachable.** `packages/vue-vscode/package.json` declares **no `capabilities` key at all**.
   VS Code therefore disables the extension entirely in Restricted Mode, so `workspace.isTrusted === false`
   never reaches our code. The filter that rejects a relative user-scoped tsdk in an untrusted workspace is,
   today, **dead code guarded by a discriminating test**. The test is real; the branch it guards cannot run.

2. **The gate is lexical.** It admits any *absolute* user-scoped path — including one that points INSIDE the
   opened repository, or a symlink out of it. Absoluteness is not provenance.

3. **The server does not enforce it.** `verter_type_runtime`'s tsgo discovery has **no notion of trust**. Tier 2
   probes and spawns `<workspace>/node_modules/@typescript/.../tsc(.exe)` unconditionally. A hostile repository
   does not need the tsdk setting at all — it just ships the binary. This is PRE-EXISTING, not a regression of
   this change, but it means the client-side gate is not a boundary: it is a lock on one door of an open house.

The honest statement of what landed: a correct, tested filter on a branch that cannot currently execute, in
front of a server that would ignore it anyway. If workspace trust is ever declared in the manifest, (2) and (3)
must be closed IN THE SAME CHANGE, and the enforcement belongs in the SERVER (the thing that spawns), not the
client (the thing that suggests).

---

## 7. Discovery is candidate-arity, and the workspace tier understands hoisting

Two closures, each landed with discriminating tests. Both change ARITY and REACH, never
precedence: the tier order (operator override, workspace `node_modules`, editor-supplied,
npm/npx cache) is byte-identical.

**One broken binary no longer demotes the family.** Discovery
(`find_tsgo_binary_candidates`) yields the ordered candidate list across all four tiers;
`find_tsgo_binary_canonical` is exactly its head. The spawn path
(`spawn_first_runnable_tsgo`) attempts candidates in tier order and serves the first that
starts; only when EVERY candidate has failed does the `TsgoUnrunnable` witness exist, and
`decide_with_tsgo_unrunnable` — the family demotion — is unreachable without it. Before
this, an editor handing over a broken engine while a working engine sat in the npx cache
got TSSERVER, while silence got TSGO: the observation did worse than silence, which the
decision docs state as impossible. Pinned by
`a_broken_editor_engine_never_leaves_the_workspace_worse_than_silence` (end to end across
the real tiers), `the_family_demotes_only_when_every_candidate_is_exhausted`, and
`discovery_lists_every_candidate_across_tiers_in_order`.

**The workspace tier resolves hoisted monorepo installs.** pnpm / npm / yarn workspaces
install TypeScript in the PROJECT ROOT's `node_modules`; opening a member package used to
miss it, so the editor's engine (tier 3) beat the workspace's own — falsifying "the editor
supplies only what the workspace lacks" in the ordinary Vue monorepo layout. Tier 2 now
probes from the workspace root up to the ENCLOSING PROJECT ROOT
(`workspace_tier_probe_dirs`): ascent stops at the first install/workspace/repository
marker (`pnpm-workspace.yaml`, lockfiles, `.git`), inclusive; with no marker anywhere
there is NO ascent (fail-closed); a `node_modules` above the project root is never
resolved. The probed roots stay inside the repository the user opened, so §6.5's tier-2
exposure is unchanged in kind, and the tsserver twin's unmarked 10-ancestor walk (§4)
remains its own finding. Pinned by
`workspace_tier_finds_the_hoisted_monorepo_engine_over_the_editors` and
`hoisted_engine_walk_never_escapes_the_project_root`.

The §2 precedence question — editor parity vs. project parity when a workspace OWNS an
engine — remains deliberately unsettled: the workspace tier still beats the editor tier,
now also when the workspace's engine is hoisted.
