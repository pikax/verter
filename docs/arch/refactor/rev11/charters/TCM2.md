# TCM2 — Content-mapper projection plane

**Status:** PREPARED — charter structure implementation-ready (5-part shape, numbered exit criteria).
Digest-bound authorization record still required before dispatch; ledger status stays LOCKED until TCM0
is ACCEPTED and TCM1's mapping contract is ACCEPTED. TCM0's own topology/performance-number gaps are
tracked in `evidence/TCM0/OPEN-GAPS.md` and gate TCM0's acceptance, not this charter's readiness.
**Predecessors:** TCM0, TCM1. **Downstream:** TCM4.
**Dormant: unregistered and unreachable from production routing until TCM4.**
**Authority:** `rulings/MAINTAINER-STEERING-TCM-CONTENT-MAPPERS.md` §"Projection plane", global locks
§1-2, §9, §11-13; `evidence/TCM0/package-lock-and-semantic-api.md` §3 (protocol shape, live-verified);
`evidence/TCM0/acyclic-invariant-test-spec.md` (the discriminating test this block implements);
`evidence/TCM0/external-source-decision-table.md`; `evidence/TCM0/projection-class-contract.md`;
`evidence/TCM0/distributed-lifecycle-contract.md`; `rulings/
MAINTAINER-RULING-TCM-PACKAGE-CERTIFICATION-SETTLED.md`.

## 1. Intent contract

**Actor / problem.** TypeScript calls Verter's content mapper (`Initialize`/`OpenProject`/`Transform`/
`CloseProject`, confirmed present in the certified candidate's native binary,
`package-lock-and-semantic-api.md` §3) to obtain generated TSX, span mappings, mapper diagnostics,
directives, and supplemental outputs. Today this role is played by a relay/carrier/plugin stack this
program is retiring. TCM2 builds the ONE process that plays this role going forward, dormant until TCM4
wires it into production.

**Required observable outcomes.**
- Exactly one current content-mapper codec ships — the one TCM0 certified.
- Every emitted `SpanMapSegment` carries an explicit, computed `features` mask (never omitted — an
  omitted mask silently normalizes to `SpanMapFeature.All` upstream, `projection-class-contract.md`).
- The mapper is provably pure with respect to TypeScript semantic state: it cannot reach
  `TypeSemanticOracle`, TypeScript LSP, an API-session bridge, a project-snapshot wait, or the old
  provider/relay from inside a `Transform` handler.
- `TypeScriptSingleInputProjection` is validated at the constructor: every serialized original range
  belongs to the exact input file and revision TypeScript supplied.

**Forbidden observable outcomes.**
- Any second codec, versioned-vs-versionless dual init path, `V1 | V2` runtime enum, or fallback codec.
- Any transform path that can reach TypeScript semantic state before its `Transform` call returns (the
  acyclic-invariant violation this block exists to make structurally impossible, not merely untested).
- Registration or reachability from production routing before TCM4 (no flag, no env var, no second
  provider selection, no hidden fallback).

**Authority / fallback order.** The mapper calls the canonical Verter compiler/cache implementation for
its own process — it is a NEW transport over EXISTING compilation, never a second compiler. There is no
fallback: an unknown future contract fails closed until Verter is updated (steering global lock §1).

## 2. Owned scope

1. **A separately versioned package**, provisionally `@verter/typescript-content-mapper`. Does NOT
   overload `@verter/typescript-plugin` (which TCM4 deletes, `deletion-closure.md` item 1).
2. **Exactly one codec** — the one TCM0 selected (`package-lock-and-semantic-api.md` §3). Not both
   versioned and versionless init codecs; not a `V1 | V2` runtime enum; not the TypeScript-Go historical
   protocol; not semver-selected wire formats; not a fallback codec; not a compatibility parser spanning
   protocol generations.
3. **Close the exact wire method-name spelling gap** TCM0 left open (`package-lock-and-semantic-api.md`
   §5: structural Go type-name evidence for the four-step lifecycle exists; a byte-exact wire trace does
   not) — via live protocol trace or `typescript-go` source read, before claiming byte-exact protocol
   fidelity (`tcm1-tcm4-charter-refinements.md`'s TCM2 note).
4. **Speak the current JSON-RPC protocol over stdio**, writing ONLY protocol frames to stdout; logs and
   telemetry route through a separate non-protocol channel.
5. **Call the canonical Verter compiler/cache implementation** for its process — no second compiler.
6. **Purity / the acyclic invariant**, implemented per `acyclic-invariant-test-spec.md`: a sealed,
   type-state `Transform`-handler context exposing exactly the inputs `Transform` needs (filename,
   content, project handle) and structurally incapable of reaching a semantic-oracle client, enforced by
   Rust module privacy/visibility — not a runtime `assert!`. Both halves of the spec's discriminating
   test: (a) a `trybuild`/compile-fail proof that the forbidden call cannot even be written inside the
   sealed context; (b) a deliberately-broken control-build deadlock reproduction proving the test would
   catch the cycle if the structural guard were ever bypassed.
7. **`MapperProcessProjectState`** (`distributed-lifecycle-contract.md`) — isolate state by TypeScript's
   opaque project handle; serve several handles in arbitrary order; release state on `closeProject`;
   bound message sizes, queues, outstanding work, handles, and caches; support cancellation where the
   protocol permits.
8. **Config/watched-file identity, primary and supplemental outputs.** Route Verter's existing
   `VirtualFileNaming` companion-suffix outputs through the protocol's native `SupplementalOutput` field
   (`external-source-decision-table.md` row #10) — this SUPERSEDES today's convention, it does not
   approximate it alongside a second parallel mechanism.
9. **`TypeScriptSingleInputProjection` validation** (steering §11): the constructor proves every
   serialized original range belongs to the exact input file/revision supplied. Never encode source-unit
   IDs into offsets, map foreign-file offsets onto the component, silently take the first source unit,
   blindly drop foreign spans, or serialize `PlacementMap` as a span map.
10. **Explicit feature-mask emission** on every wire span, per `projection-class-contract.md`'s five-class
    terminal policy (class × relation × region × owner × certified capability → mask). Never omit
    `features` — an omission silently normalizes to `All` upstream.
11. **Diagnostics and directives**: mapper diagnostics and diagnostic directives, per
    `diagnostic-ownership-matrix.md`'s mapper-diagnostic/directive rows.
12. **Signed `int32` bounds validation** before serialization; every wire offset and end in
    `0..=i32::MAX`; overflow is a typed mapper error, never a wrap/saturate/truncate/reinterpret.
13. **Preserve exact project/compiler options**; conform across project references, monorepos, build,
    watch, incremental operation, declarations, and declaration maps.
14. **Do not treat TypeScript-created virtual filenames as stable Verter identities.**
15. **Telemetry**: expose complete timing, cache, allocation, IPC, memory, and lifecycle telemetry.

## 3. Owned-scope boundary (what TCM2 does NOT own)

- No TypeScript semantic-API calls, no LSP requests, no project-snapshot waits — that is the acyclic
  invariant's forbidden edge, structurally impossible from inside `Transform` (owned-scope item 6).
- No `TypeScriptApiSessionState` or `VerterSemanticClientState` implementation — those are TCM3's local
  owners (`distributed-lifecycle-contract.md`).
- No production registration, no editor activation, no `tsconfig` mutation — TCM4 only. TCM2 stays
  dormant and unreachable from production routing.
- No feature-ownership decisions beyond emitting the mask `projection-class-contract.md` already
  specifies — TCM2 encodes the ratified policy, it does not re-derive ownership.

## 3a. Timing taxonomy

Every TCM2 timing-sensitive mechanism is classified using `architecture.md` §1.6.

- The `@verter/typescript-content-mapper` JSON-RPC/stdio boundary is **external liveness**: protocol
  completion plus one independent real monotonic watchdog. Completion is never inferred from sleep,
  handler idle, or pseudo-idle, and is never discovered by polling when a protocol frame or OS
  completion exists.
- Bounded queue admission (owned-scope item 7: message size, queue depth, outstanding work, handles,
  caches) sits inside cancellation and one absolute deadline. Admission, execution, and response share
  that deadline; no second timeout begins after dequeue.
- TCM2 is the sole acceptance and cutover owner of mapper-protocol admission, cancellation, and
  deadline. G3 may supply a reusable bounded-admission primitive that this path consumes; G3 does not
  implement, accept, or delete the mapper JSON-RPC path. This is the same single-owner split as H2's
  `ClientHandle::request`, not a second admission owner beside G3.
- TCM2 does not introduce a generic coordinator duplicating G2's `FlightCell`. Mapper-process
  `MapperProcessProjectState` queues are TCM2-owned protocol admission, not a second query-runtime
  flight cell.

## 4. Numbered exit criteria

1. **Exactly one codec ships.** Evidence: a negative test asserting no second codec path, versioned/
   versionless dual-init branch, or `V1 | V2` enum exists in the mapper's init handling.
2. **Wire method-name spelling closed.** Evidence: a recorded protocol trace or `typescript-go` source
   citation naming the exact `initialize`/`openProject`/`transform`/`closeProject` (or their actual
   spellings) wire method names, replacing TCM0's "structural evidence only" hedge.
3. **The acyclic-invariant discriminating test passes both halves.** Evidence: the `trybuild` compile-fail
   fixture (structural half) and the deliberately-broken control-build deadlock reproduction (runtime
   half), both named per `acyclic-invariant-test-spec.md`, both present in the test suite.
4. **No emitted `SpanMapSegment` omits `features`.** Evidence: a negative test asserting every code path
   that constructs a wire segment computes an explicit mask — the exact defect class
   `projection-class-contract.md` names as forbidden.
5. **`TypeScriptSingleInputProjection` constructor-level proof.** Evidence: a test asserting a
   cross-source-unit range is REJECTED at construction (not silently dropped, not silently included), for
   at least one fixture per external-source shape in `external-source-decision-table.md` that reaches the
   mapper. **For `<template src>` specifically, this negative test is necessary but not sufficient**: the
   steering permits content-mapping it only under model 2, "independently content-mapped under a proven
   project/context contract" (`rulings/MAINTAINER-STEERING-TCM-CONTENT-MAPPERS.md` §11) — see
   `evidence/TCM0/OPEN-GAPS.md`'s `G-TEMPLATE-SRC-PROJECT-CONTEXT-CONTRACT` row. TCM2 additionally
   provides a POSITIVE fixture proving: the mapper's `transform()` input for the external file is that
   file's own content, distinct from the referencing SFC's; which TypeScript project owns the external
   file for content-mapping purposes; and which `tsconfig` identity applies to it. Without this positive
   proof, `<template src>` is not yet resolved under a proven model and TCM2 may not claim it is.
6. **Supplemental outputs route through the protocol's native field.** Evidence: a test asserting today's
   `VirtualFileNaming` companion suffixes (`ide`, `import_surface`, `testing_api_suffix`, `sidecar_suffixes`,
   `declaration_surface`) are all reachable via `SupplementalOutput`, with zero remaining parallel
   companion-file convention for content the mapper protocol now owns.
7. **Signed-offset boundary proof.** Evidence: fixtures at `i32::MAX` and one past it, proving overflow
   returns a typed mapper error before serialization, never wraps/saturates/truncates.
8. **Purity negative test set** (owned-scope item 6): a hard test that FAILS if any transform path can
   reach `TypeSemanticOracle`, TypeScript LSP, an API-session bridge, a TypeScript project-snapshot wait,
   or the old provider/relay — named explicitly in the charter per the steering's own instruction ("Add a
   hard test that fails if any transform path can call...").
9. **Dormancy proof.** Evidence: a test or static check confirming zero production registration/reachability
   of the mapper package before TCM4 — no feature flag, no env var, no second provider selection path.
10. **Bounded-state proof** (owned-scope item 7): tests confirming message size, queue, outstanding-work,
    handle, and cache bounds are enforced, not merely intended.
11. **Cross-project-reference/monorepo/build/watch/incremental/declaration conformance fixtures** pass, per
    steering's "Required conformance coverage" list, scoped to TCM2's projection-plane responsibilities.
12. **Mapper-protocol admission is inside cancellation and one absolute deadline.** Evidence: a test that
    fails if reservation of a full mapper queue starts a second timeout after dequeue, or if a waiter
    cancelled before admission is still charged a slot. G3 tests do not accept this path.
13. **JSON-RPC completion is external liveness.** Evidence: real-process mapper tests use protocol
    completion plus one independent real monotonic watchdog; a test fails if sleep, idle, or polling
    substitutes for a protocol frame.

## 5. Forbidden

- A TypeScript-Go historical protocol, semver-selected wire formats, a fallback codec, or a compatibility
  parser spanning several protocol generations.
- Writing anything but protocol frames to stdout (logs/telemetry MUST use a separate channel — a stray
  `println!`/log line on stdout corrupts the JSON-RPC stream).
- Any call from inside `Transform` to `TypeSemanticOracle`, TypeScript LSP, an API-session bridge, a
  project-snapshot wait, or the old provider/relay.
- Encoding source-unit IDs into TypeScript offsets; mapping an external file's offsets onto the component
  file; silently selecting the first source unit; blindly discarding foreign-origin spans; serializing
  `PlacementMap` as a TypeScript span map.
- Treating a TypeScript-created virtual filename as a stable Verter identity.
- Any production registration, feature flag, environment-variable back door, or extension activation
  before TCM4.
- A second parallel supplemental-output convention alongside the protocol's native `SupplementalOutput`
  field.
- Inferring mapper-protocol completion from sleep, handler idle, or pseudo-idle; starting a second
  timeout after queue admission; G3 implementing mapper JSON-RPC admission; a local duplicate of G2's
  `FlightCell` inside the mapper process.

## 6. Material bounds

Per `performance-baselines.md` (locked before implementation):

1. **Warm/unchanged transform must be near-zero cost** — a HARD REQUIREMENT: a repeat `transform()`/
   `updateSnapshot()` with no content change must not cost materially more than the client-side cache
   lookup alone (order of the single-topology 0ms reference point, achieved correctly rather than as a
   symptom of the §4c stale-cache defect).
2. **Cold-start ceiling**: no TCM2 topology may regress cold start beyond the single-topology reference
   point (34ms `API` construction + 1037ms first `updateSnapshot` for a 1-file fixture) by more than a
   small constant factor attributable to genuine additional work (e.g. an extra process for a daemon
   topology); any larger regression must be justified in the topology write-up, not silently accepted.
3. **Zero process/fd leaks across 100 open/close cycles** — hard requirement.
4. **Interactive-tier features must not regress versus today's relay-based latency**, even though their
   owner changes (`feature-ownership-ledger.md` sub-rows `a`) — the new path must be shown at least as
   fast as the measured relay baseline before TCM4 may delete the relay code currently serving them.
5. **The debounced background-diagnostics 300ms silence window is unchanged** — widening it is a rescope
   trigger, not a quiet adjustment.
6. Thresholds above were locked in `performance-baselines.md` BEFORE any TCM2 implementation result
   existed, per the steering's own ordering rule ("Do not choose acceptance thresholds after viewing
   implementation results") — TCM2 may not renegotiate them post hoc.
7. Package certification is settled (`rulings/MAINTAINER-RULING-TCM-PACKAGE-CERTIFICATION-SETTLED.md`);
   the reproduced stale-`Program`-after-dispose defect is NOT TCM2's concern (TCM2 never holds a
   `Program`/`Checker` handle) — it binds TCM3.

## Abort / rescope

Per steering global abort conditions, applied to TCM2: a mapper transform can re-enter TypeScript; normal
Verter compilation crosses JSON-RPC; a multi-source projection is falsely encoded as single-source; old
and new transport routes coexist; deletion is deferred to another release; a required performance/memory
gate would need weakening to pass.
