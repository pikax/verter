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
- **Scope the surface as wider than the two cited lines.** At least nine distinct `Option<String>`/
  `String` fields across `verter_compiler` plus four across `verter_protocol`'s FFI wire types carry the
  same string-encoded convention (full list in `mapping-products-string-surface.md`). TCM1's acceptance
  bar should include the FFI wire types, not stop at the in-process boundary — otherwise TCM1 leaves a
  second string-encoded path alive at the NAPI/WASM boundary, in tension with the "one clean cutover"
  rule.
- **Single point of origin.** TCM1 should replace the discard-to-string pattern at `CodeTransform`'s own
  `generate_map`/`generate_map_json*` (`code_transform/source_map.rs`), not at each downstream consumer
  site — the typed intermediate (`oxc_sourcemap::SourceMap<'static>`) already exists transiently at
  exactly that point and is thrown away by every current caller.

## TCM2 (Content-mapper projection plane)

- **The acyclic-invariant test is now specified, not just named.** `evidence/TCM0/
  acyclic-invariant-test-spec.md` gives the structural (sealed-context, compile-fail) plus runtime
  (bounded-timeout deadlock control build) shape the discriminating test must take. TCM2's charter
  should reference this spec directly as its acceptance criterion for the invariant, rather than
  restating the invariant prose without a concrete test shape.
- **The exact wire method-name spelling is an open verification gap, not settled fact.** TCM2 must close
  it (live protocol trace or `typescript-go` source read) before its own implementation can claim
  fidelity to the upstream protocol — `evidence/TCM0/package-lock-and-semantic-api.md` §3 records
  strong structural (Go type-name) evidence for the four-step lifecycle but not a byte-exact trace.
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

## TCM3 (TypeScript semantic capability closure)

- **A required design constraint, from a reproduced defect.** Never retain a `Program`/`Checker` handle
  past its owning `Snapshot`'s `dispose()` — the exact candidate build silently serves stale cached data
  from such a handle with no error, while every sibling method fails closed
  (`evidence/TCM0/package-lock-and-semantic-api.md` §4c). TCM3's charter should add this as an explicit
  acceptance criterion (a structural/type-state rule if the surrounding language allows it, per this
  program's general preference for structural guards over runtime discipline), not leave it to
  case-by-case caller discipline.
- **The session-attach topology needs its own certification pass.** TCM0 certified the direct-native-
  client topology candidate live; it explicitly did NOT probe `API.fromLSPConnection`
  (`custom/initializeAPISession`) for the session-initialization-hang defect class. TCM3's charter should
  name this as a required probe before that topology candidate may be selected, not assume it inherits
  TCM0's certification by association.
- **No cancellation primitive exists in the candidate API.** TCM3 must design its own in-flight-query
  abandonment strategy (fresh snapshot, not server cancel) rather than assuming a cancel-token pattern is
  available to build on.

## TCM4 (Atomic activation and deletion)

- **The deletion list is now concrete, not a category description.** `evidence/TCM0/
  deletion-closure.md` names six specific mechanisms/call-path halves to delete and the exact rows of
  `feature-ownership-ledger.md` and `diagnostic-ownership-matrix.md` that justify each. TCM4's charter
  should reference that file directly as its deletion manifest rather than re-deriving the list at
  execution time.
- **Two ledger rows are explicitly NOT ready for deletion.** `register_carrier_member`/
  `register_carrier_metadata`/`activate_carrier_member(s)` (ledger rows #25-26) require a maintainer
  ruling before TCM4 may delete them — TCM4's charter should gate on that ruling explicitly rather than
  treat "TCM0-TCM3 landed" as sufficient authority to delete everything TCM0 discussed.
