# AMD-005 governance/DAG impact-bounded reattestation

**SUPERSEDED AFTER REBASE.** This historical impact-bounded report remains bound to
`6920ddc6feed70cd4b25eb3b557ceac66c535939`, tree
`7d38eb20dd152433a469811be82a61ba200a38c3`; it does not review or approve the
rebased bytes. The current independent governance report is
[`governance-challenge.md`](governance-challenge.md), bound to
`ce1d0e4688af1b5bd548b6b68286632cc0f7ede8`, tree
`1ff1f83d8e994b6f1169b0b209c9f557c23f4728`.

## Exact identity and scope

- previous candidate: `8fbef4ba2ce30d93a636f769639519df7a773a92`
- previous tree: `eba511f865239ac27abf7da4fd3b4d292ed9ebec`
- reattested candidate: `6920ddc6feed70cd4b25eb3b557ceac66c535939`
- reattested tree: `7d38eb20dd152433a469811be82a61ba200a38c3`

This is an impact-bounded reattestation of the four blocking findings in
`governance-challenge.md`, not a fresh review of the full package. The exact
candidate-to-candidate diff changes only:

- `docs/arch/refactor/rev11/charters/BV1.md`
- `docs/arch/refactor/rev11/charters/C3.md`
- `docs/arch/refactor/rev11/contracts/fragment-assembly.md`
- `docs/arch/refactor/rev11/evidence/framework-conformance/emitter-mapping-dispositions.tsv`

This report is a post-candidate review artifact and is not a member of the candidate
tree it reattests.

## Verdict

**PASS — all 4 original findings resolved, no new blocking issue introduced, bound
to commit `6920ddc6feed70cd4b25eb3b557ceac66c535939`.**

## Finding-by-finding reattestation

### 1. BV1/C3 acceptance deadlock — resolved

`charters/BV1.md:29-37` defines `FC-TS-001-LOCAL` as an independently closable BV1
partition. It covers the source-local PublicApi, TSC/TSX, and declaration cells plus
the producer/consumer behavior of deterministic protocol stubs, expressly requiring
neither a C3 implementation nor a live project resolver. BV1's exit now requires that
local criterion rather than the unsplit `FC-TS-001` (`charters/BV1.md:39-45`) and
leaves jointly owned project-aware cells `projection-required`.

`charters/C3.md:78-86` separately assigns `FC-TS-001-PROJECT` to C3's real
project/type-substrate integration and says that this later proof closes the parent
`FC-TS-001` for the jointly owned Vue cells. The split does not relocate the cycle:
the retained DAG is BV1 -> B5 -> C2 -> C3 (`program-dag.toml:100-139`), but BV1's
exit is now executable entirely before C3, while C3 deliberately consumes accepted
BV1 codegen. Both exits are explicit and independently satisfiable at their respective
DAG positions.

### 2. BV1-to-C3 demand protocol — resolved

`charters/C3.md:10-76` now supplies a closed protocol rather than an implementation
placeholder:

- common demand identity includes prepared-root identity, syntax/effective macro
  indices, role, lane, typed subject, semantic profile, and projection-plan token;
- the exhaustive demand variants are `MacroPayload` and `PropsWithDefaults`, with
  closed role/lane admissibility and ordered subject fields (`:23-31`);
- lane/variant-specific success payloads and the closed `DemandResult<T>` envelope are
  defined (`:39-58`), including exhaustive `NotFoundReason`, `StaleReason`, and
  `ErrorReason` vocabularies;
- deterministic identity, batch ordering, duplicate handling, and whole-batch
  rejection for missing, extra, duplicate, reordered, mismatched, or stale results are
  specified (`:60-68`); and
- omitted, `NotFound`, `Stale`, or `Error` results become typed
  `ProjectProjectionUnavailable` non-success with no artifact publication. Even an
  allowed member-level degradation must be explicit and diagnostic-bearing; empty or
  silently incomplete success is forbidden (`:70-76`).

BV1 independently proves its side with typed deterministic stubs
(`charters/BV1.md:29-35`), while C3 proves the real substrate behavior
(`charters/C3.md:80-85`). This closes both producer and consumer acceptance surfaces.

### 3. fragment assembly mapping contradiction — resolved

`contracts/fragment-assembly.md:23-30` now distinguishes contract-required mapping
parts from optional map products. Requesting an IDE/provider companion implicitly
requests its non-optional `SourceProjectionMap`, and companion plus map must be
produced, delivered, and published atomically. Only optional runtime/build map content
(`RuntimeSourceMapData` and terminal `EncodedSourceMap`) remains request-gated.

This is coherent with `contracts/mapping-products.md:10-13` and `:29-36`: an IDE
companion cannot publish without its interpretation map, runtime map construction is
skipped when not requested, and no universal empty map is fabricated. The earlier
“maps only when requested” ambiguity is removed rather than hidden.

### 4. emitter/mapping disposition completeness — resolved for all three named owners

The ledger adds three real, separately owned rows at
`evidence/framework-conformance/emitter-mapping-dispositions.tsv:39-41`:

- `EM-038` names `verter_session::compile::assemble_vue_main_module`, assigns
  `Replace`, and binds the Vue fragment, atomic assembly, and sole-direct-core cutover
  to BV1+B4+B5.
- `EM-039` names the primary Svelte `runtime/client.rs` and
  `client_module_frame.rs` surfaces, assigns `Converge`, retains Svelte-owned
  plan-to-fragment topology under BS1, and assigns final assembly/map composition and
  publication to B4.
- `EM-040` names `virtual_file_pipeline.rs::compile_entry`, assigns `Replace`, and
  binds atomic artifact replacement to B4+B5 plus project-aware route identity to C4,
  explicitly forbidding a second session assembly owner.

The cited paths were spot-checked read-only in the main checkout
`/Users/carlosrodrigues/Documents/dev/verter` at
`e6035b433352b106957f27f3e97b71911f39f9ae`:

- `crates/verter_session/src/compile.rs:21` defines
  `assemble_vue_main_module`; its body performs style/custom imports, Vue runtime/SSR
  imports, script/template/render binding, and HMR assembly.
- `crates/verter_compiler/src/svelte/runtime/client.rs:98` defines the primary client
  module entry. Its emitter builds imports, module statements/hoists, the component
  body, delegate/custom-element epilogues, and finishes code plus source map at
  `:353-465`. `client_module_frame.rs:19-51` emits the ordered import prelude and
  `:61-97` emits the root factory hoist.
- `crates/verter_session/src/host_resolve/virtual_file_pipeline.rs:2751` defines
  `compile_entry`; its publication path constructs Main, Script, Template, Style, and
  Custom virtual artifacts, injects Template imports, attaches/selects their maps, and
  constructs the cached IDE artifact (`:2996-3144`).

These are evidence-backed dispositions for each owner named in the original finding,
not path-only acknowledgements.

## Changed-scope sanity pass

The exact diff passes `git diff --check`. The new charter split preserves the existing
DAG while making each gate closable; the protocol is closed and fail-closed; the map
wording preserves mandatory interpretation maps without forcing unrequested runtime
maps; and the new ledger rows have unique owner IDs, six valid TSV fields, existing
source paths, concrete dispositions, and durable acceptance owners. No new blocking
issue was introduced by the four-file fix.

The review was read-only with respect to all candidate package files and the main
checkout. The only write was this new reattestation report.
