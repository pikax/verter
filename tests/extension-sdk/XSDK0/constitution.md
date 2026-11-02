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
[PUB0](../../../docs/arch/kernel/public-envelope.md),
[WDX0](../../web-product/WDX0/products/evidence-class-policy.v1.json) and
LRA0's [rule and action contract](../../../docs/arch/kernel/rules-actions.md).
Their identities, selection, query scheduling, validity, authored maps,
publication, rule execution and typed outcomes remain core-owned. First-party
static row providers retain `official-static` provenance and require no SDK
dependency.

The [inventory](products/extension-inventory.v1.json) separates the SDK's own
boundary modules, the host routes it consumes, the canonical target-table
owners that receive validated rows, and the core authorities no contribution
reaches. A contribution never transfers row ownership (VIM0 VM26): T01–T03
stay CPF1's, T04 PPR0T's, T05 EMB0I's, T06–T07 COX0's, T08 LNT3's under the
LRA0 contract, and T09 XSDK1's. Each named route has one surviving owner and,
where CAT0 records one, its cutover owner. A producer that finds a further
receiving route on its implementation candidate inventories it with its
owner before mutation. The capability matrix is a required target
population, with zero currently qualified cells. Existing editor extensions,
bundler plugins and TypeScript providers are host adapters, not SDK guests;
this decision does not reroute them. XSDK0 deletes nothing: the boundary is
additive, its module homes contain placeholders and there is no current
dynamic plugin implementation to retire. Displaced first-party registries keep
their CAT0 cutover owners (CPF1, COX0, LNT3). XSDK1 reconciles this inventory
on its implementation candidate; subsequent producers migrate any route they
actually displace, including docs and examples.

## Contribution modes

The matrix crosses all nine CAT0 kinds with three contribution modes and both
host classes. A declarative contribution supplies descriptors referencing
already supported adapters. A data-only contribution supplies schema-validated
rows/facts with no executable callbacks. An isolated executable contribution
runs inside a qualified guest and returns only typed payloads: registration
rows/facts, tool-operation results read through existing typed queries, and
action/migration proposals. Computing registration data is not evaluating it:
a guest-computed row is admitted as data exactly like a data-only row and
confers no authority to execute what it registers. A descriptor or fact
cannot introduce an unsupported parser, backend or semantic interpretation:
that target remains unsupported until its core owner implements the adapter.
No mode supplies native function pointers, loads a native library or
registers a replacement core service. Native and browser cells qualify
independently; data support never implies executable support.

Rules keep one executor. Under LRA0 a contributed T08 row is data, never
executable third-party rule code. Its rule body runs only as an owned native
(or equivalent) implementation statically integrated into a first-party pack,
including in the WASM build, or through LNT2's explicit trusted external
process, which is never a silent fallback and which WASM and browser hosts
report unavailable. One native/external authority is selected per diagnostic
concept and exact profile, and the T08 registry cutover is LNT3's. No isolated
guest evaluates a rule body on either host. The XK06 isolated-executable cell
therefore covers action and migration proposals only, mediated by XSDK8.

XSDK1 admits descriptors for all kinds; XSDK2 owns fact ingress and rule and
operation identity validation; XSDK6 specializes XK08 HTML/CSS/component
metadata; XSDK8 mediates action/migration proposals and tool operations and
never executes a rule. XSDK3 and XSDK4 supply native and browser isolation and
XSDK5 the demand lifecycle; none of them is a rule executor. The matrix's
required-unimplemented label binds producer delivery; it does not weaken the
required outcome into optional work.

## Admission and compatibility

**E01 (XSDK1).** One manifest declares one namespace, exact contribution version,
manifest schema version, SDK protocol version, source/license, configuration
schema, contribution kinds/targets, dependencies on other contributions,
required and optional capabilities, execution mode and requested permissions.
Each profile target references the canonical typed identities and exactly one
release. Carrier targets contain no framework release. A dependency names an
exact T09 key (kind, namespace, exact version). Supported alternatives for a
target or dependency are separate exact entries, not ranges, floating tags,
implied majors or a wildcard. Capabilities are T07 `CapabilityId` values
qualified by their owning profile (VID0 I10). Requested permissions are a
separate space: the typed execution permissions (filesystem, network,
process, workspace, origin) XSDK3 receives from XEC0 through XEC1. Core, not
the manifest, mints T09 provenance from the admission path: declarative and
data-only manifests yield `data-manifest`, isolated-executable ones
`isolated-guest`, and `official-static` belongs only to statically registered
first-party rows. Core computes the content digest over the admitted manifest
bytes; a manifest-supplied provenance or digest is only an expectation that
must match exactly. These are contract fields, not a claim that a wire schema
exists.

**E02 (XSDK1, XSDK7).** Admission decides registration only: whether a T09 row
enters a new snapshot. It is an exact lookup of the manifest's schema and
protocol versions and, for executable mode, guest-format/engine release
against the host's published schema/protocol/build and guest tuple, plus
resolution of every target, dependency and capability id. That host tuple is checked, never part of snapshot identity
(CAT0 CR06). Unknown required fields, kinds, schemas, versions or ids, and a
missing dependency, reject with a typed reason. Grants, trust state,
effective configuration, project activation and host-cell isolation
availability are participation facts the DemandPlan reads per request (CAT0
CR11), never admission inputs. A registered contribution whose required
capability, grant or host cell is unavailable does not participate and
reports a typed disabled reason; an unavailable optional capability is
reported disabled and grants nothing. No downgrade, implicit fetch or native
library loading is a fallback. Inspection of static data never starts a
guest. XSDK7's permission review records grant and trust state under XEC0's
typed execution-permission and consent contract.

**E03 (XSDK1).** Validate all candidates before building a new immutable catalog.
T09 keys are (kind, namespace, exact version); duplicate keys reject naming both
owners, including equal-byte duplicates. Distinct versions may coexist only
with explicit selection and non-conflicting target claims; no highest-version
or load-order preference. CAT0 CR07 rejects duplicate target keys, editor ids
and equal-precedence suffix claims even across namespaces. Shared-carrier
claims require T06 relations (XK09); missing relations yield typed ambiguity,
duplicate relations reject. Canonical typed-key order determines serialization;
permuting discovery, validation or reply order never chooses a winner. Install,
upgrade, rollback or removal builds a new snapshot; grant, trust or
configuration changes alter participation for later requests and never
rebuild or mutate a snapshot.

## Contributions and non-replaceable authority

**E04 (XSDK2, XSDK6).** Declarative rows and data-only HTML/CSS/component metadata
are validated facts, with declared schema, canonical target, source spans/map
basis and provenance. Executable guests return the same typed fact payloads.
The owner validates shape, references, bounds and target before ingress and
merges through the canonical target-table owners in the inventory. A new
carrier or profile may supply bounded adapter facts through declared
interfaces, but may not replace a registered parser, recovery policy or
TypeScript semantics. Metadata text cannot masquerade as a type-engine answer.
A T08 row is validated against LRA0's registration fields and carries a
disposition naming an owned native body or LNT2's trusted external execution;
a row that carries rule code or asks a guest, a native library or any other
executor to evaluate its body is rejected. Unsupported semantic requirements
stay unsupported; there is no private resolver, completion engine or rule
engine.

**E05 (XSDK3, XSDK4).** Executable contributions require explicit trust and a
qualified guest format/engine with mediated inputs, memory/work/fuel/deadline
limits, cancellation and teardown. A guest computes registration rows/facts,
tool-operation results and action proposals; it never evaluates a rule body
and never receives rule-execution authority. No implicit filesystem, network,
process, workspace or browser-origin authority passes into a guest. Declared
scoped inputs are supplied through the host's acquisition boundary. A
subprocess or browser worker alone proves neither sandboxing nor permission
isolation. Host qualification names exact platform, engine, format and tested
permission cells; an unavailable isolation facility disables that executable
cell for participation rather than running untrusted code in-process. A
browser host reports LNT2's external execution unavailable and never
substitutes a guest. Data-only contributions remain usable.

**E06 (XSDK5).** Only selected demand executes a guest. The core supplies
source/config/build/session basis and bounded inputs, owns scheduling and
cancellation, and validates the reply against the live basis before atomic
publication. Reuse observes contribution bytes/version, relevant
configuration, catalog/profile and source/map dependencies through canonical
validity facts; no plugin cache key substitutes for them. Execution policy
remains waiter-local, not semantic identity. Cancelled, stale, malformed or
partial replies cannot become complete or warm a complete cache. Unselected
contributions do zero execution work. Teardown releases guest resources and
retained state on every exit path. Rule applicability and rule-body
execution stay with LRA0's lint service (LNT2, LNT3); the extension lifecycle
schedules no rule body.

**E07 (XSDK8).** Tool operations read through existing typed queries. Action and
migration output, including every XK06 isolated-executable output, is only a
proposed authored edit plan: core map reversibility, source revision
preconditions, conflict checks and explicit execution grants are mandatory
before LSO8's authored edit transaction engine applies anything. An extension
cannot write raw workspace bytes, approve its own plan or use an action path
to evaluate a rule body. Where a reversible authored map is absent, the
operation is refused, not approximated.

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
AC2 is E04/E05/E07 and cases N01–N03 and N13. AC3 changes no state/query/map
boundary here: fresh/incremental, edit/revert, cancellation, stale/partial
rejection and order perturbations belong to XSDK5/XSDK10. AC4 is E08. AC5 adds
no numeric budget; producers use the standing measurement rule: equivalent
answers and paired same-worker work/growth comparisons; timings/memory are
advisory, unavailable controls are recorded, and no stored receipt is a
performance gate.

## Condition justifications

These justify the contract conditions, not additional gates. Existing CI tests
current core behavior; it does not yet exercise SDK guest ingress or isolation.

| Conditions | concern | prevents | notCoveredBecause |
| --- | --- | --- | --- |
| E01, E03 | correctness | Plausible admission of an incompatible release, unresolved dependency, forged provenance or order-dependent owner when multiple packages claim a target | Core catalog rules do not yet have an external manifest producer; XSDK1 must apply them at that boundary |
| E02 | correctness | Grant state or host facilities changing snapshot membership and identity, against CAT0 CR06/CR11 | CAT0 rules have no grant input, and no SDK admission implementation exercises the boundary |
| E04 | correctness | Malformed guest metadata becoming authoritative semantic facts, or a contributed rule row gaining a second, unowned rule executor | Existing first-party typed producers do not validate untrusted guest payloads, and the LRA0 rule contract is CI-inert prose |
| E05 | security | Executable guests inheriting ambient workspace or network access, or running third-party rule bodies, if run as ordinary worker code | Core queries and browser worker lifecycle do not establish a guest security or rule-execution boundary |
| E06 | correctness | Delayed guest replies warming results after edit, upgrade or cancellation | Existing cache validity cannot cover an unrecorded external contribution dependency |
| E07 | correctness | A guest edit applying against stale or non-reversible generated coordinates | Core transactions protect edits only when all guest proposals enter that owner |
| E08 | process | Publishing supported cells from a manifest or upstream capability without authored integration proof | Generic CI success does not qualify each host/profile/permission tuple or run missing reference examples |
