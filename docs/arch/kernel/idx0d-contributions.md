# Atomic semantic contributions and candidate discovery

The workspace index owns typed, snapshot-qualified candidate storage and
bounded enumeration. A posting says where an owning operation may find a
fact; it does not resolve a symbol, decide a framework relationship, establish
project membership, or certify reachability. Producers retain those semantic
authorities. Consumers validate candidates through the producing vertical,
TypeInfo or PM operation before returning semantic answers or authored edits.

This decision describes the repository at `docs(arch): define the TypeInfo
component view and facet ownership (#875)`, 2026-10-13. The title is descriptive
context, never acceptance evidence. It follows the [docs-only rule](README.md):
this file is the contract, inventory and case table. It changes no production
route, test, dependency, schema implementation or CI gate, and deletes nothing.
IDX0 implements the storage and migration; UAO0 validates the ownership table
under `UAO0-AC-R1`.

## Imported boundaries and intent

The contract imports [orthogonal identities](identities.md), [coordinate
domains](coordinates.md), [catalog registration](catalog.md), [demand and
activation](demand-activation.md), [TypeInfo observation identity](typeinfo-facade.md),
and the [ComponentInfo view](tif1d-componentinfo-view.md). The logical names
below specify fields and laws, not additional public DTOs or another type
engine, project graph, activation registry or observation identity family.

Contribution production is an explicit `IndexContribution` demand under DEM0.
Only activated, claimed, demanded framework profiles produce their requested
tables. Neutral carrier/PM facts require their own declared demands and do not
depend on activation of a framework profile.
Dormant, ambiguous, unproven, withdrawn and selected-but-undemanded profiles
perform zero index projection. Shared carrier parsing happens only when an
admitted demand needs it. A broad search may request a bounded discovery plan;
it cannot trigger an eager deep workspace crawl or pretend unloaded files
were enumerated. PM owns acquisition/discovery scope and project facts.

IDX0 stores no checker APIs, provider sessions, executable callbacks, live
graph handles or evaluated component-meta copies. Relations refer to the
owner's registered operation and exact observations. WSI interprets
reachability and impact over these facts; absence from postings never means
dead code. Extension producers use the same contribution ingress with their
own validated attribution; first-party producers need no SDK dependency.

## Identity, basis and provenance

| Element | Required representation and equality |
| ------- | ------------------------------------ |
| `ContributionId` | Logical replacement slot: canonical tagged encoding of producer descriptor identity, registered table kind/schema contract, `SourceUnitId`, exact profile claim and producer partition. A claim carries `FrameworkProfileId` and `AttachmentId`/`RegionId` when applicable; carrier-only facts use `CarrierProfileId`, not a fabricated framework. Project-scoped slots also carry the PM-owned project identity. A partition is a declared bounded fact family, never a thread, timestamp or execution limit. |
| Source basis | Beside the slot: `SourceRevision` and `ContentId`, exact carrier/semantic/project profiles as applicable, catalog/schema identities, selection and demand epochs, captured owner snapshot and dependency read set. Changing bytes invalidates the value without changing the unit's lineage. Reopening or renaming a unit uses the source owner's lineage decision, never an index-minted path identity. |
| Record identity | Contribution slot plus the typed declaration/occurrence/relation slot assigned by the producer. Revision-local occurrence slots are qualified by their basis; they cannot be interpreted in a later revision. Duplicate names, overloads and multiple origins remain distinct. The index never creates a semantic target from a name. |
| Component key | TIF1D's existing OP10 `QueryIdentity`, including typed component selector, exact export/profile, canonical requested facets/arguments, observed profiles and `ResultContractId`. No basename key or ComponentInfo-specific observation key is minted. Subject postings may find several demanded views of one subject; they cannot merge them into a new view. |
| Observation references | The native/TypeScript child fact's `(QueryIdentity, InputBasisId)` and declaration/role slot. A composed ComponentInfo view retains its ordered child references: it has no single `InputBasisId`, and the index does not synthesize one. |
| Source anchor | Authored `SourceUnitId`, revision/content and checked half-open `SourceByteRange` in ENC0 `CD1`. Imported origins name the imported unit. Multiple origins remain multiple; unavailable origins carry a cause rather than a zero range. |
| Projected anchor | Original `CD3` generated or `CD4` embedded domain and producing map chain with `MapRevision`, plus the exact authored `CD1` origin when mapping succeeds. `CD2` relative spans are rebased only by their owner. Unmapped anchors cannot become authored occurrences or edits. `CD5`/`CD6` wire positions never enter index keys or core ranges. |
| Provenance | Producer descriptor/operation and result contract, exact profile/release, source and observation bases, fact quality/causes, relation derivation and its consumed observations. Runtime observations additionally name execution/build/session basis. Backend provenance stays value-side under the imported observation law. Display strings and optional traces are not validity evidence. |

All identities use the canonical tagged, length-delimited encoding and full
canonical-byte comparison on a suspected digest collision. Display family,
adapter spelling, path alias and file extension cannot substitute for nominal
profile or source identities. A schema revision incompatible with a payload
cannot be decoded as complete-empty. Unknown kinds/versions return an explicit
unsupported result; payload bytes are never opaque and unversioned.

Every row belongs to exactly one producer slot. Common evidence must be
complete even when optional audit capture is off. Missing authority, schema,
source basis, coordinate domain or read evidence prevents publication; adding
a guessed default is forbidden.

## Typed tables

Every table inherits the identity, provenance, completeness and ordering laws
above. A node contains its typed subject and occurrence/declaration slot; an
edge contains typed endpoints, relation kind, role, origin and evidence.
Unresolved or dynamic endpoints remain explicitly unresolved/dynamic with a
cause. A spelling used to find possible endpoints is a candidate key only.
External endpoints carry owner-qualified references, not dangling local row
indices. Consumers request the target owner's current observation.

| Table | Nodes / edges and candidate keys | Producing authority; contract receiver |
| ----- | ------------------------------- | -------------------------------------- |
| Names | Declaration and authored occurrence headers; import/export/name postings with typed scope and roles. Authored spelling and a versioned lexical search projection are separate from target identity. | Shared shallow inventory and owning symbol operation; IDX0 `IDX0-AC2` |
| Components | View references keyed by OP10 identity; usage/child/facet links with occurrence slots and exact profiles. Inputs, events, snippets/slots and models keep their producing facet semantics. | TypeInfo/TIF1D view and framework projection; TIF1 `TIF1-AC2`; storage IDX0 `IDX0-AC2` |
| Links | Authored imports, reexports, includes, component uses and other registered relations; source/target and static/conditional/dynamic/unresolved role. Package/module resolution proof is a PM reference. | PM for module resolution, each vertical for its relations; IDX0 `IDX0-AC2` |
| Registrations | Owner-qualified registration subjects and usage links: provide/inject, events/listeners, route/store and other declared domain relations. String-key and symbol-key candidates are distinguishable. | Registered producing vertical; IDX0 `IDX0-AC2` |
| Stylesheet symbols | Class, ID, custom property, definition/reference/manipulation and stylesheet links. Scope/module/global flags and style/template/script origins are producer facts, not index deductions. | CSS authority and owning framework projection; CSS1 `CSS1-AC1`; storage IDX0 `IDX0-AC2` |
| Config/env references | Typed config keys, environment references and source/config/target links, retaining realm/condition and uncertainty. Store references/provenance, never secret values or a deployment-absence guess. | CFG/PM/CENV producers; CENV4 `CENV4-AC1`; storage IDX0 `IDX0-AC2` |
| Test declarations | Runner-qualified declaration identity, authored anchor and static/dynamic/unknown discovery status. No result/pass/fail claim follows from a declaration. | Test discovery; TST1 `TST1-AC1`; storage IDX0 `IDX0-AC2` |
| Extension facts | Registered kind/version and typed subject/relation, attributed extension identity/version/permissions, validated origins, quotas, quality and read evidence. Unknown schemas are unsupported, never empty. | Extension producer through XSDK2 `XSDK2-AC1`; storage IDX0 `IDX0-AC2` |
| Project memberships | Unit-to-set and project-to-unit postings over PM-owned identity and membership observation. No glob, root, import or precedence inference in IDX0. | PM1 `PM1-AC1`; storage IDX0 `IDX0-AC2` |

Kind/schema and executable relation registration remain immutable catalog
rows. The registration table above stores source-backed domain registrations,
not a mutable duplicate of that catalog. A schema upgrade that changes a
contribution slot retires its previous schema's slots in the same fenced delta;
old and new layouts cannot remain parallel current authorities.

Declaration-signature contributions (AGM-TSIG), structural-pattern postings
(SSTR1) and relation registrations used by `query` (AGM10) are typed kinds on
this interface. Their authors add producer-local schemas/rows, not an IDX0
switch or edits to IDX0. Signature relations still ask TypeInfo; a pattern's
callee predicate still asks its symbol owner. A relation schema cannot create
an executable capability that its producer has not registered and delivered.
These obligations receive `AGM-TSIG-AC1`/`AC2`, `SSTR1-AC1`–`AC3` and
`AGM10-AC3`/`AC5`, respectively.

Deterministic table order is the canonical tuple `(table kind, profile claim,
typed subject, ContributionId, record slot)`; edges add relation kind and
typed endpoints. Project sets sort by canonical PM identity, not preference.
Semantic sequence positions (overloads, ordered children, signature arguments,
tuple positions) remain in their producer's slots. Type union/intersection
order stays `VerterStableV1`. Hash-map, load, scheduling and registration order
do not affect equality. Presentation ranking, basename casing and labels ride
a separate projection; ties use canonical record identity. Deduplication uses
record/target identity, never label alone.

## Enumeration and fact quality

Enumeration status and the quality of each underlying fact are independent.
An exact enumeration of unresolved relation records does not resolve them.
A complete posting population may contain approximate owner facts only when
their guarantees and causes are retained and the producing owner admits the
fact; no consumer can promote them to an exact semantic answer.

| Enumeration status | Claim and reuse |
| ------------------ | --------------- |
| `Complete` | All requested partitions for the declared source/project/profile scope were enumerated on one valid basis. Only then may the completed contribution enter persistent postings. Complete-empty certifies no candidates in that declared population, never semantic absence or absence outside it. |
| `Partial { cause, covered scope, continuation }` | Positive candidates may be returned for this request with the missing scope and machine cause (budget, unavailable input/producer, open discovery or unsupported coverage). They and their enumeration result are `ReturnOnly`: no staged prefix, exact negative, complete membership or query-result cache entry. A continuation is optional when retry cannot resume; otherwise it is exact-snapshot-qualified. |
| `Cancelled` | No contribution, candidate page or refusal is published by the cancelled work. Staging is discarded. Retry executes against a valid captured basis. |

Known unsupported applicability and missing inputs are typed causes, not
complete-empty populations. An undemanded partition is outside the request,
not absent. A limit reached before proving exhaustion is Partial even when
the last visited posting happens to match the last returned candidate.
An old contribution does not become current because a replacement failed.

Partial index enumeration never publishes exact negative or complete
membership. The separate native owner may retain an eligible semantic refusal
under its exact profile/context, root identity and failed-prefix evidence.
IDX0 retains no refusal for a cancelled, budget-exhausted or partial walk;
child refusal reuse cannot warm the enclosing index result or turn it into
negative membership. This preserves the 2026-09-30 partiality amendment.

## Staged atomic `ContributionDelta`

`ContributionDelta` describes a finite set of typed slot additions,
replacements and removals, its expected prior slot bases, captured publication
basis, complete enumeration scope and dependency read sets. It is a transaction
over contributions and all their forward/reverse postings, not incremental
mutation of visible maps. Replacement publishes a whole declared partition;
records absent from a partial prefix are never removals.

1. Capture one coherent source/project/catalog/selection view and demand plan.
   Existing valid postings may serve bounded discovery; demanded producers
   obtain cached shallow facts or owner observations through shared ports.
2. Stage the rows and affected postings privately. Validate schema, producer
   authority, source ranges/maps, exact claims, read evidence, endpoint forms,
   complete partition coverage and quotas. No reader sees staging.
3. Revalidate the publication fence: expected prior slot bases still match,
   all source/read observations remain current, and selection/demand epochs
   have not been superseded or withdrawn. Cancellation or budget failure at
   any boundary, including immediately before the swap, discards the delta.
4. Publish all slot changes and postings as one immutable index snapshot.
   A reader sees the previous complete snapshot or the new complete snapshot,
   never replaced nodes with old reverse edges or half a membership set.
   A failure in one slot rejects the whole delta; it cannot commit siblings.

Concurrent writers compare the expected slot bases. A losing writer returns
stale/conflict and replans on a fresh captured view; it cannot overwrite a newer
contribution by completion order. Disjoint deltas may share one publication
only after every basis has been revalidated on that view. Replay of an already
committed delta is idempotent only when both slot identities and full bases
agree. Another value under that identity is a conflict, never last-writer-wins.

Removal needs a current authoritative deletion/retirement observation from
the source, PM or activation owner, with its basis and expected prior slot.
Profile withdrawal retires its contributions without projecting the inactive
profile. Deletion of a source or profile atomically retires owned rows and
their postings; retained external references remain unresolved until their
producer updates them. No incomplete scan can issue a tombstone for unseen
records. Read-set invalidation marks old entries ineligible immediately;
requests cannot serve them as current while a rebuild is pending.

Each query captures one immutable index snapshot and its compatible owner
view. A historical snapshot is usable only for a request explicitly tied to
that basis. After observing a newer required basis/epoch, work under the old
one returns stale/return-only or cancels and cannot publish warm or materialize
current edits. Readers holding old snapshots retain them by reader lifetime;
retirement is bounded by the existing host cache/resource policy, not a new
timer or a forever-growing per-query workspace copy.

## Dependency read sets and incremental equivalence

The read set contains the exact owner observations used to derive rows and
coverage, independently of optional chronological traces:

- Source units, revisions/content, demanded shallow header/declaration slots
  and projected map revisions actually consumed.
- TypeInfo child observations and their bases, exact framework claims,
  result/schema contracts, catalog and activation/demand identities.
- PM membership/project/environment/resolution observations; config/package,
  directory/discovery or absent-input evidence the owner used, including
  conditions, aliases and reference visibility when relevant.
- Relation endpoints and contributing slots/bases, plus scoped enumeration
  witnesses for any complete candidate population or removal.

An exact negative dependency requires its owner's complete failed-prefix or
absence witness for the queried scope. A directory or project addition can
affect a result even when no existing source changed, so complete enumeration
records its population witness; a list of files previously seen is insufficient.
IDX0 records/validates references to these witnesses, never probes the
filesystem, executes config, resolves modules or re-derives membership.

When a dependency observation changes, invalidate its dependent contributions,
their postings, and query coverage/results that consumed them. Changes to
population witnesses invalidate complete-empty queries and project-to-unit
enumeration too. Unchanged unrelated partitions remain eligible. Map-only
changes invalidate affected anchors without claiming source semantics changed;
profile/release/context changes cannot reuse a family-wide entry. Presentation
changes invalidate only the presentation projection. A reverted content digest
alone does not authorize reuse: every revision, map, context and read witness
must still validate under the owner's reuse law.

For a captured view V and demand D, incremental lookup equals fresh derivation
on V and D when every served contribution/read witness validates, the declared
population coverage is complete, and all invalidated partitions are replaced
atomically before they are served. Equality includes candidate identities,
order, origins, causes, fact quality and coverage, not just names or counts.
Changed inputs outside the read/coverage set do not change that answer.
Otherwise the query remains partial/stale and return-only or reruns affected
owners; it cannot manufacture equivalence by hiding a missing partition.
IDX0 proves edit/revert, add/delete, overlapping membership, cancellation,
concurrent stale completion and incremental/fresh cases under `IDX0-AC3` and
`IDX0-AC-R1`.

## PM-owned set-valued project membership

PM supplies a membership observation for `SourceUnitId` on one PM snapshot:
the complete set of PM-owned configured/inferred project identities and
their environment references, its discovery/read evidence and status. Kernel
identities remain kernel-owned; PM derives their project facts. Configured
projects use VID0's `ConfiguredProjectId`; any inferred-project identity is
supplied by PM over kernel identities, never minted here from a path hash.
Positional `ProjectId(u32)` in a current snapshot is not a durable key.

IDX0 stores that observation as one membership contribution, including its
exact PM basis, and derives only mechanical forward/reverse postings. Two
legitimate projects produce a two-member set; IDX0 cannot choose the first,
closest root or open editor as the sole owner. A preferred owner is a separate
PM decision and never deletes other members. Project/profile identities are
orthogonal: matching framework profiles do not imply matching projects, and
membership does not activate a profile.

A complete empty set means PM proved no member for that unit and scope. A
partial PM observation may expose known positive members return-only but
publishes no membership contribution and proves no excluded project. PM's
`NeedInputs`, `Ambiguous`, `Unsupported`, `Cancelled` and `Stale` outcomes keep
their causes; ambiguity between preferred owners does not erase an otherwise
proved set. Membership changes replace the whole unit set and its inverse
postings atomically. Unit-to-project completeness does not imply that all units
of a project were discovered: reverse enumeration requires PM's complete
project population witness as well.

The current `WorkspaceSnapshot::owners_for_file` already returns overlapping
owner sets, and `ConfiguredOwnerResolution` represents ambiguity separately.
Those are evidence that PM membership is expressible without IDX0 deriving
it, not a claim that the future PM identity vocabulary is implemented today.
PM1 receives identity/derivation and preferred-owner proof (`PM1-AC1`);
IDX0 receives storage and partiality proof (`IDX0-AC2`, `IDX0-AC-R1`).

## Current producer/consumer and displacement inventory

Paths and symbols below were inspected on the described repository. A grouped
row enumerates its finite fields/callers and assigns that route one receiving
owner and acceptance. Storage migration does not transfer semantic authority.
Receiving owners recheck the population on their implementation candidate;
parity precedes retirement, with one final route and no dual-running index.
The earlier [authority inventory](authority-inventory.md)'s `D18` is referenced
here; its older empty framework-index population is not current evidence.

| Route | Current index/walk, producer and consumers | Disposition; sole later owner and receiving acceptance |
| ----- | ------------------------------------------ | ----------------------------------------------------- |
| I01 | `verter_semantic::analysis::ProjectIndex.files`, `provide_index`, `inject_index`; input `FileUsageInfoOwned` via `add_file`/`remove_file`; queried by provider/inject accessors, summaries and diagnostics `build_cross_file_snapshot` | Replace candidate storage with typed name/registration postings; IDX0 `IDX0-AC1` |
| I02 | `ProjectIndex.component_graph`, `component_reverse_index`; component usages supplied in `FileUsageInfoOwned`; component accessors/summary and cross-file snapshot consumers | Replace adjacency and reverse candidate storage, retain producer relation meaning; IDX0 `IDX0-AC1` |
| I03 | `ProjectIndex.class_index`, `id_index`, `v_bind_css_index`, `custom_property_index`, `var_reference_index`, `template_css_var_index`, `script_css_var_index`; file usage producers; stylesheet accessors and `css_var_flow` | Replace style postings, preserving definition/reference/template/script distinctions; IDX0 `IDX0-AC1` |
| I04 | `ProjectIndex.emit_index`, `listener_index`, `template_id_index`; file usage producers; event flow/template-ID accessors and cross-file diagnostics | Replace event/ID candidate postings; IDX0 `IDX0-AC1` |
| I05 | `ProjectIndex.store_usage_index`, `store_definition_index`, `store_dep_graph`; file usage producers; store flow/summary and diagnostics cycle candidate input | Replace store registration/link storage; IDX0 `IDX0-AC1` |
| I06 | `ProjectIndex.route_snapshot` from `set_route_snapshot`; route accessors/flattening; snapshots contain owner-produced route definitions | Replace route candidate storage without deriving routing semantics; IDX0 `IDX0-AC1` |
| I07 | LSP `server_utils::build_workspace_components`: `host.list_files` + carrier classification + basename projection; completion in `server/nav_features.rs` | UAK0 `D18`: retire per-request discovery in favor of component candidates, preserving Vue/Svelte name/import-path results; IDX0 `IDX0-AC1` |
| I08 | LSP `features/workspace_symbol::workspace_symbols`: `list_files` + binding/template/style analyses; `server/aux_features.rs` workspace-symbol handler | Replace all-file candidate aggregation with scoped name/component/style lookup; IDX0 `IDX0-AC1` |
| I09 | LSP `css/global_classes.rs` all-host-files lookup: cached style/template analyses; references and rename in `server/nav_features_css.rs` | Replace candidate discovery, preserving scoped/module/global distinctions; IDX0 `IDX0-AC1` |
| I10 | Host `host_manage/analysis_io.rs::css_var_flow`: scheduler node IDs and effective style/script/raw-template facts; public host callers and tests | Replace per-query candidate walk, preserving profileless and override-aware distinctions; IDX0 `IDX0-AC1` |
| I11 | LSP `server/custom_methods/mod.rs` `get_project_overview`, `get_route_tree`, `get_component_parents`: `list_files` and per-file analyses | Replace consumer-private component/population discovery; IDX0 `IDX0-AC1` |
| I12 | MCP `server.rs` `get_component_graph`, `check_component_props`, `detect_prop_drilling`, `trace_event_flow`, `validate_provide_inject`, `get_store_usage`, `get_store_graph`, `trace_store_flow`, `build_route_snapshot`: private maps/graphs from `list_files` and cached analyses | Replace candidate walks and retained whole-workspace copies with bounded typed relation queries; IDX0 `IDX0-AC1`. Owner operations still validate props, flow and registrations. |
| I13 | MCP `detect_css_bleed`: all-file class/template maps from analysis; project CSS comparison | Replace class candidate discovery; IDX0 `IDX0-AC1`; CSS interpretation remains CSS-owned |
| I14 | MCP `find_orphan_components`, `get_test_impact`: local traversal of component/import facts; orphan/affected-file responses | Retire private reachability/impact interpretation over a guessed closed graph; WSI4Q `WSI4Q-AC1`/`AC2`, consuming IDX0 candidates and PM roots |
| I15 | MCP `get_project_stats`, `detect_migration_targets`, `ssr_project_report`, `analyze_route_health`: all-file population walks for aggregate owner facts | Replace population discovery with scoped typed candidate enumeration; IDX0 `IDX0-AC1`; scoring, migration, SSR and route judgments stay with their producing operations |
| I16 | LSP references/rename in `server/nav_features_navigation.rs`, `rename_plan.rs`, `child_prop_rename.rs`: local/provider occurrences and child declaration validation; feature-specific candidate/occurrence planning | Converge on bounded `OccurrencePlanner`, preserve authoritative declaration validation and completeness guards; LSO4 `LSO4-AC1`, consuming IDX0 |
| I17 | LSP workspace/native/framework/provider completion composition and label collision handling, `server/nav_features.rs` and completion features | Retire private semantic candidate composition; LSO6 `LSO6-AC1`, consuming IDX0; presentation names never deduplicate distinct targets |

I01–I13 and I15 assign the indexed discovery/storage boundary, not the
consumer's semantic body or public tool retirement. In particular, the MCP
consumer migrations retain their existing owners below. They use the typed
lookup instead of copying candidate walks; they do not implement another
index. I14 assigns reachability/impact interpretation to WSI, while the public
test operation remains its own consumer.

| Consumer | Current operations consuming the indexed facts | Sole later consumer owner and receiving acceptance |
| -------- | ---------------------------------------------- | -------------------------------------------------- |
| M01 | MCP `get_component_graph`, `check_component_props`, `trace_event_flow` and component-usage projections | AGM-COMP `AGM-COMP-AC1`/`AC4` |
| M02 | MCP `validate_provide_inject`, `detect_prop_drilling`, `get_store_usage`, `get_store_graph`, `trace_store_flow`, `ssr_project_report` | AGM-VUE1 `AGM-VUE1-AC3`; later context/effect producers keep their separately registered semantics |
| M03 | MCP `detect_css_bleed` and stylesheet relation presentation | AGM-CSS `AGM-CSS-AC2`/`AC3` |
| M04 | MCP `get_test_impact` / affected-test presentation over WSI impact and test discovery | AGM-TEST `AGM-TEST-AC1` |
| M05 | MCP `build_route_snapshot`, `get_route_tree`, `get_route_for_component`, `get_navigation_map`, `get_router_views`, `analyze_route_health` | AGM-ROUTE `AGM-ROUTE-AC1` |
| M06 | MCP `get_project_stats` | AGM-VUE2 `AGM-VUE2-AC3`/`AC4` |
| M07 | MCP `detect_migration_targets` | AGM-MIGV `AGM-MIGV-AC3`/`AC4` |
| M08 | MCP `find_orphan_components` presentation over WSI reachability | AGM-WS `AGM-WS-AC1` |

`ProjectIndex` is an exported standalone analysis structure, not the live host
workspace authority. Its current construction in the inspected tree is in
tests; `verter_diagnostics::build_cross_file_snapshot` is its library consumer,
not evidence that the host populates it. The host/MCP walks above run
independently. Its semantic helpers (`validate_inject`, store-cycle detection,
route matching) are not index answers: their owning diagnostic/domain
operations consume candidate evidence and require their own completeness.
`OriginEdgeKind::ProjectIndex` is a type-member projection audit edge and is
unrelated to workspace candidate storage; it is not a deletion target.

### Retained owner-local indexes and walks

These are part of the current inventory, but are not displaced semantic
candidate databases. Exactly one later integration owner is listed per route;
the existing semantic authority is retained rather than copied into IDX0.

| Route | Producer, index and consumers | Retained authority / receiving owner and acceptance |
| ----- | ---------------------------- | --------------------------------------------------- |
| R01 | Workspace snapshot builder → `WorkspaceSnapshot.projects`, materialized `ConfiguredMembership`, `OwnersMemo`/declaration ranks → host/resolver/LSP ownership queries | Membership derivation and overlap remain PM-owned; PM1 `PM1-AC1` |
| R02 | Workspace config/package facts → `ProjectGraph`, `PackageIndex`, `DirIndex`, exact resolution cache/proofs → workspace resolver and host resolution ports | Resolution stays PM-owned; PM2 `PM2-AC1`/`AC2`; IDX0 consumes proofs only |
| R03 | LSP `workspace_state::LspViews` and `LspProjectView` from published workspace → lint and provider adapters | Thin views of PM snapshots, never another graph; PM3 `PM3-AC1` |
| R04 | Workspace carrier discovery and LSP `workspace_scanner.rs`, MCP `scanner.rs::scan_directory`, unplugin `core/scanner.ts` → host loading/provider/bundler batches | Explicit acquisition/discovery remains host/PM-owned; PM1 `PM1-AC1`. Scanning cannot certify contribution coverage without the PM witness. |
| R05 | Shallow source processing → `IndexedReady`, `ScriptShallowIndex`, declaration/header/root-binding/class/flow-span indexes and TypeInfo `list_file_symbols` → native queries, solver and local navigation | Shared native file/TypeInfo authority retained; TIF1 `TIF1-AC2` integrates observation references; IDX0 retains headers/references, never reparses or resolves |
| R06 | `global_contributors/mod.rs` per-file header facts → `GlobalContributorIndex`/`GlobalContributorPopulation` → global and augmentation type queries | Shared TypeInfo/native authority retained; TIF1 `TIF1-AC2`; complete global contributor semantics are not replaced by name postings |
| R07 | Framework/catalog descriptor registration → `ActiveProviderIndex`/language registry → provider selection and carrier classification | DEM0/CAT0 selection ownership retained; COX0 `COX0-AC2`; index registrations are domain facts, not a second activation registry |
| R08 | MCP `list_files`, `lint_project`, `save_baseline` and file tools → host tracked population/local lint or explicit baseline capture | Declared batch/local/capture demands, not an index migration; COX0 `COX0-AC2` binds demand admission. Tracked files never claim complete workspace membership. |

At this head there is no universal contribution/delta API, PM-identity-keyed
membership posting table, test-declaration index, config/env contribution
table or typed third-party extension posting store. Their schemas are additive
slots above, not invented existing deletion targets. Existing configuration,
test and extension products keep their producing authorities. No consumer is
promised a complete population until its producer and scope are available.

## Work bounds by query family

The following are counter specifications for IDX0 `IDX0-AC4`, not timings or
stored transcripts. Let K be requested lookup keys, B the admitted posting
visit budget, R the returned record budget, A the admitted adjacency visits,
and M the PM-supplied memberships examined. Every query names its scope,
exact snapshot, requested tables and these finite budgets. Work performed to
validate/filter duplicates, stale rows or nonmembers counts against B/A even
when it returns no candidate. Output ordering costs at most O(R log R).

| Family | Required counted work and discriminating control |
| ------ | ------------------------------------------------ |
| Name/component/search | At most K keyed lookups and B posting visits, returning at most R records. Lexical search uses versioned candidate postings, not per-file analysis. An empty/broad query consumes B and returns Partial unless exhaustion is proved. Adding unrelated files with unchanged matching postings adds zero file visits. |
| Consumer/rename seed | At most K target/relation lookups and A adjacency visits plus B candidate checks. IDX0 does zero semantic resolves or occurrence validation; LSO/TypeInfo own those demands. Repeated declaration/target lookup does not scan all files. |
| Style/registration/config/env/test/extension | At most K kind/subject/role lookups and B postings, with owner-defined scope filters. Unsupported/missing producers return explicit causes without running unrelated producers. |
| Membership | Unit-to-project inspects at most M returned PM set entries; project-to-unit at most B inverse postings. No path/glob matching, module resolution or config reads inside IDX0. Overlap retains every admitted member and reports Partial rather than truncating the set as complete. |
| Incremental replacement | Visit only invalidated slots, their read-set dependents and the old/new postings affected by the delta. Unrelated-row rewrite, parse and semantic resolve counts are zero. A changed population witness may invalidate a broader declared scope, which must be counted explicitly. |
| Warm/undemanded | Valid unchanged lookup parses/resolves/projects zero files; inactive or undemanded profile projection count is zero. Capture off adds zero observation-only work. |

For scaling controls, grow unrelated workspace size while holding K, matching
postings and budgets fixed: candidate visits remain fixed. Grow matching
postings with B fixed: visits stay at most B and status becomes Partial when
coverage exceeds it. Grow a fully admitted population with its budget: counted
visits grow linearly with admitted rows/edges, not quadratically through
consumer-private scans. Cold projection counts each demanded canonical
source/hash once through the shared parse/shallow path.

Retained rows/postings/read edges are proportional to admitted live
contributions; per-query scratch is bounded by B/A/R/M. Query materializations
do not retain an unversioned whole workspace. IDX0's memory-plateau case
repeatedly edits/removes/replaces the same bounded live population and releases
readers: live row/posting/read-edge counts return to that population's bound.
Existing resource policy bounds retained candidates and reader snapshots;
no new eviction semantic or hard RSS number is chosen here. Timing, allocation
and memory measurements are advisory; production timing belongs to a named
`bench-m3` evidence run, never a local wall-clock acceptance number.

## Executable case table for the receiving implementation

These cases are specifications, not tests or claimed runtime passes. IDX0
receives them under `IDX0-AC-R1`; the extra owner named in a row proves its
downstream semantic boundary. UAO0 receives the ownership validation, not the
production implementation. Each case names the distinct boundary it tests.

| Case | Input / perturbation | Required result; forbidden result | Receiving acceptance |
| ---- | -------------------- | -------------------------------- | -------------------- |
| C01 | Vue and Svelte components, aliases/barrels, same basename in different units, multiple facets and duplicate event/overload occurrences; perturb load/thread order | Stable ordered bounded candidate identities and exact origins; owner validation chooses targets. No name-only merge or evaluated metadata copy. | IDX0 `IDX0-AC2`, `IDX0-AC-R1`; TIF1 `TIF1-AC2` |
| C02 | Search/consumer/rename seeds with unrelated similarly named symbols and imports | Candidate supersets remain candidates; authoritative TypeInfo/vertical validation excludes unrelated targets. No index-driven authored edit or semantic resolution. | IDX0 `IDX0-AC-R1`; LSO4 `LSO4-AC1` |
| C03 | Cancel before staging, mid-enumeration and immediately before publication; retry | No delta/page/refusal/warm entry from cancelled work; retry equals fresh. Prior current contributions remain intact only if their bases still validate. | IDX0 `IDX0-AC3`, `IDX0-AC-R1` |
| C04 | Budget exhaustion or missing producer halfway through rows, including zero matching positives | Partial with nonempty cause and covered/missing scope; resumable cursor when possible. No warm prefix, exact negative, complete membership or removal of unseen rows. | IDX0 `IDX0-AC-R1` |
| C05 | Replace nodes/edges/membership together; fail validation in one partition; inspect concurrent readers | Readers see whole old/new snapshots only; failed delta commits nothing. No stale reverse edges or committed healthy siblings from the failed batch. | IDX0 `IDX0-AC2`/`AC3` |
| C06 | Edit/revert source, map-only edit, source add/delete/rename, package/config change and a previously absent dependency becoming present | Incremental equals fresh for identities/order/origins/causes/coverage on each view; dependent entries invalidate and unrelated partitions stay eligible. No content-only warm admission. | IDX0 `IDX0-AC3`, `IDX0-AC-R1` |
| C07 | Same unit legitimately in two configured projects, inferred/no-owner unit, partial PM discovery and incomplete reverse population | Preserve full PM set and separate preferred-owner/ambiguity facts; partial positives are return-only. No closest-root derivation or complete reverse enumeration from only forward-set proof. | IDX0 `IDX0-AC-R1`; PM1 `PM1-AC1` |
| C08 | Two concurrent replacements of one slot; older source/activation/demand epoch finishes last; profile withdrawal | Expected-basis conflict rejects stale writer; withdrawal retires postings atomically. No old entry served as current, warmed result or completion-order winner. | IDX0 `IDX0-AC3`; COX0 `COX0-AC3` |
| C09 | Dormant, ambiguous, unproven, withdrawn and selected-but-undemanded profiles; capture off | Zero index projection for those profiles and zero observation-only work with capture off. Existing demanded carrier work is shared. | IDX0 `IDX0-AC4`; COX0 `COX0-AC4` |
| C10 | Imported/embedded/generated origins with non-ASCII bytes, changed map and unmapped range | Domain-tagged exact owner/map basis; unavailable origin remains explicit. No synthetic zero anchor, parent-file offset or boundary-encoding key. | IDX0 `IDX0-AC2`; ENC0T coordinate contract |
| C11 | Unknown payload version, malformed/unattributed extension, missing read evidence, declaration/registration without a delivered operation | Reject ingress or return unsupported; preserve cause/producer. No complete-empty decode, callback in storage or invented executable capability. | IDX0 `IDX0-AC2`; XSDK2 `XSDK2-AC1` |
| C12 | Native child refusal under exact profile/context and failed-prefix evidence inside a partial index walk | Child owner may reuse eligible refusal; index remains Partial/ReturnOnly. Cancellation leaves no retained refusal. | IDX0 `IDX0-AC3`, `IDX0-AC-R1` |
| C13 | Grow unrelated files, grow matching postings beyond B, repeat edit/delete cycles and release readers | Query visits obey family bounds; saturation stays Partial; retained live rows/postings/read edges plateau. No hidden whole-workspace scan or negative budget admission. | IDX0 `IDX0-AC4`, `IDX0-AC-R1` |
| C14 | Scoped/module/global CSS classes and profileless/override-aware CSS-variable facts in Vue/Svelte | Preserve candidate/result parity and exact scope/basis. No cross-file scoped-class match or profile override leaking into raw results. | IDX0 `IDX0-AC1`/`AC2`; CSS1 `CSS1-AC1` |
| C15 | Unknown dynamic edge, incomplete project roots or missing deployment env/test producer | Explicit uncertainty; no definitely-dead verdict, secret absence or exact missing-test result from postings. | IDX0 `IDX0-AC2`; WSI4Q `WSI4Q-AC2`; CENV4 `CENV4-AC2`; TST1 `TST1-AC2` |

## Evidence selection and acceptance

The evidence for this docs-only decision is inspection of the named current
routes, the reviewed tables and the imported contracts. Existing behavioral
coverage identifies parity controls, but is not proof that the successor
contribution service exists:

- `crates/verter_semantic/src/analysis/project_index_tests.rs`: `component_graph`,
  `update_file_reindexes`, `remove_file`, `provide_index`, `inject_index` and
  stylesheet/event/store cases cover current standalone storage.
- LSP `server/tests/workspace.rs`:
  `build_workspace_components_enumerates_svelte_and_strips_extension` and
  `build_workspace_components_dotted_collision_is_deterministic`;
  `integration_tests.rs::workspace_symbols_includes_svelte_components`
  cover current cross-framework candidate presentation.
- Host `tests/cases/g_misc0/host_tests.rs::css_var_flow_across_files` and
  `unissued_template_content_is_refused_without_affecting_css_var_flow`
  cover the current raw/override boundary.
- Workspace `workspace_snapshot_tests.rs`:
  `overlapping_tsconfigs_both_returned`,
  `ambiguous_configured_resolution_for_overlap` and
  `memoized_owners_equal_fresh_compute_for_member_excluded_and_non_member`
  establish current PM-shaped set/ambiguity and memo behavior.

`IDX0D-AC1` is the inventory and single-owner receiving table; its executable
validator belongs to UAO0 (`UAO0-AC-R1`). `IDX0D-AC2` is the identity/table/status
law and C01–C15; IDX0 executes `IDX0-AC-R1`. `IDX0D-AC3` is the read-set and
invalidation law, with runtime equivalence assigned to IDX0. `IDX0D-AC4` is
the query-family work specification, measured by IDX0.

Local behavioral TDD, new tests, runtime incremental comparisons and
performance runs are not applicable to this patch: it changes only this
contract file, leaving every production cache, scheduler, parser and query
route untouched. The receiving case/work obligations above remain mandatory;
they are not waived by this rationale. Local configured documentation/format
checks provide author evidence; fresh architecture reviews and independent
workflow verification evaluate the published candidate separately. No test
count, timing transcript, historical object pin or fabricated review verdict
is part of this contract.
