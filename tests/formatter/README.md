# Formatter contract artifacts

The [native formatter architecture](../../docs/arch/kernel/formatter.md) owns
composition and private/public boundaries. This directory supplies its concrete
compatibility, geometry, fixture and measurement lock. It implements no formatter,
changes no route or capability, and deletes no production code. Implementation
state stays in Tama; there is no repository ledger or roadmap validator.

## Compatibility

[`formatter-compatibility-v1.toml`](../../catalogs/formatter-compatibility-v1.toml)
pins Prettier **2.8.8**, already present in the frozen dependency graph. There is
no new dependency, plugin, network corpus or production oracle. The captured
support metadata includes every option of that release, the deprecated
`jsxBracketSameLine` alias, and explicit rejection of the four named options that
are absent from that release. `rangeEnd` uses `"source-end"` for positive infinity,
resolved against the exact authored input; it is not a numeric wire default.
Configuration/ignore/override capture remains with the configuration owner.

The declared finite dimensions are ten languages, thirteen option sets, three
recovery modes and three private request modes. Each combination has one cell;
each cell names a source, normalized options and exact output or typed refusal.
Family sets select only applicable options and store that selection explicitly
in the expectation. The explicit inapplicable set instead requires a diagnostic.
This enumerates admitted configurations, not all combinations of integer widths.
All known options have explicit type, applicability, accepted values/defaults and
unsupported policy; unknown names never acquire defaults. Empty plugin arrays do
not authorize discovery or execution.

Valid `prettier-exact` output was captured from the pinned local package, without
normalizing bytes. The Svelte cells are `verter-default`: that Prettier release
has no built-in Svelte parser. The admitted simple Svelte carrier layout is fixed
as native output; its script/style and markup layout follows the checked-in bytes
and makes no plugin parity claim. The other default divergence retains an entire
explicit recovery island byte for byte. Unsafe malformed cells refuse before
printer work. Both divergences are named in
[`divergences.json`](../../fixtures/formatter/options/divergences.json).

[`laws.json`](../../fixtures/formatter/laws.json) fixes renderer width/newlines,
provenance-anchored edit minimality, map boundary bias and absence/ambiguity,
range expansion, cursor affinity, BOM, Unicode, CRLF and EOF behavior. Inputs use
private authored UTF-8 coordinates; outputs use distinct formatted UTF-8
coordinates. No implicit conversion or generated-TSX coordinate reuse is admitted.
Every complete cell requires byte-identical idempotence and zero second-pass
edits; permutations and cold/incremental execution must preserve ordered output,
edits and map answers. These are successor behavior obligations, not a claim
that the current whitespace formatter passes them.

Extension files `catalogs/formatter-compatibility-<lang>-v1.toml` declare the same
release and dimensions for exactly their own language. They carry the complete
option vocabulary and their own corpus/cell/asset rows, rooted under
`fixtures/formatter/corpora/<lang>/`. Validation discovers them automatically and
rejects missing cells, duplicate cells, unknown language ownership and mismatches.
No network oracle is consulted during validation.

## Inventory and ownership

[`inventory.json`](inventory.json) binds the seven current source symbols to the
reviewed architecture inventory. It assigns route mutation and whitespace-body
deletion exclusively to the cutover owner, records every private contribution's
empty deletion population, and retains tag auto-close outside formatter ownership.
Protocol, LSP, NAPI, WASM and MCP capability cells are separate: LSP has no range
or cursor result, while protocol/MCP have no cursor result. Private support cannot
advertise those capabilities. New routes/prototypes require an authority amendment
with exact paths/symbols and one owner before mutation.

## Performance and lifecycle

[`formatter-performance-v1.toml`](../../catalogs/formatter-performance-v1.toml)
contains the calibration operations, seed, constants and sample order; counter
units and structural work bounds; every numeric mode's advisory thresholds;
absolute result/cache limits; hermetic source identities and prepared record
inventories; and all 10,000 contiguous churn actions. Its source records partition
the complete authored byte extent. A descriptor's digest identifies the descriptor
file; its authored size/blocks/records describe the referenced source population,
not the JSON descriptor's serialized byte length.

The six primary populations are 4 KiB/3-block small, 100 KiB/10-block/10,000-record
medium, 1 MiB/40-block large, incremental, 1,000 range/cursor/map queries, and
128 distinct medium project sources. Long churn uses those exact sources and
payloads. Revisions, profile/config/policy identities, edits/reverts, cancellation
observation and expected admission are explicit per action. Cancellation publishes a fresh source revision to force a render miss before its observation hook. An edit-revert pair links its two ordinals and gives each request one incremental allowance; the pair shares the stated combined ceiling. Runtime generation
from a seed is forbidden. The range/cursor population also names inserted,
deleted, replaced and retained boundary controls. Open/close, provider transitions,
and disabled/unsupported/unopened/masked requests have the declared zero-work
pipeline obligations, without suppressing the allowed envelope/cache bookkeeping.

The standing measurement rule governs the numeric tables: compare answer classes
and work counts or growth against a baseline on the same worker/session when
comparison is needed. Timing, allocation and memory measurements are advisory;
missing controls are unavailable, never a landing failure. Absolute benchmarks
belong to the separate performance methodology. Normalization retains raw values,
calibration, CPU/build identity and catalog digest; it does not turn a measurement
into a portable acceptance gate.

## Validation and evidence registration

```sh
node tests/formatter/verify.mjs
node --test scripts/benchmark/evidence-run.test.mjs
node scripts/benchmark/evidence-run.mjs --run formatter-performance --candidate <review-candidate> --corpus medium --dry-run
```

These tests run in the existing `test:scripts`/`test:scripts:ci` lane. Its impact
filter covers the catalogs, fixtures and validators; no CI lane or xtask is added.
The existing evidence-run tests validate every corpus parameter and reject omitted
candidate/unknown corpus, missing/duplicate cells and route owners, corrupt hashes,
omitted numeric modes/calibration, and missing/noncontiguous churn actions.
Fixture paths are repository-confined, including realpath checks against symlink
escapes. The restricted TOML reader rejects unknown syntax and duplicate keys.

The parameterized manifest records every latency and peak-RSS cell. A dry run
records the candidate, corpus, fixture/catalog digests and unmeasured cells. This
contract-only checkout has no native formatter benchmark; a non-dry invocation
therefore records `unavailable`, never a fabricated passing measurement. Consumers
supply executable native formatting evidence as their implementations land. No
production implementation is supplied by this registration.

The checks validate authority completeness and consistency; printer semantics,
actual visit/copy counts, cache admission and runtime geometry remain the receiving
implementation owners' tests. Output text and policy changes require reratifying
the lock, rather than rewriting goldens after a mismatch.

## Added mechanism justification

| Mechanism | concern | prevents | notCoveredBecause |
| --- | --- | --- | --- |
| Catalog/asset validation and finite dimension declarations | correctness | Missing or duplicated compatibility outcomes, corrupt fixtures or incomplete calibration/churn can silently change the successor contract during manual edits. | Existing formatter tests cover only whitespace gaps and have no catalog/fixture completeness checks. |
| Churn identity and pair-budget validation | correctness | A cancelled request can otherwise hit a just-warmed identity without observing rendering, or a revert can incorrectly receive the whole pair allowance in addition to the edit allowance; both were observed during lock validation. | Asset hashes and field-presence checks do not prove cache-miss preconditions or allocation-budget composition, and no native formatter exists yet to exercise them. |
| Route inventory join | correctness | A missing or conflicting sole cutover owner can leave a live route on the displaced formatter. | Current route behavior tests do not express future deletion/mutation ownership. |
| Parameterized evidence registration | correctness | Reusing a different candidate/corpus's performance claim is plausible when multiple consumers run the same named manifest. | The semantic/soak runner has neither formatter corpus identities nor required candidate parameters. |
| Fixture-local Git attributes and formatter exclusion | correctness | Repository LF normalization would corrupt intentional CRLF bytes and their content digests on staging/checkout; automatic formatting would rewrite captured inputs and goldens. | Hash validation detects the damage afterward; the repository-wide LF and formatting policies otherwise rewrite these bytes. |
| Existing script-lane impact binding | process | A catalog-only edit could bypass the validator lane. | Script path ownership alone does not cover root catalogs/fixtures. |

All of these mechanisms implement the accepted lock obligations. They add no new
product requirement, runtime safeguard, parser, formatter abstraction or performance
gate. There are no legacy deletions or unsettled design choices in this patch.
