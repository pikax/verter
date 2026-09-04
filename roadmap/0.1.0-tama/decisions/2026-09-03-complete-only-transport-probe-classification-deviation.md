# Recorded deviation: complete-only route classification on the WASM transport probe

- Status: **ratified 2026-09-04 — architect ruling, option (b), charter
  amended to the two-route contract**
- Date: 2026-09-03
- Node: `CCA1O3B` — "WASM transport-probe host-request migration"
  (`charters/compiler-compiler-bridge/CCA1O3B.md`)
- Adds: this record, plus an amendment section appended to CCA1O3B's own charter
  that points here. Amends no other node's charter, budgets, boundaries,
  predecessors, or ledger line. Adds no DAG node.

## Why this is recorded rather than decided locally

`CLAUDE.md` — "Where implementing the plan appears impossible, record a
deviation for maintainer ratification rather than substituting a local
decision — an unrecorded deviation is far more expensive to unwind than a
delay." The CCA1O3B implementation hit exactly that condition twice and
resolved it in code. This record is the missing half.

## The conflict

CCA1O3B's charter states three things that cannot all hold at once for the
route it mandates.

1. **Surfaces:** "missing-versus-refused classification … remain unchanged."
2. **Acceptance:** "Probe output keys, ordering, output/map/refusal
   classification, canonical IDs, and serialized offsets are equivalent."
3. **Aborts:** "Abort on a deleted probe axis, duplicate WASM execution,
   changed normalization, or transport divergence."

against, in the same charter:

4. **Surfaces:** "Each becomes one typed `compileRequest`; the IDE cases stop
   being an ensure-then-read pair."
5. **Deletions:** "Delete no WASM compatibility type, binding decode, probe
   case, output key, or comparison rail."

The typed `compileRequest` route is complete-only by construction
(`VerterHost::compile_request`, `crates/verter_session/src/host_resolve/compile_request_execute.rs`;
the binding at `crates/verter_wasm/src/lib.rs`): a refusal at any stage fails
the WHOLE request and publishes no product, and every accepted demand
compiles. That eliminates two intermediate states the profile-cached route
has, and two probe cases classified exactly those states.

### Case A — `svelteServerStyle`: `missing` → `error`

The demand is the CSS node of a Svelte server runtime surface the carrier
refuses. On the profile-cached route the refused compile still leaves a
per-node read that finds no such node, so the case classifies `missing`. On
the complete-only route no product is assembled, so there is no node of it to
be absent; the same demand surfaces the carrier's typed refusal
(`svelte-runtime-unsupported-server-generate`), classified `error`.

There is no third answer. Preserving `missing` would require the probe to
fabricate a classification the wire does not produce.

### Case B — `getIdeWithoutMap`: `missing` → `published`

The demand is a map-less IDE surface. On the profile-cached route this was a
`get_ide` read against a profile nothing had ensured, so there was no cache
slot to serve and the case classified `missing` — an artifact of the
ensure-then-read pair, which point 4 above explicitly directs this node to
remove. On the complete-only route every IDE demand compiles, so the
projection publishes and the case becomes the IDE surface's optional-product
axis (the map is withheld; the bytes are unchanged).

Preserving `missing` would require keeping the ensure-then-read pair the
charter directs this node to remove, or issuing a demand the probe does not
make.

## What the implementation did

`assert_transport_matches_the_host_route`
(`crates/verter_session/src/framework/transport_route_equivalence_tests.rs`)
gained a two-variant `ProductRoute` discriminator, and each transport is
compared against the in-process host's answer to ITS OWN demand:

- `ProfileCachedReads` (NAPI) — unchanged expectations, plus a new staleness
  guard asserting the host still answers `Missing` for the refused compile's
  style node.
- `TypedCompileRequest` (WASM) — `svelteServerStyle` is compared against the
  host's own `VerterHost::compile_request` failure for the identical typed
  server-runtime demand (not against a per-node profile read, which answers a
  different question); `getIdeWithoutMap` is compared against the host's
  map-less IDE product, with the bytes pinned equal to the mapped demand's.

No probe axis, output key, or comparison rail was deleted. Both cases still
run on both transports, both are still compared against the host, and neither
may carry bytes for a refusal.

## What is asked of the maintainer

Ratify ONE of:

- **(a) Accept the deviation as scoped here.** The abort clause's "transport
  divergence" is read as "a transport whose answer diverges from the host's
  answer to the same demand", not "two transports classifying different
  questions identically". The charter amendment below is the durable record.
- **(b) Amend the charter.** Replace the "transport divergence" abort clause
  and the "classification … equivalent" acceptance line with the explicit
  two-route contract, so future work on this file reads the real invariant.
- **(c) Reject.** Then CCA1O3B is not implementable as chartered and needs a
  rescope — the complete-only route admits no other classification for these
  two cases.

Until the ruling, this record stood as the disclosure; it was not itself an
approval.

**Ruling, 2026-09-04 (architect): option (b).** The charter's "classification …
equivalent" acceptance line and "transport divergence" abort clause are
replaced by the explicit two-route contract, so future work on this file
reads the real invariant. The charter's deviation section now records the
ruling; the implementation as scoped under "What the implementation did"
is the shape the amended charter mandates.

## Known residual, disclosed

On the complete-only route `svelteServerStyle` and `svelteServerRefusal`
produce byte-identical records: with nothing assembled, the CSS demand and the
main-node demand cannot differ, and the `kind`/`index` the case states is
never consulted. Its residual value there is the no-bytes check. Restoring
per-product refusal granularity requires a wire that can answer per product
and belongs to whoever introduces partial-product responses; the limitation is
stated at the assertion site so no reader mistakes it for a second rail.

## Consequences

- No other node's budget, boundary, predecessor list, or ledger line changes.
- The NAPI transport's classifications are untouched.
- The `ProductRoute` discriminator is the durable mechanism either ratification
  branch (a) or (b) keeps; only branch (c) would remove it.

## Disclosed adjacent gaps

Three findings surfaced during review. Gap 2 is a mechanical compile repair
this candidate had to carry. Gap 1 is this node's evidence obligation: the
typed-vs-profile comparison cannot be proven by compilation. Gap 3 is not
this node's work and is not repaired here.

### 1. The suite needs an execution gate — `TRANSPORT-SUITE-UNGATED`

- **Finding:** `transport-authoritative` compiled in no CI job and ran in no
  `scripts/gate.mjs` lane. Syntax and Clippy cannot detect a behavioral
  divergence between the typed WASM request and the in-process profile
  route — the comparison this node now asserts for the first time.
- **Disposition:** `ADOPT-NOW`. `.github/workflows/ci.yml` keeps the cheap
  Clippy compile of the feature (signature drift) and adds
  `transport-route-equivalence`, which downloads the `wasm-build` and
  `build-native-node` artifacts, runs
  `cargo test -p verter_session --lib --features transport-authoritative`
  on an exact allowlist of the nine native/WASM/census test names (no
  substring skips, so a later test cannot be silently dropped for its
  name's tokens), and fails unless the `running N tests` line is present,
  at least 9, and names
  `the_wasm_transport_matches_the_in_process_host_route` and
  `the_napi_transport_matches_the_in_process_host_route`.
- **Scope:** one extra workflow file. Related packages stay at one
  (`@verter/wasm`). Under the charter's rescope trigger of 3 files / 2
  unrelated packages. Recorded here rather than silently absorbed.
- **Residual:** the bundler lane is not in that job (see gap 3). Clippy
  still cannot prove it compiled the `cfg`-gated module if the `mod` line
  is dropped; the execution job is that proof.

### 2. The suite did not compile at the merge base — `TRANSPORT-SUITE-STALE-CALL`

- **Finding:** `b71832360` (2026-09-02, "Exact style identity and owner-domain
  reuse") inserted `input_stage: CascadeInput` as `transform_vue_style`'s
  second parameter and turned the style outcome's `code` field into a `code()`
  method. The suite's two call sites still passed the pre-change 8-argument
  form and read `.code`, so the merge-base tree was UNCOMPILABLE under
  `transport-authoritative`.
- **Consequence for evidence:** this charter's acceptance leans on "the
  existing native/WASM transport comparison" as prior proof. That comparison
  could not have executed at the merge base, so no evidence is inheritable and
  the suite must be run on this exact tree with freshly built artifacts.
- **Disposition:** `ADOPT-NOW`, minimally. The patch repairs the two
  call sites and the three accessor reads because the suite cannot run
  otherwise. This is someone else's breakage carried by this candidate, not
  this node's work, and it is disclosed here rather than folded in silently. It is
  a mechanical signature update; it changes no assertion and no expectation.
- **Root cause:** gap 1. A gated suite would have failed `b71832360`.

### 3. The bundler lane of the same suite is red at HEAD — `BUNDLER-PROBE-FRESHNESS-PIN-STALE`

- **Finding:** `packages/unplugin/scripts/probe-bundler-route.freshness.json` pins
  the plugin source and dist digests the bundler probe requires. `4d92d368d`
  (2026-08-29, "align Vue types and Vite style staging") changed
  `packages/unplugin/src` without regenerating that pin, so all bundler-lane
  tests in `transport_route_equivalence` fail their artifact-freshness check
  before reaching any assertion — even with `pnpm --filter @verter/unplugin build`
  freshly run.
- **Not introduced here.** `4d92d368d` is an ancestor of this branch. This
  candidate does not touch `packages/unplugin/`. The native, WASM,
  audited-compile, missing-node and enumeration lanes are the lanes this
  node needs, and they do not consume that pin.
- **Disposition:** `DEFER`, deliberately NOT repaired here. Regenerating the
  pin, or changing bundler cache-key handling, is a different package's
  production surface. Absorbing a red the owning surface should see, or
  landing unrelated plugin behavior in this node, is refused.
- **Durable owner:** `@verter/unplugin` (the next production change to
  `packages/unplugin/src`).
- **Resolution gate:** no later than plan close; the acceptance test is the
  `transport-authoritative` bundler lane going green against a pin produced
  by a checked-in generator, not a hand-edited digest.
- **Ruling reference:** this record.
