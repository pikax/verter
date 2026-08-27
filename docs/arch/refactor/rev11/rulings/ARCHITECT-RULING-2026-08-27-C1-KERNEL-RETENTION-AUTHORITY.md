# Architecture ruling — C1 kernel-retention authority

Status: **RATIFIED — narrow C1 amendment required.**

## Decision

The round-4 finding is valid. The existing five input-resolution meters do not authorize a bound over
workspace-alias geometry or completed output witnesses: `unique_keys` remains exclusively the first-seen
complete `InputKey` identity in the workspace operation ledger. C1 may close the finding only by adding the
two meters below to the same policy carrier and ledger. No candidate-count interpretation is authorized.

Reviewed source: `d4bed980bea382a7d42dae4b932f16ba247065dc`, tree
`b2f56be5201560710df45bca7f2dde3f106cedf3`. This act authorizes a fix; it does not accept or pre-stamp that
subject.

## Exact meters

`InputResolutionBudgets` becomes the sole seven-field semantic-owned carrier. Its five existing fields and
meanings are unchanged. It adds:

| Meter | Type | Inclusive maximum | Charged retained unit |
|---|---:|---:|---|
| Alias geometry retention | `u32` | 1,024 | One simultaneously live executable workspace-alias geometry bundle: project identity, the priority-selected alias target, normalized target, probe profile, ordered probe vector, and candidate-exclusive memo entries |
| Completed witness retention | `u32` | 1,024 | One simultaneously live first-seen entry in the tagged disjoint union `Fact(FactVersionRef) | AmbientDependency(consumer, virtual) | ConsumedResolutionObservation(key)` across the whole composed `AttemptOutput` |

The prospective live value `<= maximum` is admitted; `> maximum` is rejected before retaining the unit.
Checked widened arithmetic is mandatory and overflow is a breach. A duplicate exact completed-witness entry
charges zero. Alias configuration rows, projects visited, candidates examined, and complete `InputKey`s are
not units of either new meter.

The one workspace-owned operation ledger gains the two current-live counters; there is no second table or
per-project/per-frontier allowance. It begins once per independent import-resolution operation, persists
through kernel waves, basis changes, conditional-commit retries, and outer discovery/replay/final-fence
restarts, and resets only for a later independent operation. Destroying or discarding retained state
releases its live charge; resetting a counter is not a release. Alias geometry stays charged while its frame
or memo state stays live. Attempt output stays charged until it is discarded or, after `Complete`, until the
operation publishes or ends.

## Resolver and witness semantics

Lazy priority-preserving alias geometry is mandatory. The established longest-`find`-prefix order and its
lexical tie-break apply before any normalized-target deduplication; any deduplication retains the first item
in that established order. Geometry is generated/evaluated on demand and semantically dead bundles are
released before the next prospective charge. The number of matching aliases, reachable projects, or total
candidates may never be pre-counted into a failure. A long stream that keeps live retention within the
maximum must continue to its real hit or exhausted miss. No silent truncation, skipped candidate, alternate
winner, reordered probe, or changed module-resolution answer is permitted.

Completed witnesses remain exact and ordered. Every composition path, including top-level, source-id,
node-modules, tsconfig-path, and project-reference folds, uses the same whole-output meter. At the maximum,
the exact complete output is returned unchanged. If the next distinct entry would breach the maximum, the
whole attempt is discarded and returns the typed resource failure below; it never returns a sampled witness,
cacheable miss, different hit, or partial `Complete`.

## Failure, precedence, audit, and admission

Add distinct terminal identities:

- `InputResolutionAliasGeometryRetentionLimit { retained, prospective, maximum }`;
- `InputResolutionCompletedWitnessRetentionLimit { retained, prospective, maximum }`.

Both are input-resolution limit failures and map through every facade to
`NonCacheable(BudgetExceeded)`. They publish no positive/negative resolution candidate, exact/parsed edge,
package state, reverse-index entry, fact signature, persistence entry, or partial witness, and receive no
same-operation retry. They are never represented as `InputResolutionUniqueKeyLimit`, an empty unresolved
`InputKey` set, `ResolutionRetryExhausted`, or a semantic miss.

Outer precedence remains unchanged: attempt admission precedes kernel invocation; an allowed invocation's
terminal precedes later no-progress/capability/depth/unique-key/byte checks. Inside the kernel, release dead
state first, then check the prospective charge immediately before retention. The first required prospective
charge in canonical resolver order wins. The two domains do not share a charge action; an already-produced
semantic terminal that needs no further retention wins. Witness entries are charged in their canonical
merge order and any breach discards the entire output.

`InputResolutionBudgetMeter` adds `AliasGeometryRetention` and `CompletedWitnessRetention`. Before terminal
escape, the existing audit path emits exactly one `InputResolutionBudgetExhaustion` with `meter`,
`consumed` (live retained before the rejected action), `prospective`, and `maximum`; the matching terminal
failure carries the same three scalars. The workspace owns emission because it owns the operation ledger.

## Carrier and ingress

`RATIFIED` and `Default` carry the two maxima above. Every override remains one complete immutable
`InputResolutionBudgets` value; each of all seven values must be in `1..=RATIFIED`. Preserve the existing
five-argument tightening constructor as a compatibility ingress that selects the ratified defaults for the
two new fields, and add one named seven-field tightening ingress. The existing filesystem/memory
whole-value override constructors remain the only workspace ingress. No `HostConfig`, FFI, NAPI,
`FilesystemOptions`, `MemoryOptions`, semantic fuse, duplicate constant table, or second policy carrier is
authorized.

## Required discriminators

1. Exact inclusive/max+1 tests for each new meter, including checked-arithmetic overflow, exact terminal and
   audit scalars, `BudgetExceeded`, zero publication, and a fresh independent operation starting clean.
2. A normalization collision in which declaration order presents a less-specific alias first but the
   established more-specific alias still wins; moving deduplication before priority ordering must make it
   red.
3. More total aliases and more reachable projects than the tightened live-geometry maximum, with a late
   winner and live high-water within the maximum, must complete. Pre-count failure, per-project reset,
   eager all-project construction, candidate truncation, or priority change must make it red.
4. A pinned-live geometry max/max+1 fixture must admit the boundary and fail before the extra allocation,
   candidate evaluation, or loader call.
5. Completed misses composed across multiple frontier families and project-reference frames must pass at
   exactly the whole-output maximum and fail at max+1. Leaving any unbounded `AttemptOutput` merge, resetting
   per frontier/frame/basis, or truncating one witness must make the test red.
6. Duplicate entries charge zero; positive and exhausted-miss cases assert the exact ordered witness, and a
   breach cannot select a lower-priority answer or fabricate a cacheable miss.
7. Default, five-field compatibility, seven-field tightening, unique-`InputKey` independence, and the
   existing nominal second-table/ingress discriminator must all remain green.

Each production check requires an apply-proven mutation and clean revert. Extend existing structural or
behavioral ownership tests; do not add a name-keyed source-tree scanner.

## Scope and alternatives

This amendment is C1-only correctness/resource confinement. It authorizes no C2 continuation or TypeInfo
gateway, no C4/end-of-train work, no performance tuning, and no second resolver.

Non-exhaustive alternatives, not selected: (1) a separately evidenced exact retained-byte reservation meter
could later supersede both entry meters, but it is broader and has no current authority; (2) leaving the
structures unbounded is invalid because bounded retention is a retained non-waivable C1 gate. Existing
authority alone is not an option because it defines exactly five different charged domains.

