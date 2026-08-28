# TCM0 — TCM1-TCM4 charter refinements (proposal; charters stay LOCKED)

TCM1-TCM4 remain LOCKED — this file does not edit `charters/TCM1.md`-`TCM4.md` in place (those files are
committed text a future amendment/ratification act must change, not something TCM0 may mutate by
assumption). This is the recorded set of refinements this investigation's evidence supports, for a
maintainer or the amendment process to adopt when each block is authorized.

## TCM1 (Compact mapping products inside `CodeTransform`)

- **Correct the citation.** `TCM1.md` (and the amendment/discovery docs) state `checker.rs:411` calls
  `PositionMapper::from_json`. Verified false: that call exists only in a test
  (`crates/verter_lsp/tests/cases/kebab_tag_mapping_full_columns.rs:65`); `checker.rs:411` base64-encodes
  the string directly into a `sourceMappingURL` comment. See
  `evidence/TCM0/mapping-products-string-surface.md` for the corrected chain.
- **Scope the surface as wider than the two cited lines.** At least 36 distinct `Option<String>`/
  `String`/`Option<Arc<str>>`/`&str` fields (32+ across `verter_compiler`, 4 across `verter_protocol`'s
  FFI wire types) carry the same string-encoded convention — a best-effort STARTING inventory in
  `mapping-products-string-surface.md`, explicitly not claimed exhaustive (two manual passes each found
  the prior one incomplete; see that file's own hedge). TCM1's acceptance bar should include the FFI wire
  types, not stop at the in-process boundary — otherwise TCM1 leaves a second string-encoded path alive at
  the NAPI/WASM boundary, in tension with the "one clean cutover" rule.
- **~~Single point of origin.~~ CORRECTED BY THE SOURCE INVENTORY — `CodeTransform` is not the only point of origin.**
  This bullet originally read: *"TCM1 should replace the discard-to-string pattern at `CodeTransform`'s own
  `generate_map`/`generate_map_json*` (`code_transform/source_map.rs`), not at each downstream consumer
  site — the typed intermediate (`oxc_sourcemap::SourceMap<'static>`) already exists transiently at exactly
  that point and is thrown away by every current caller."* The premise was checked against source and is
  false: `CodeTransform` is one of **eight** in-repo producers of encoded map strings, and its two
  string-returning methods have zero production callers outside `crates/verter_compiler`. Replacing the
  discard there migrates seven call sites in one crate and leaves every map field in `verter_session`,
  `verter_lsp`, `verter_protocol`, `verter_napi` and `verter_dx_baseline` untouched. The enumerating
  instrument is a **value newtype retype**, not a producer replacement — see
  `mapping-products-string-surface.md`'s closure and `OPEN-GAPS.md`'s `G-STRING-SURFACE-CITATIONS`. This
  bullet is retained struck-through rather than deleted, because `TCM1.md`'s owned-scope item 1 cites it and
  a reader tracing that citation must land on the correction, not on the original claim.

## TCM2 (Content-mapper projection plane)

- **The acyclic-invariant test is now specified, not just named.** `evidence/TCM0/
  acyclic-invariant-test-spec.md` gives the structural (sealed-context, compile-fail) plus runtime
  (bounded-timeout deadlock control build) shape the discriminating test must take. TCM2's charter
  should reference this spec directly as its acceptance criterion for the invariant, rather than
  restating the invariant prose without a concrete test shape.
- **~~The exact wire method-name spelling is an open verification gap, not settled fact.~~ CLOSED FOR THE
  CAPTURED COMPILE BY LIVE PROBE EVIDENCE.** This bullet originally required TCM2 to close the
  spelling (live protocol trace or `typescript-go` source read) because §3 recorded only structural (Go
  type-name) evidence. `probes/probe7-mapper-wire-capture.mjs` now records every frame against a real
  configured mapper: `initialize` / `openProject` / `transform` / `closeProject`, params shapes, handle
  format, configuration keys and the 5-second `initialize` timeout
  (`package-lock-and-semantic-api.md` §3a). What TCM2 still owes is the narrowed residual only: the
  `transform` RESPONSE body layout. Retained struck-through so a reader tracing the old obligation lands
  on the closure, not the original claim.
- **Supplemental outputs supersede, don't approximate, today's virtual-file-naming convention.** The
  protocol's native `SupplementalOutput` field was purpose-built for exactly this ("multiple TypeScript
  files from a single source", upstream's own Astro example) — TCM2 should route Verter's existing
  `VirtualFileNaming` companion-suffix outputs through this ONE native mechanism rather than inventing a
  parallel supplemental-output convention alongside it (`evidence/TCM0/
  external-source-decision-table.md` row #10).
- **Feature-mask emission must be explicit, never omitted.** `evidence/TCM0/
  projection-class-contract.md` records that an omitted `features` field on a wire `SpanMapSegment`
  silently normalizes to `All` upstream — TCM2's acceptance tests must include a negative check that no
  code path can emit a segment without computing this field.
- **Projection-plane topology selection is TCM2's, by ruling** —
  `docs/arch/refactor/rev11/rulings/ARCHITECT-RULING-2026-08-24-TCM0-DECISIONS.md`
  Q2 ratifies the transfer and makes evidence-based projection-plane selection a **blocking exit** of
  TCM2. The transfer itself needs no further ratification act; what remains is applying it to `TCM2.md`'s
  own numbered lists, which is the program orchestrator's charter-amendment act and not TCM0's. The text
  below is the recommended wording for that act (see `OPEN-GAPS.md` §`G-TOPOLOGY`). Proposed owned-scope item 16: *"**Projection-plane topology
  selection**: select among the surviving projection-plane candidates (native mapper with in-process
  compiler; thin mapper over a shared native daemon) using the measurement contract in
  `evidence/TCM0/topology-benchmark-plan.md`."* Proposed numbered exit criterion 14: *"**Projection-plane
  topology selected on evidence** (owned-scope item 16). Evidence: the current-path baseline captured and
  committed as the block's FIRST act, per `evidence/TCM0/performance-baselines.md` requirements 6-8; the
  complete comparison across the surviving candidates over the benchmark plan's full metric list, every
  timing claim a distribution over N>=10 iterations with raw samples; the non-dominance rule applied as
  written; and, if multiple candidates remain non-dominated, a stated secondary criterion applied and
  recorded, as the benchmark plan's selection rule requires."*

## TCM3 (TypeScript semantic capability closure)

- **A required design constraint, from a reproduced defect.** Never retain a `Program`/`Checker` handle
  past its owning `Snapshot`'s `dispose()` — the exact candidate build silently serves stale cached data
  from such a handle with no error, while the four probed siblings `getSemanticDiagnostics`,
  `getSourceFileNames`, `emitToString`, and `getSyntacticDiagnostics` fail closed
  (`evidence/TCM0/package-lock-and-semantic-api.md` §4c). TCM3's charter should add this as an explicit
  acceptance criterion (a structural/type-state rule if the surrounding language allows it, per this
  program's general preference for structural guards over runtime discipline), not leave it to
  case-by-case caller discipline.
- **The session-attach topology needs its own certification pass — STANDS, and TCM0 has probed it once.**
  TCM0 certified the direct-native-client topology candidate live; it did not, in its first pass, probe
  `API.fromLSPConnection` (`custom/initializeAPISession`) for the session-initialization-hang defect
  class, so TCM3's charter should name this as a required probe before that topology candidate may be
  selected. It has since probed it: `probes/probe8-lsp-session-attach.mjs` drives a real LSP handshake,
  obtains the API pipe via `custom/initializeAPISession`, attaches, and answers a `Checker` query over
  it. **No hang**, and one hard constraint nothing had recorded — the attach topology is
  ASYNC-CLIENT-ONLY (`dist/api/sync/client.js:11` refuses socket connections), plus a bind race requiring
  bounded retry. See `package-lock-and-semantic-api.md` §4a-attach.
  **The refinement is NOT withdrawn, and an earlier revision of this bullet was wrong to withdraw it.**
  `rulings/MAINTAINER-RULING-TCM-PACKAGE-CERTIFICATION-SETTLED.md` clause 2 gates this probe to TCM3 by
  ratified assignment, and one probe run by another block is evidence, not a discharge of that
  assignment — only a fresh ratification act discharges it. TCM3 therefore still owns the probe, and
  inherits TCM0's run and the async-client-only constraint as a head start on it. What a future
  amendment may reasonably do with this evidence is narrow the probe's scope; that is the maintainer's
  act to take, not this document's to assume. (This note previously sat under the cancellation bullet
  below, which it does not supersede — re-anchored 2026-08-24.)
- **No cancellation primitive exists in the candidate API.** TCM3 must design its own in-flight-query
  abandonment strategy (fresh snapshot, not server cancel) rather than assuming a cancel-token pattern is
  available to build on.
- **Semantic-plane topology selection is TCM3's, by ruling** —
  `docs/arch/refactor/rev11/rulings/ARCHITECT-RULING-2026-08-24-TCM0-DECISIONS.md`
  Q2 ratifies the transfer and makes evidence-based semantic-plane selection a **blocking exit** of TCM3.
  The transfer itself needs no further ratification act; what remains is applying it to `TCM3.md`'s own
  numbered lists, which is the program orchestrator's charter-amendment act and not TCM0's. The text below
  is the recommended wording for that act (see `OPEN-GAPS.md` §`G-TOPOLOGY`). Proposed owned-scope item 9: *"**Semantic-plane topology selection**:
  select among the surviving semantic-plane candidates (attach to the editor-owned API session; direct
  native client; managed process for non-editor hosts) using the measurement contract in
  `evidence/TCM0/topology-benchmark-plan.md`."* Proposed numbered exit criterion 10: *"**Semantic-plane
  topology selected on evidence** (owned-scope item 9). Evidence: the current-path baseline captured and
  committed as the block's FIRST act, per `evidence/TCM0/performance-baselines.md` requirements 6-8; the
  complete comparison across the surviving candidates over the benchmark plan's full metric list, every
  timing claim a distribution over N>=10 iterations with raw samples; the non-dominance rule applied as
  written; and, if multiple candidates remain non-dominated, a stated secondary criterion applied and
  recorded, as the benchmark plan's selection rule requires."*

## TCM4 (Atomic activation and deletion)

- **The deletion list is now concrete, not a category description.** `evidence/TCM0/
  deletion-closure.md` names six specific mechanisms/call-path halves to delete and the exact rows of
  `feature-ownership-ledger.md` and `diagnostic-ownership-matrix.md` that justify each. TCM4's charter
  should reference that file directly as its deletion manifest rather than re-deriving the list at
  execution time.
- **Two ledger rows are RETAINED, and their deletion gate is now explicit.**
  `register_carrier_member`/`register_carrier_metadata`/`activate_carrier_member(s)` (ledger rows
  #25-26) are retained under `VerterWithTypeSemanticOracle` per
  `docs/arch/refactor/rev11/rulings/ARCHITECT-RULING-2026-08-24-TCM0-DECISIONS.md`
  Q4 — row 25 preserves local content/position conversion and carrier-to-project routing, row 26 preserves
  oracle working-set activation. TCM4 may remove the tsserver-specific methods **only after TCM3 supplies
  and tests equivalent semantics**; TCM4's charter should gate on that condition explicitly rather than
  treat "TCM0-TCM3 landed" as sufficient authority to delete everything TCM0 discussed.
