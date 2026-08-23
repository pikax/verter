# TCM0 — Acyclic invariant: the discriminating deadlock/reentrancy test (spec only; TCM2 implements)

The charter is explicit that TCM0 SPECIFIES this test and TCM2 IMPLEMENTS it. This file is the
specification only — no test code lands in this block.

## The invariant

"The mapper callback must never query the TypeScript semantic API or send LSP requests. The only legal
order is: TypeScript requests transform → Verter compiles and returns output plus mappings → TypeScript
commits its snapshot → Verter may then acquire that snapshot → Verter-owned operations may query it."

Restated as a directed constraint on the two protocols this program locks (both confirmed to genuinely
exist in the candidate package, `package-lock-and-semantic-api.md` §3-4): the content-mapper protocol
(`Initialize`/`OpenProject`/`Transform`/`CloseProject`, TypeScript-as-client, Verter-as-server) and the
semantic-capability protocol (`initialize`/`updateSnapshot`/`release`/query methods, Verter-as-client,
TypeScript-as-server, `package-lock-and-semantic-api.md` §7 in the sub-investigation). The forbidden edge
is: **while executing a `Transform` call (Verter acting as content-mapper server), Verter's own code
issues a semantic-capability-protocol call back into the SAME TypeScript process** (or sends any LSP
request to the editor) before that `Transform` call returns.

## Why this is checkable structurally, and why it must be (not just tested at runtime)

A single missed runtime test case proves nothing about the general absence of the cycle — this needs a
STRUCTURAL guarantee (consistent with the codebase's existing "landed guards are structural, not
name-keyed scanners" discipline) plus a runtime test that exercises the structural boundary under load.

**Structural half (TCM2's job to build, specified here):** the code path executing inside a `Transform`
handler must be given a capability/type-state token that provably cannot reach the semantic-capability
client (no `Arc<dyn SemanticOracleClient>`, no equivalent, reachable from the `Transform` handler's
argument or ambient state). This is the same pattern as `FrameworkAdapterCtx`'s closed, facts/carrier-only
surface (CLAUDE.md's Framework Adapter Substrate section: "It never resolves types, indexes a file, runs
OXC, calls `ProjectSemanticDispatch`, or reads a `StoreView`") — TCM2's `Transform` handler context should
be built the same way: a sealed struct exposing exactly the inputs `Transform` needs (filename, content,
project handle) and nothing that could reach the semantic-capability client, enforced by Rust's module
privacy/visibility, not by a runtime `assert!`.

**Runtime half (the actual discriminating test, TCM2 implements):**

1. Spawn a real (or faithfully faked) TypeScript host that calls `Transform` on Verter's content-mapper
   server for a fixture file.
2. Inside the `Transform` handler (test-only instrumentation), attempt the forbidden call: issue a
   semantic-capability-protocol request to the same TypeScript process, or send an LSP request to a
   stand-in editor endpoint.
3. **Pre-condition for a valid test**: the attempt must be a COMPILE-TIME impossibility (the handler has
   no value of the required type in scope) wherever the structural half above is actually in place — so
   the "test" at this layer is a `trybuild`/compile-fail assertion (the handler function body cannot even
   be written to attempt the forbidden call), not a runtime probe. This makes the test discriminating in
   the sense CLAUDE.md's Stub Prevention section requires: it must fail against a pre-fix tree (one where
   the `Transform` handler still holds a reachable oracle-client handle) and pass against the post-fix
   tree (the sealed-context tree) — exactly the characterization-test property CLAUDE.md demands.
4. **Deadlock reproduction, separately:** a live test that has TypeScript call `Transform`, and — only in
   a DELIBERATELY-BROKEN control build (not production) — has the handler synchronously call back into
   the same TypeScript process's request queue, and asserts the call times out / the test harness detects
   a hang within a bounded wall-clock budget (mirroring this investigation's own bounded-probe discipline —
   see `package-lock-and-semantic-api.md` §4a). This control build proves the test WOULD catch the cycle
   if the structural guard were ever bypassed (e.g. by a future refactor that accidentally widens the
   `Transform` handler's context) — satisfying "a characterization test must be writable such that it
   FAILS against the pre-change tree AND PASSES against the post-change tree."

## What TCM0 records as already-verified relevant to this invariant

- The upstream protocol design itself is single-directional per call (`Transform` returns synchronously
  with output + mappings; it has no callback/query sub-protocol back to the caller) — confirmed by the
  full `APIMethodInfo` table quoted in `package-lock-and-semantic-api.md` §4.0 having no
  content-mapper-initiated method. The cycle, if it exists, would be a Verter-introduced bug (calling out
  from inside the handler), not something the upstream protocol invites.
- The native binary's own process-teardown code independently documents one deadlock class it already
  guards against (`dist/api/async/client.js:193-212`, `package-lock-and-semantic-api.md` §4d) — evidence
  that this class of bug is a real, known risk category in this exact protocol family, reinforcing why
  TCM2's structural guard is not defensive-programming theater.

## What this file does not do

It does not write the sealed-context type, the `trybuild` fixture, or the deadlock-control-build harness —
all of that is TCM2's implementation. This file is the specification those artifacts must satisfy.
