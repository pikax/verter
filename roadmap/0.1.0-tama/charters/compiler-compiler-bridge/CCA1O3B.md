<!-- unified-charter-v2
id=CCA1O3B
name=WASM transport-probe host-request migration
phase=compiler
train=compiler.compiler-bridge
product=compiler_bridge
kind=migration
semantic_role=delivery
class=compiler
predecessors=CCA1O3,CCA1O1A,CCA1O3C,CCA1O3D
owner=compiler.compiler-bridge:WASM transport-surface probe typed host requests
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
charter=charters/compiler-compiler-bridge/CCA1O3B.md
max_production_loc=250
max_production_files=1
max_related_packages=1
rescope_loc=600
rescope_files=3
rescope_unrelated_packages=2
-->

# CCA1O3B — WASM transport-probe host-request migration

## Independently acceptable outcome and rollback boundary

Move the direct WASM transport-surface probe to CCA1O3's typed request while the binding-local legacy profile remains available. Reverting restores only the probe's request objects; the typed adapter, local compatibility DTO, and fixture producer remain installed.

## Concrete surfaces and APIs

- The sole production/tooling surface is `packages/wasm/scripts/probe-transport-surface.mjs`.
- Owns every profile-bearing `getVirtualFile`, `getIde`, and `ensureIdeCompiled` probe case in that file, including Vue/Svelte success, optional-product, structural-absence, and refusal variants. Each becomes one typed `compileRequest`; the IDE cases stop being an ensure-then-read pair.
- Exported-surface enumeration, missing-versus-refused classification, canonical IDs, output/map/diagnostic normalization, SFC-absolute span meaning, and JavaScript UTF-16 offsets remain unchanged.

## Exact predecessor contract

- **CCA1O3:** implemented ledger row for “WASM typed host-request adapter”.
- **CCA1O1A:** implemented ledger row for “Canonical Svelte custom-element prop-type admission”; the Svelte custom-element prop-type slot has its final shape, so the probe cases encode no superseded closed vocabulary.
- **CCA1O3C:** implemented ledger row for “Execution-proven WASM JS-boundary gate”; the browser boundary refusals this probe classifies are proven by execution rather than by compilation alone.
- **CCA1O3D:** implemented ledger row for “WASM typed host-request callable route”; the browser host object exposes one callable typed compile entry on its generated JavaScript surface, so this consumer has a reachable typed route to move onto.

## Acceptance and evidence

- The probe contains no legacy profile request or positional IDE profile and exercises the typed Vue/Svelte request variants. Every probe axis survives, and each former ensure-then-read IDE case is one typed call; no case gains a WASM call or copies a source into its request.
- Probe output keys, ordering, output/map/refusal classification, canonical IDs, and serialized offsets are equivalent.
- `node --check`, WASM request-conversion fixtures, and the existing native/WASM transport comparison prove shape and behavior.

## Deletions, budgets, and aborts

- Delete no WASM compatibility type, binding decode, probe case, output key, or comparison rail.
- Ceiling: 250 production/tooling LOC, 1 production/tooling file, 1 related package; rescope above 600 LOC, 3 files, 2 unrelated packages, or if fixture generation or playground runtime routing enters.
- Abort on a deleted probe axis, duplicate WASM execution, changed normalization, or transport divergence.

## Verification and review

Run `node --check`, WASM request-conversion tests, the hermetic transport comparison, and `targeted-domain`. Add only CCA1O3B's ledger row and apply `public-3`.

## Recorded deviation (awaiting ratification)

The complete-only typed route this charter mandates admits no `missing`
classification for two of the cases it also directs to preserve, so the
implementation changed `svelteServerStyle` (`missing` → `error`) and
`getIdeWithoutMap` (`missing` → `published`) on the WASM transport only. That
is a deliberate departure from this charter's "missing-versus-refused
classification … remain unchanged" surface line, its "classification …
equivalent" acceptance line, and its "transport divergence" abort clause.

The conflict, why no third answer exists, what the implementation did instead,
the disclosed residual, and the three ratification options are recorded in
`decisions/2026-09-03-complete-only-transport-probe-classification-deviation.md`.
Nothing in this charter is amended by that record until a maintainer rules.
