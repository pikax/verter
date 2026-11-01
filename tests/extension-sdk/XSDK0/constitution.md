# Extension authority and capability constitution

This is a contract, not an SDK availability claim. It describes the candidate
whose landing title is “feat(session): reserve module homes for the lint, css,
a11y, formatter, (#1059)”, dated 2026-11-01. The controller records WDX0, CAT0,
PUB0, VID0 and BR0 as DONE; readiness is supplied by that ledger, not inferred
from files. XSDK1–XSDK11 implement and qualify this contract.

The cause of this decision is CAT0's reserved external-contribution table T09:
it has contribution kinds but no executable admission or host. The owner is
the extension boundary in `verter_session::extensions`, with isolated hosts as
consumers. The maintainable fix is a validated contribution into the existing
catalog and demand pipeline, never an extension-owned semantic pipeline.
There is no unresolved authority choice. Concrete host engines/formats and
release tuples remain producer decisions requiring real qualification.

## Imported authority

The normative imports are [CAT0](../../kernel/CAT0/products/catalog-inventory.v1.json)
(T01–T09, XK01–XK09, CR01–CR14),
[VID0](../../kernel/VID0/products/identity-inventory.v1.json),
[PUB0](../../../docs/arch/kernel/public-envelope.md) and
[WDX0](../../web-product/WDX0/products/evidence-class-policy.v1.json).
Their identities, selection, query scheduling, validity, authored maps,
publication and typed outcomes remain core-owned. First-party static row
providers retain `official-static` provenance and require no SDK dependency.

The [inventory](products/extension-inventory.v1.json) assigns every current
route and future consumer a surviving owner. Its capability matrix is a
required target population, with zero currently qualified cells. Existing
editor extensions, bundler plugins and TypeScript providers are host adapters,
not SDK guests; this decision does not reroute them. Deletion population is
empty: the boundary is additive and its module homes contain placeholders.
There is no current dynamic plugin implementation to retire. XSDK1 reconciles
this inventory on its implementation candidate; subsequent producers migrate
any route they actually displace, including docs and examples.

## Contribution modes

The matrix crosses all nine CAT0 kinds with three contribution modes and both
host classes. A declarative contribution supplies descriptors referencing
already supported adapters. A data-only contribution supplies schema-validated
rows/facts with no executable callbacks. An isolated executable contribution
computes those same typed payloads inside a qualified guest. A descriptor or
fact cannot introduce an unsupported parser, backend or semantic interpretation:
that target remains unsupported until its core owner implements the adapter.
No mode supplies native function pointers or registers a replacement core
service. Native and browser cells qualify independently; data support never
implies executable support. XSDK1 admits descriptors for all kinds, XSDK2 owns
fact ingress, XSDK6 specializes HTML/CSS/component data, and XSDK8 mediates rule,
action and tool operations. The matrix's required-unimplemented label binds
producer delivery; it does not weaken the required outcome into optional work.

## Admission and compatibility

**E01 (XSDK1).** One manifest declares one namespace, exact contribution version,
manifest schema version, SDK protocol version, provenance, content digest,
source/license, configuration schema and digest, contribution kinds/targets,
required and optional capabilities, execution mode and requested permissions.
Each profile target references the canonical typed identities and exactly one
release. Carrier targets contain no framework release. Supported alternatives
are separate exact entries, not ranges, floating tags, implied majors or a
wildcard. These are contract fields, not a claim that a wire schema exists.

**E02 (XSDK1, XSDK7).** Compatibility is an exact lookup on the host's published
schema/protocol/build and guest-format/engine tuple, then intersection of
requested capabilities, host-supported cells and explicit grants. Required
capability failure rejects admission; optional failure is reported as disabled
and grants nothing. Unknown required fields, kinds, schemas, versions or target
ids reject with a typed reason. No downgrade, implicit fetch or native library
loading is a fallback. Inspection of static data never starts a guest.

**E03 (XSDK1).** Validate all candidates before building a new immutable catalog.
T09 keys are (kind, namespace, exact version); duplicate keys reject naming both
owners, including equal-byte duplicates. Distinct versions may coexist only
with explicit selection and non-conflicting target claims; no highest-version
or load-order preference. CAT0 CR07 rejects duplicate target keys, editor ids
and equal-precedence suffix claims even across namespaces. Shared-carrier
claims require T06 relations (XK09); missing relations yield typed ambiguity,
duplicate relations reject. Canonical typed-key order determines serialization;
permuting discovery, validation or reply order never chooses a winner. Install,
upgrade, rollback or removal builds a new snapshot; no mutation of a live one.

## Contributions and non-replaceable authority

**E04 (XSDK2, XSDK6).** Declarative rows and data-only HTML/CSS/component metadata
are validated facts, with declared schema, canonical target, source spans/map
basis and provenance. Executable guests return the same typed fact payloads.
The owner validates shape, references, bounds and target before ingress and
merges through existing catalog/index/completion owners. A new carrier or
profile may supply bounded adapter facts through declared interfaces, but may
not replace a registered parser, recovery policy or TypeScript semantics.
Metadata text cannot masquerade as a type-engine answer. Unsupported semantic
requirements stay unsupported; there is no private resolver or completion engine.

**E05 (XSDK3, XSDK4).** Executable contributions require explicit trust and a
qualified guest format/engine with mediated inputs, memory/work/fuel/deadline
limits, cancellation and teardown. No implicit filesystem, network, process,
workspace or browser-origin authority passes into a guest. Declared scoped
inputs are supplied through the host's acquisition boundary. A subprocess or
browser worker alone proves neither sandboxing nor permission isolation. Host
qualification names exact platform, engine, format and tested permission cells;
an unavailable isolation facility disables that executable cell rather than
running untrusted code in-process. Data-only contributions remain usable.

**E06 (XSDK5).** Only selected demand executes. The core supplies source/config/
build/session basis and bounded inputs, owns scheduling and cancellation, and
validates the reply against the live basis before atomic publication. Reuse
observes contribution bytes/version, relevant configuration, catalog/profile
and source/map dependencies through canonical validity facts; no plugin cache
key substitutes for them. Execution policy remains waiter-local, not semantic
identity. Cancelled, stale, malformed or partial replies cannot become complete
or warm a complete cache. Unselected contributions do zero execution work.
Teardown releases guest resources and retained state on every exit path.

**E07 (XSDK8).** Tool operations read through existing typed queries. Action and
migration output is only a proposed authored edit plan: core map reversibility,
source revision preconditions, conflict checks and explicit execution grants
are mandatory before the canonical transaction owner applies anything. An
extension cannot write raw workspace bytes or approve its own plan. Where a
reversible authored map is absent, the operation is refused, not approximated.

## Delivery and proof

**E08 (XSDK7, XSDK9–XSDK11).** Local packaging exposes reproducible content,
compatibility and permissions before installation, with rollback/removal and
no implicit remote fetch. Public examples cover data, embedded-language and
action contributions on native and browser hosts. XSDK9 supplies DOC1-tested
`examples/reference` and exact source/config/build/session provenance; XSDK10
qualifies isolation, conflicts, lifecycle and compatibility with real hosts;
XSDK11 publishes completed VIM/DX cells and permission/uncertainty/migration
notes only after those artifacts pass. An upstream engine's capability, this
contract or an SDK package version is not evidence of Verter support.

The [case table](products/extension-cases.v1.json) defines positive controls and
discriminating negatives with producer ownership; these are unexecuted future
runtime obligations, not tests passed here. AC1 is the inventory plus E01–E08.
AC2 is E04/E07 and cases N01–N03. AC3 changes no state/query/map boundary here:
fresh/incremental, edit/revert, cancellation, stale/partial rejection and order
perturbations belong to XSDK5/XSDK10. AC4 is E08. AC5 adds no numeric budget;
producers use the standing measurement rule: equivalent answers and paired
same-worker work/growth comparisons; timings/memory are advisory, unavailable
controls are recorded, and no stored receipt is a performance gate.

## Condition justifications

These justify the contract conditions, not additional gates. Existing CI tests
current core behavior; it does not yet exercise SDK guest ingress or isolation.

| Conditions | concern | prevents | notCoveredBecause |
| --- | --- | --- | --- |
| E01–E03 | correctness | Plausible admission of an incompatible release or order-dependent owner when multiple packages claim a target | Core catalog rules do not yet have an external manifest producer; XSDK1 must apply them at that boundary |
| E04 | correctness | Malformed guest metadata becoming authoritative semantic facts on ingress | Existing first-party typed producers do not validate untrusted guest payloads |
| E05 | security | Executable guests inheriting ambient workspace or network access if run as ordinary worker code | Core queries and browser worker lifecycle do not establish a guest security boundary |
| E06 | correctness | Delayed guest replies warming results after edit, upgrade or cancellation | Existing cache validity cannot cover an unrecorded external contribution dependency |
| E07 | correctness | A guest edit applying against stale or non-reversible generated coordinates | Core transactions protect edits only when all guest proposals enter that owner |
| E08 | process | Publishing supported cells from a manifest or upstream capability without authored integration proof | Generic CI success does not qualify each host/profile/permission tuple or run missing reference examples |
