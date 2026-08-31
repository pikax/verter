<!-- unified-charter-v2
id=CCA1O2A
name=Native benchmark host-request migration
phase=compiler
train=compiler.compiler-bridge
product=compiler_bridge
kind=migration
semantic_role=delivery
class=compiler
predecessors=CCA1O2,CCA1O2H,CCA1O2I
owner=compiler.compiler-bridge:native benchmark typed host request population
conflict_domains=compiler_execution,host_service_graph,public_protocol
resource_class=ts-heavy
review_profile=public-3
gate_profile=targeted-domain
implementation_effort_min=high
implementation_effort_default=high
review_effort_min=high
review_effort_default=high
verification_effort_min=high
verification_effort_default=high
confirmation_effort_min=high
confirmation_effort_default=high
size=S
dispatchable=true
optional=false
release_gating=none
external_requirements=
charter=charters/compiler-compiler-bridge/CCA1O2A.md
max_production_loc=350
max_production_files=4
max_related_packages=1
rescope_loc=700
rescope_files=6
rescope_unrelated_packages=2
-->

# CCA1O2A — Native benchmark host-request migration

## Independently acceptable outcome and rollback boundary

Move the complete native benchmark population that supplies a legacy compile profile to CCA1O2's typed NAPI request while both public request forms remain available. Reverting changes only benchmark request construction; the typed adapter and every binding route remain installed.

## Concrete surfaces and APIs

- Production/tooling surfaces are exactly `packages/benchmark/src/compilers/verter.ts`, `packages/benchmark/src/perf/axis-a-child.ts`, `packages/benchmark/src/svelte-perf-fence.ts`, and `packages/benchmark/src/verter-mt-worker.mjs`; focused evidence may update `packages/benchmark/src/perf/axis-a-child.spec.ts`.
- Owns the explicit-profile `upsert`, `getVirtualFile`, and positional `getIde` calls in those four files. Profile-free benchmark calls and benchmark algorithms are excluded.
- Each migrated call constructs one framework-discriminated request with the same products and options. Timed regions, warmup, process isolation, content hashing, and measured call count remain unchanged.
- Canonical IDs, output bytes/maps/diagnostics, and serialized span/offset coordinate semantics remain unchanged.

## Exact predecessor contract

- **CCA1O2:** implemented ledger row for “NAPI typed host-request adapter”.
- **CCA1O2H:** implemented ledger row for “NAPI own-property closedness repair”; the native decode refuses an own unknown or cross-framework key whatever its value, so the typed route this caller moves onto is closed as declared.
- **CCA1O2I:** implemented ledger row for “Generated native host-request TypeScript mirror”; the request declarations this caller is written against are generated from the Rust schema and byte-pinned, so they cannot drift from the decoder.

## Acceptance and evidence

- The four named production files contain no legacy `compileProfile` field or positional legacy IDE profile; every former profile-bearing call uses the typed request.
- Benchmark fixtures preserve framework, product, SSR/client, source-map, target, and refusal intent without an extra native call or source copy.
- The existing axis-A unit boundary and package type checks prove request shape and missing-carrier behavior; performance comparison semantics are byte-for-byte unchanged.

## Deletions, budgets, and aborts

- Delete no binding type, converter, public signature, benchmark, or measurement rail.
- Ceiling: 350 production/tooling LOC, 4 production/tooling files, 1 related package; one focused existing test file may change without enlarging the production-file budget.
- Rescope above 700 LOC, 6 production/tooling files, 2 unrelated packages, or if a non-benchmark consumer enters.
- Abort on timing-boundary movement, an added compile call, changed benchmark inputs, or output/map/diagnostic divergence.

## Verification and review

Use TDD only if an existing benchmark request-shape boundary does not discriminate the migration. Run benchmark package type/unit tests and `targeted-domain`; add only CCA1O2A's ledger row. Apply `public-3`.
