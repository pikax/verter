# Profile-scoped diagnostics, lint, fixes and actions

Rules and actions are immutable, profile-qualified rows in `CatalogSnapshot`
table `T08`. A request selects them through `DemandPlan`; registration itself
does no analysis. A diagnostic, suppression or edit cannot borrow another
release's rule identity, facts or verification. The common service owns
scheduling and publication; vertical packs own exact-release semantics.

This decision describes `docs(arch): define atomic contribution and candidate
contracts (#879)`, 2026-10-13. The title and date are descriptive context.
It follows the [docs-only rule](README.md): reviewed contract data only, with
no production change, executable schema, test, validator or CI lane.

## Imported authority and reviewed products

The [identity law](identities.md) owns `CarrierProfileId`, `FrameworkProfileId`
and `ProjectProfileId`; [catalog registration](catalog.md) owns snapshot
construction and `T08`; [demand planning](demand-activation.md) owns selection.
[Configuration](configuration.md) supplies captured layers and provenance.
[ComponentInfo](tif1d-componentinfo-view.md) is a TypeInfo view, and
[contributions](idx0d-contributions.md) supply typed index facts and read sets.
Rules use `TypeInfoRequest` for demanded semantic facts, never another resolver,
component scanner or display-text interpretation.

The controller owns DAG nodes, charters, readiness and implementation state.
The reviewed products under `tests/kernel/LRA0/products/` are contract data:

| File                              | Content                                                                                                                             |
| --------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------- |
| `rules-actions-inventory.v1.json` | Outcomes, consumers, current routes, single receiving owners, successor paths, imported retirements and empty populations           |
| `rules-actions-cases.v1.json`     | Registration, zero-work, epoch, provenance, edit and ownership cases; representative authored fixtures and existing parity controls |

The inventory gives each new outcome and consumer one implementation owner.
Each displaced route has one production-capable deletion/rejection owner.
Imported routes retain the original contract's owner and path; they are not
reassigned here. UAP0 executes ownership validation (`UAP0-AC-R1`) and the
three transferred applicability/epoch/edit cases (`UAP0-AC-R2`). LNTV0 and
LNTS0 execute their representative migration cases (`LNTV0-AC-R1`,
`LNTS0-AC-R1`). A table is an obligation, not evidence of completed code.

## Current boundary and completeness audit

The inspection starts at all four charter surfaces and follows their actual
producers to consumers:

- `verter_identity` supplies source lineage, revision, content and map identity.
  The successor profile types are specified in VID0; they are not implemented
  merely because this decision names them.
- `verter_language::LanguageDiagnostic` supplies carrier diagnostics and a
  typed-argument ordering key. `RuleApplicability::Adapter` in
  `verter_diagnostics` instead selects rules by `FrameworkAdapterId`;
  `LintRule::applicability` defaults to Vue. `RuleRegistry::builtin` constructs
  a mutable global list and `for_carrier` filters it after construction.
- `verter_session` serves analysis, framework-surface TypeInfo and
  component-meta observations. Bindings currently compose lint and actions
  outside the session. No profile-scoped lint service is present here.
- `verter_protocol::typeinfo::TypeInfoRequest` is the graph-request alias.
  Its current framework surface carries adapter/schema and support status;
  it is not already an exact-release `T08` request. `LintDiagnostic` and
  `CodeAction` have separate public converters and unqualified rule strings.
- `verter_actions::ActionEngine` invokes every registered provider.
  `FileEdit` carries a byte span and optional file spelling, without a source
  revision/map basis. `AutofixSafety` conflates the old classifications with
  no independent applicability evidence. ACT1 migrates this model; LSO8
  supplies authored transactions; ACT2 supplies verification.

The finite consumer population is lint configuration, catalog/rule selection,
analysis and TypeInfo, suppressions, diagnostic publication, action planning,
LSP, NAPI/WASM/FFI and MCP. The inventory names the live paths for each.
MCP scoring is included: its direct `Linter::new` call is a consumer even
though it presents a score instead of individual diagnostics. Rule bodies,
providers, compiler diagnostics and the component view remain with their
existing owners. The initial shared invocation route is retired once by
LNT3; ACT1's model conversion does not create a parallel registry.

## Exact registration identity

Each `T08` row declares:

| Field           | Contract                                                                                                                          |
| --------------- | --------------------------------------------------------------------------------------------------------------------------------- |
| Rule/action key | Nominal rule or action id, typed owning profile and rule epoch; semantic and project profile variants remain distinct             |
| Release         | The profile's one admitted exact release, never a range, display family, adapter spelling or implied major                        |
| Ownership       | One pack/provider owner, native/equivalent/external/unsupported disposition, diagnostic concept and public capability             |
| Applicability   | Carrier/region roles, profile and project-overlay requirements, capability and evidence predicates                                |
| Fact demand     | Typed fact kinds, TypeInfo operations/levels, contribution tables, completeness obligations and read-set dependencies             |
| Output          | Diagnostic schema, help and secondary labels, suppression category, action alternatives and declared postconditions               |
| Provenance      | Static pack or validated data contribution origin, exact oracle version/cell where compatibility is promised, semantic rule epoch |

Common rows belong to the common pack and declare neutral fact contracts;
they are not copied into a Vue-shaped global table. A vertical row may reuse
a common body but owns its vocabulary, defaults, aliases and release-specific
admission. A supposedly common body that must branch on framework meaning
returns to its vertical owner before mutation. Adding a family adds rows in
that family's pack, without another neutral dispatch arm.

Snapshot construction rejects duplicate keys, missing owners, unknown fact
schemas and dangling capability references. Different profiles may use
different epochs for the same rule spelling. Changing a rule's semantics,
demand or output contract changes its epoch and snapshot identity. Effective
configuration, project membership and source revisions remain request facts;
they never mutate the catalog. A contributed row is data, not executable
third-party rule code. Explicit trusted external execution belongs to LNT2
and is never a silent native fallback.

## Applicability before work

The lint/action request binds configured project, source unit/region and exact
revision, carrier, selected semantic/project profiles, catalog identity,
capability demand, effective configuration and its provenance. It reads the
captured activation evidence and row predicates before requesting rule facts.
An inapplicable, disabled or withdrawn row performs zero rule-body calls,
TypeInfo queries, index walks, subprocess launches or rule-owned allocations.
Shared carrier parsing required by a separate admitted operation is not
charged as rule work. Applicability itself must not trigger deep resolution.

An applicable row requests only its declared facts through the shared owners.
Component facts come from the ComponentInfo view of TypeInfo; workspace facts
come from IDX0 contributions. Missing facts return NeedInputs or an explicit
unavailable/degraded cause. Cancellation, stale, unsupported, ambiguous and
partial results remain distinct from complete-empty lint. No partial or
degraded observation warms a complete-result cache or verification receipt.

One native/external authority is selected for each diagnostic concept and
exact profile. Aliases translate ids; they do not run a second rule. A
compiler-owned diagnostic is linked with its original owner and provenance,
not re-emitted by a native rule. Withdrawal of one profile/capability leaves
unaffected common and other-profile work intact. WASM truthfully reports an
external-process capability unavailable.

## Diagnostic identity, order and suppression

A diagnostic's semantic identity binds rule/profile/epoch, diagnostic concept,
configured project, source unit/region and semantic subject/occurrence.
Publication additionally binds exact source/content revision, map-chain
revision, catalog/configuration basis and dependency read set. This separates
stable tracking across edits from permission to publish on the current basis.
Localized messages, severity presentation, transport encoding and arrival
order are not semantic identity.

Primary and labeled secondary spans retain source identity and checked
authored UTF-8 geometry; transports convert through ENC owners. Diagnostics
carry native/external/compiler provenance, independent fact guarantees,
completeness/cause, a `help:` explanation, and a suppression reference when
applicable. Per-fact quality is not replaced by one optimistic result flag.

The canonical output order extends the carrier ordering law: source identity,
authored span, severity, profile-qualified rule/epoch, diagnostic code and typed
arguments, with occurrence identity for remaining distinct findings. Identical
findings coalesce only with matching concept, owner, profile and basis.
Messages and registry/hash/provider iteration never break ties. Related spans
and alternatives also have a declared canonical order; authored edit order
is preserved when semantically significant.

LK2 owns suppression parsing, geometry and consumption accounting. Categories
are `lint` and `assist`; formatter suppression stays with the formatting train.
Targets bind canonical rule identity and profile-qualified aliases, authored
range, reason and captured configuration provenance. Suppression changes
presentation, not the source fact. A directive is used only by a produced
matching diagnostic; inactive services cannot report it unused. Unavailable
geometry refuses suppression rather than guessing a 500-byte window.

## Alternatives and authored transactions

ACT1 separates applicability (`machine-applicable`, `maybe-incorrect`,
`has-placeholders`, `none` with reason) from behavioral safety. Diagnostic
fixes and diagnostic-free assists share labeled alternatives, help, secondary
spans, multi-file edits, placeholders and postconditions. A legacy `Safe`
flag alone grants no applicability. A placeholder action cannot be automatic.
Pure formatting is served by the formatter, never presented as a lint fix.

Every alternative binds its profile/rule epoch, exact source revisions,
content/map identities, configuration and semantic basis, edit content and
read sets. Only LSO8's authored transaction validator (or its explicitly
delegated equivalent) issues applicable edits. Irreversible maps, stale files,
conflicting ranges, invalid boundaries or unresolved target identity reject
the whole alternative, including all files. No string search repairs a lost
map. The apply receipt binds transaction identity and observed preconditions.

ACT2 alone grants machine applicability after isolated overlay verification:
complete affected-population enumeration; the original finding removed; no
new lint/checker/type diagnostics; all declared postconditions; idempotence.
Missing, cancelled, partial or inconclusive required checks leave the action
maybe-incorrect with the blocking check named. Verification leaves base
caches unchanged. A reused receipt revalidates its population/dependency
read sets and mints a new current-basis receipt; it never relabels the old one.

LNT3's initial packs retain their existing exact-basis non-overlap composition.
ACT2 owns later general conflict handling: replan the lower-precedence
alternative against the updated overlay or drop it with a reason; never
textually merge overlaps. Fix-all stops at a bounded fixpoint or returns
NonConvergent naming the cycling rules. Unsafe and external edits are never
automatically applied. Actions never trigger formatter side effects.

## Representative migration and pinned authored fixtures

The case product fixes exact source bytes, rule/provider identity, expected
authored ranges/replacement, clean twins and current test citations. Ranges
are UTF-8 half-open. Positive migrated diagnostics/edits preserve those bytes;
profile qualification, help, provenance and applicability enrich the envelope.
Existing safety labels do not certify ACT2 verification.

LNTV0 receives `require-component-is` / `AddComponentIs` and
`no-unsafe-url` / `RemoveUnsafeUrl`. The former intentionally has a placeholder;
the latter changes link behavior and remains a reviewable suggestion. Neither
is silently upgraded to machine-applicable. LNTS0 receives the neutral
`block-lang` diagnostic on a Svelte script and the static unsafe-URL body/fix
under its exact Svelte profile. The unsafe-URL body currently defaults to Vue:
there is no active Svelte-specific rule or fix to claim equivalent at this
head. Its selected Svelte row adopts the existing static-attribute behavior
with authored Svelte dirty/clean controls; dynamic URLs remain undiagnosed.
This is an explicit receiving implementation obligation, not a claim of live
Svelte support or an invented current diagnostic.

The existing unsafe-URL provider tests supply a quoted-value span, whereas
the rule reports `attr.span`. FV02/FS02 pin that provider input explicitly;
they do not certify current end-to-end span composition. Each receiving pack
must bind parser-owned attribute/value geometry and verify the integrated
diagnostic-to-edit route. Preserve existing rule and provider tests; do not
bless a wrong-range replacement as fixture equivalence.

The runtime release comes from exact captured profile selection, not from
this document or a dependency bump. Fixture packages retain the repository's
real pins; unsupported release cells stay explicit. The representative set
does not admit an ecosystem's entire rule population. LNT0 locks oracle cells;
later Vue/Svelte packs keep their own migrations. Only the named profile
rows/adapters are retired by LNTV0/LNTS0. Shared rule-registry and invocation
deletion belongs solely to LNT3, and shared action model migration to ACT1.

## Acceptance and proof selection

- **LRA0-AC1:** the product inventories cover each outcome, consumer and
  displaced route with an existing node, successor path and receiving
  acceptance. UAP0-AC-R1 must reject omissions, unknown/pathless owners,
  conflicting assignments and docs-only deletion owners. Imported central
  switches, untagged coordinates/identities and duplicate ComponentInfo
  authority retain CPF1/COX0, ENCF0/ENCL0 and TIF1/IDX0 ownership respectively.
- **LRA0-AC2:** existing carrier `diagnostic_ordering` and `parse_identity`
  tests discriminate typed arguments, message-independent ordering and exact
  parse inputs; protocol `typeinfo_proto_roundtrip` distinguishes unsupported
  from supported-empty and preserves origin hops. Session
  `g_block::framework_surface_executor` covers validation before dispatch,
  complete kind enumeration and degraded-result truth. These are current
  controls, not proof that profile/epoch registration is implemented.
  UAP0-AC-R2 receives zero-work, independent-epoch and stale/conflicting-edit
  cases; the pack owners receive exact authored fixture equivalence.
- **LRA0-AC3:** runtime incremental comparisons are not applicable to this
  docs-only change: no cache, publication, scheduler, cancellation or source
  authority changes. The case table carries incremental/edit-revert and
  degraded non-admission requirements for LNT2, LSO8 and ACT2 implementation.
- **LRA0-AC4:** runtime performance measurements are not applicable: zero
  production code, hot paths, counters, timers or retained candidates change.
  Applicability zero-work is a required work invariant for receiving owners;
  no new soak or benchmark is invented here.

Behavioral TDD is not applicable to contract data alone. Existing rule/fix
tests are cited as parity controls; missing successor proof is explicitly
assigned. Author checks validate configured documentation and formatting.
Independent workflow review and verification judge the published content;
this document contains no stored test transcript, count or historical object
pin. UAP0 reconciles drift against the implemented owners before locking
the contract. Production cutovers update their owning skills and documentation.
