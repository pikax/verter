# TCM0 — Current TypeScript contract and dual-plane architecture lock

Read-only investigation and architecture lock, per `charters/TCM0.md` and
`amendments/DISC-2026-08-22-TYPESCRIPT-CONTENT-MAPPERS-amendment.md`. TCM0 changes no production route,
ships no code, certifies no package by assumption, and deletes nothing — every finding below is against
bytes actually inspected or a probe actually run, not against documentation alone, per the charter's own
warning that a published package does not necessarily contain every repository-main change.

**This is a decision-record block. It changes no file under `crates/`, `packages/`, or `scripts/`.**
Every architectural decision it locks is executed by a later, separately-authorized block (TCM1-TCM4,
each requiring its own digest-bound authority-registry record before dispatch).

## The evidence

| artifact | what it resolves |
|---|---|
| [`decisions/ADR-021-typescript-content-mapper-dual-plane.md`](../decisions/ADR-021-typescript-content-mapper-dual-plane.md) | the ratified architecture decision this investigation locks |
| [`TCM0/package-lock-and-semantic-api.md`](TCM0/package-lock-and-semantic-api.md) | charter items 1-2 — exact candidate package identity/provenance, the content-mapper protocol confirmed present in the actual downloaded bytes, and a live-probed semantic-API certification including one genuinely reproduced defect |
| [`TCM0/mapping-products-string-surface.md`](TCM0/mapping-products-string-surface.md) | the full extent of the string-encoded projection surface `source_projection_map()` represents, a correction to the amendment's own citation, and the acceptance bar this hands to TCM1 |
| [`TCM0/feature-ownership-ledger.md`](TCM0/feature-ownership-ledger.md) | charter item 3 — all 44 `TypeProvider` methods/capabilities (31 ledger rows; 8 priority-tier variants folded into their base method's row), one owner each from the four legal owners, zero left unclassified, two rows explicitly named governance-pending rather than silently defaulted |
| [`TCM0/diagnostic-ownership-matrix.md`](TCM0/diagnostic-ownership-matrix.md) | charter item 4 — attribution/suppression/precedence/dedup for every diagnostic class, including one required correction to current behavior (generated-region diagnostics must surface with honest attribution, not be silently dropped as today's code does) |
| [`TCM0/projection-class-contract.md`](TCM0/projection-class-contract.md) | charter item 5 — the minimal class set and the terminal mask-derivation policy, built directly on the upstream `SpanMapFeature`/`SpanMapKind`/`SpanMapFidelity` wire primitives confirmed present in the candidate |
| [`TCM0/external-source-decision-table.md`](TCM0/external-source-decision-table.md) | charter item 6 — one model each (TS-owned / content-mapped / Verter-owned / unsupported) for all 11 named external-source shapes |
| [`TCM0/topology-benchmark-plan.md`](TCM0/topology-benchmark-plan.md) | charter item 7 — the plan and harness for both planes' topology candidates; explicitly no comparative numbers produced yet |
| [`TCM0/cache-lifecycle-contracts.md`](TCM0/cache-lifecycle-contracts.md) | charter item 8 — one cache/invalidation law per concern, built on the candidate's own confirmed ref-counted snapshot cache rather than a second parallel scheme |
| [`TCM0/deletion-closure.md`](TCM0/deletion-closure.md) | charter item 9 — six concrete deleted mechanisms, their survivors with proven owners, and the two rows explicitly withheld pending governance ruling |
| [`TCM0/performance-baselines.md`](TCM0/performance-baselines.md) | charter item 10 — thresholds locked from evidence gathered in this investigation, explicitly excluding any number an implementation this program hasn't built could only supply |
| [`TCM0/acyclic-invariant-test-spec.md`](TCM0/acyclic-invariant-test-spec.md) | the discriminating deadlock/reentrancy test specification this charter requires TCM0 to write and TCM2 to implement |
| [`TCM0/tcm1-tcm4-charter-refinements.md`](TCM0/tcm1-tcm4-charter-refinements.md) | recorded refinements for a future amendment to apply to the still-LOCKED TCM1-TCM4 charters — those charter files are not edited by this block |

## What the investigation found that the amendment/discovery text got wrong

The ratified amendment and discovery documents both cite `checker.rs:411` as calling
`PositionMapper::from_json(… .unwrap_or(""))`. **Verified false as written**: that exact call exists only
in a test file (`kebab_tag_mapping_full_columns.rs:65`); `checker.rs:411` instead base64-encodes the
string directly into an inline `sourceMappingURL` comment for `tsc`/`tsgo` to parse independently. The
amendment's underlying thesis — the surface is string-encoded and must become typed — is unaffected;
only the specific citation needed correcting, and the true extent of the string-encoded surface turned
out to be considerably WIDER than the two cited lines (at least nine struct fields in `verter_compiler`
plus four in `verter_protocol`'s FFI wire types — `TCM0/mapping-products-string-surface.md`).

## What the investigation found that no document had stated at all

- **The npm `typescript` package no longer contains a compiler.** `typescript@7.x` ships only a thin
  JS/TS API client; the actual compiler/checker/language service is a separate native Go binary resolved
  through `optionalDependencies`. Every topology candidate in charter item 7 spawns or attaches to the
  SAME native binary — the "light JS engine vs heavy native engine" choice this investigation might have
  expected to evaluate does not exist.
- **The content-mapper protocol genuinely exists in the exact candidate bytes**, not merely in the
  upstream PR text — confirmed by disassembling the downloaded native binary and finding its
  `internal/contentmapper` Go symbol table (`OpenProjectParams`, `TransformParams`, `CloseProjectParams`,
  `InitializeResult`, `handshake`, etc.), matching the four-step `Initialize`→`OpenProject`→`Transform`→
  `CloseProject` lifecycle the upstream design describes. The exact literal wire method-name spelling
  could not be isolated from a stripped binary via static `strings` extraction — recorded as an open gap
  for TCM2, not glossed over.
- **A genuine, reproduced defect, live-probed against the exact candidate**: a `Program` handle obtained
  from a `Snapshot` continues to silently serve cached, stale content after that `Snapshot` is disposed —
  with zero error and zero server round-trip — while every sibling `Program` method fails closed
  correctly (`"snapshot N not found"`) in the identical post-dispose state. Root cause located in the
  shipped client source (`SourceFileCache`'s ref-counting deliberately skips release for a still-latest
  disposed snapshot). This becomes a required TCM3 design constraint, not an open question.
- **The session-attach topology candidate (`API.fromLSPConnection`) was NOT reproduced for a hang** —
  recorded honestly as an untested gap rather than either a false certification or a false alarm.

## Decisions requiring maintainer ratification

None of TCM0's own findings require ratification to RECORD (that is exactly what this investigation is
for), but two items in the feature-ownership ledger are explicitly NOT TCM0's to decide:

| id | decision | artifact |
|---|---|---|
| TCM0-G1 | Whether `register_carrier_member`/`register_carrier_metadata`/`activate_carrier_member(s)` are deleted (their function may be fully subsumed by the content mapper's own identity fields) or retained | `feature-ownership-ledger.md` rows #25-26, `deletion-closure.md` |

## Claims this block does not make

- **No claim that the exact JSON-RPC method-name spelling for the content-mapper protocol matches the
  literal strings `initialize`/`openProject`/`transform`/`closeProject`.** The structural (Go type-name)
  evidence strongly corroborates the four-step lifecycle; a byte-exact wire trace was not produced.
- **No claim that the `API.fromLSPConnection` session-attach path is free of the "API-session hang"
  defect class the charter names.** Only the direct-spawn path was live-probed; the attach path is an
  open verification gap, explicitly not inherited certification.
- **No claim that the reproduced stale-`Program` defect (§4c of `package-lock-and-semantic-api.md`) is
  THE SAME pre-documented defect the charter presupposes exists.** No canonical upstream issue matching
  that exact description was located during this investigation (a WebSearch pass found related but
  non-identical issues). What is claimed is narrower and fully evidenced: this specific behavior was
  reproduced, its root cause located in source, and its consequence recorded as a design constraint —
  independent of whether it is the same defect some other, unlocated report already describes.
- **No claim that the feature-ownership ledger's owner assignments are the ONLY architecturally valid
  split.** Several rows split across two owners with a stated discriminant (e.g. plain-TS vs.
  Verter-authored code actions); a later block may find a different split correct, but every row is
  classified — none is left as "TBD," satisfying the acceptance bar without claiming finality on judgment
  calls that are legitimately TCM1-TCM3's to refine.
- **No claim of comparative topology performance.** `topology-benchmark-plan.md` is a plan; the only
  numbers in this investigation are single-topology reference points, explicitly marked as such.

## Verification

Per the charter, TCM0 runs no gate, no cargo test suite, no build slot — this is an investigation block.
Every factual claim is re-derivable from the commands recorded beside it in
`TCM0/package-lock-and-semantic-api.md`'s Reproduction section and the individual `grep`/`Read` citations
threaded through each evidence file. The load-bearing ones:

| claim | command/method |
|---|---|
| candidate package identity, provenance, dist-tag | `curl https://registry.npmjs.org/typescript/7.1.0-dev.20260822.1`; `curl https://registry.npmjs.org/typescript` for the full dist-tags table |
| tarball integrity | `shasum -a 1`/`shasum -a 256` on the downloaded tarball, compared against the registry's `dist.shasum`/`dist.integrity` |
| content-mapper protocol presence in the exact candidate | `strings -a` on the downloaded `@typescript/typescript-darwin-arm64@7.1.0-dev.20260822.1` native binary, grepped for `internal/contentmapper.*` |
| `checker.rs:411` citation correction | `grep -n "PositionMapper" crates/verter_tsc/src/checker.rs` (no hits) vs. direct `Read` of the actual base64/`sourceMappingURL` code at that line |
| stale-`Program`-after-dispose defect | four Node probe scripts run live against `npm install typescript@7.1.0-dev.20260822.1` in a scratch directory on this host, root-caused against the shipped `dist/api/sync/api.js` source |
| `TypeProvider` trait exhaustiveness (44 methods, 31 ledger rows) | `crates/verter_type_runtime/src/traits.rs:130-512`, cross-checked by grep for every method name across `crates/*/src` excluding test files; independently re-verified by a review pass that direct-enumerated every `fn` in the trait body |
| diagnostic/external-source current-state claims | file:line citations embedded directly in `diagnostic-ownership-matrix.md` and `external-source-decision-table.md`, independently re-`grep`/`Read`-able |
| authority-registry digest binding | `python3 -c "import hashlib; print(hashlib.sha256(open('charters/TCM0.md','rb').read()).hexdigest())"` matched byte-for-byte against `authority-registry.toml`'s recorded `sha256` for `TCM0-CHARTER` and the amendment document, before any work began |

Guards whose scan surface includes this content: `tracked_paths_are_portable` (path-shape enforcement,
generic); `every_critical_rule_in_docs_has_registered_guard` reads `CLAUDE.md`/`.claude/skills/*/
SKILL.md` only, so it does not see this directory — named because it is the guard a reader would assume
covers it, per the same honest-accounting convention A5 established.
`no_phase_archaeology_in_production_code` scans `crates/*/src/**` only and does not see this directory
either; TCM0 touches no production source, so the program-vocabulary prohibition on source is honoured
by not applying here at all.
