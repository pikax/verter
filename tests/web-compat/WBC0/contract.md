# Compatibility target and data authority contract

This contract defines browser-compat evidence over captured project targets:
Browserslist/runtime target capture, the pinned Browser-Compat-Data (BCD) and
Baseline/web-features provenance, exact data versions, offline behaviour, and
the typed distinctions among unsupported, partial, flagged and unknown support.
It describes the repository at `feat(session): reserve module homes for the
lint, css, a11y, formatter, (#1059)`, 2026-11-01. The title/date locate
context; they are not acceptance evidence. This is contract text, inventory
and case tables only, under the [docs-only
rule](../../../docs/arch/kernel/README.md). No executable support, dataset
loader, target resolver, validator or test is delivered here.

The sole product owner is compatibility evidence and target predicates over
captured project targets. PM and CFG own project and config identity; WBC0
owns only the compatibility boundary over their captured facts. WBC1–WBC7
own the implementation below; ENV3 owns runtime-environment capability
profiles. Accepted predecessor completion comes from the controller's task
state, not source presence or a repository ledger.

## Intent and imported authority

A user asks "does my configured browser set support this feature I just
authored?" and gets a truthful, sourced answer: which targets support it,
which are partial, flagged or vendor-prefixed, which are unsupported, and
which the data cannot answer. Missing facts stay missing. A Baseline badge, a
displayed type, an installed upstream package or a passing compile never
establishes support for a configured target.

This contract consumes the following accepted authorities and re-owns none of them:

- **CFG0** ([configuration](../../../docs/arch/kernel/configuration.md)):
  the `verter.config.jsonc` envelope, its precedence, and captured ecosystem
  configuration (`CR19`–`CR23`). Target capture is a captured-configuration
  concern; WBC0 adds no second config reader, no ambient global read
  (`CR22`) and no execution of executable configs (`CR20`).
- **VID0** ([identities](../../../docs/arch/kernel/identities.md)): the
  exact-release law and identity orthogonality. A dataset release and a
  browser-target identity obey `R04`–`R08` semantics in their own axis:
  exact versions, no floating tags, no implicit defaults, no coercion.
- **WDX0** ([evidence-class
  policy](../../web-product/WDX0/products/evidence-class-policy.v1.json)):
  static proof, runtime observation and estimates stay distinct. Everything
  in this train is static analysis over pinned bytes: it may claim structural
  facts and data-derived answers, never executed browser behaviour. A
  support answer derived from BCD is a data-backed static prediction, not a
  runtime observation, and is labelled as such.

- **WDX0 ownership:** the [disposition matrix](../../web-product/WDX0/products/web-product-disposition-matrix.v1.json)
  and [ownership map](../../web-product/WDX0/products/web-product-ownership-map.json)
  assign this constitution to expansion.web-compat and qualification to WBC7.
- **DX0 delivery:** [FeatureExposureContract](../../product-experience/DX0/products/feature-exposure-contract.v1.json),
  [HostExecutionClass](../../product-experience/DX0/products/host-execution-class.v1.json)
  and [ProductReceiptBasis](../../product-experience/DX0/products/product-receipt-basis.v1.json)
  govern operation exposure, host claims and source/config/session-bound receipts.

The WBC0-specific authority amendment, WBC0-COMPANION-AUTHORITY, replaces
the unavailable `contracts/web-product-expansion-v1.md` with the frozen
requirements and the named WDX0, CFG0, VID0 and DX0 contracts above. Existing
Tama authority, review, resource and successor-charter contracts remain in
force. This does not claim equivalence to unavailable text or generalize the
CWB0-specific precedent. Readiness is database-owned; no repository ledger
or roadmap validator is recreated. No prerequisite is added; implementation
and qualification remain with WBC1–WBC7. The reserved module homes under
`crates/verter_session/src/compat/`
(`data`, `targets`, `html_css`, `js_web_api`, `explain`, each documented
`Filled by WBC1`–`WBC5`) are the intended implementation boundary; nothing
here moves them.

## Project target capture and query

A **project target query** resolves, for one demand, the effective set of
runtime targets a project addresses, with the reason each target and each
contributing configuration survived. WBC2 implements it; the laws are fixed
here.

- **TR1 — captured sources only.** Targets come from (a) a `compat` product
  section in `verter.config.jsonc` (a new catalog row under `CR03`; its
  schema owner is WBC2, registration is a CAT0 row, not an envelope change),
  and (b) captured ecosystem configuration (`CR19`–`CR21`): the
  `browserslist` key or `"browserslist".extends` shareable config in a
  captured `package.json` (PM1 payload), a `.browserslistrc` data file, a
  `browserslist.config.js` executable config (static literal extraction only
  per `CR19`: all-literal extraction is `Complete`; mixed literal and
  non-literal forms retain literal facts as `Partial`, naming unresolved
  forms; no extractable literal form is `Uncaptured { reason }`. `CR20`
  requires authorization for dynamic evaluation into a separate snapshot;
  denied execution never erases a static partial snapshot), captured tsconfig
  `target`/`lib` (PM1), and captured build-tool targets (SM1 for Vite). Nothing reads
  `$HOME`, ambient `BROWSERSLIST`/`BROWSERSLIST_ENV` variables or any global
  config; the active browserslist environment (`production`/`development`
  sections) is an explicit demand input, carried beside the query identity
  like `CR24`'s environment binding.
- **TR2 — deterministic resolution with reasons.** Expansion of Browserslist
  queries (`defaults`, `last 2 versions`, `not dead`, `> 0.25%`, …) is an
  offline, deterministic computation over the pinned snapshot's browser
  release and population data. WBC1 pins the admitted query vocabulary; a
  query outside it, or needing a statistic (usage share, region) the pinned
  snapshot does not carry, answers a typed unsupported/unknown with the
  reason. It never fetches, never guesses a percentage and never silently
  narrows to a subset it can answer. Shareable `browserslist-config-*`
  packages resolve through the project model's module resolution; unresolved
  is a typed failure, not a fallback to `defaults`.
- **TR3 — typed target identity.** A resolved target is a typed identity:
  browser family plus exact version (and platform/edition where the dataset
  distinguishes one). A range, a floating tag or an unresolved spelling is
  never a target identity (VID0 `R07`). Runtime environments — Window,
  dedicated/shared/service workers, Node — are separately identified
  contexts (ENV3), never folded into a browser-engine target.
- **TR4 — precedence and provenance.** Explicit `compat` section
  contributions override captured ecosystem config; within captured config,
  CFG0 precedence (`CR05`–`CR08`) applies unchanged. Every resolved set
  records its read set and each contribution's source unit, revision and
  rank. Unresolvable or contradictory input answers `Invalid`, `NeedInputs`
  or `Uncaptured` per the CFG0 outcome vocabulary, never an empty success
  and never an implicit default target set (VID0 `R06`). A partial captured
  input retains its known contributions and unresolved forms; it cannot
  establish a complete target population or a complete compatibility pass.
- **TR5 — identity and invalidation.** The effective-target query key covers
  the consulted configuration identities and revisions (`CR14`), the pinned
  snapshot identity (below) and the declared environment. Distinct source
  revisions never alias, even after reverting to identical bytes. Separately,
  section-local value fingerprints and span anchors follow `CR16`: unrelated
  section edits preserve target values and their fingerprints; edit/revert
  restores the value and span digests. Reused values must re-stamp current
  `SourceRevision`/`ContentId` provenance, and re-anchor moved spans, without
  recomputing target values. No old revision-bearing result is republished.

## Pinned compatibility data snapshot

- **DR1 — named upstream sources.** The snapshot ingests MDN
  browser-compat-data (per-feature, per-browser support records with
  versions, flags, prefixes, partial implementations and alternative names;
  plus its browser release data) and the Baseline/web-features dataset
  (feature groupings and Baseline statuses). If usage/population statistics
  beyond BCD's release data are admitted for `TR2`, they are a separately
  identified dataset in the same snapshot, never an unnamed borrow from
  toolchain transitive dependencies. The exact admitted releases, artifacts,
  integrity pins and licenses are fixed in the inventory below. WBC1 owns
  ingestion and reproducible generation from those inputs, not selection of
  unspecified releases. No usage/population dataset is admitted initially;
  queries requiring it retain the typed unsupported/unknown outcome in TR2.
- **DR2 — exact data versions.** A snapshot identity is the ordered set of
  its constituent dataset identities — name, exact upstream release and
  content digest — plus the generation recipe identity. No floating
  `latest`, no implied default dataset. Two snapshot versions never share
  cache keys or answers (VID0 `R02` orthogonality: a dataset upgrade changes
  snapshot-derived identities and nothing upstream of them). The exact data
  versions are product-visible facts (WBC7 surfaces them; see AC4).
- **DR3 — offline session, explicit generation.** Generation runs only as
  explicit tooling under `scripts/web-compat`, fetching only from official
  published artifacts, recording source URL, digest and upstream license
  with attribution, and reproducing byte-identical output for identical
  inputs. The session, every semantic query and every host without a
  filesystem perform no network I/O: they read pinned bytes supplied as
  prepared inputs (`CR23`), and a snapshot neither supplied nor proven
  absent is `NeedInputs`, never an empty success. There is no hidden online
  lookup anywhere in the query path.
- **DR4 — schema-validated, condition-preserving.** Ingestion validates each
  record against the dataset's published schema; a schema mismatch or
  missing record is a typed rejection or unknown, never a guessed support
  value (WBC1-AC2). Flags, vendor prefixes, `alternative_name`, partial
  implementations, `version_added: true` (supported, version unknown) and
  `version_removed` all survive ingestion as distinct facts; none is
  flattened into a boolean.
- **DR5 — one support authority per fact.** BCD is the per-browser support
  authority; web-features is the feature-grouping and Baseline authority.
  Where a future admitted dataset overlaps, deterministic precedence is
  pinned at ingestion (WBC1) and a residual disagreement is reported as a
  typed disagreement, never silently resolved.

## Support evidence model

Per (feature identity × target identity), the answer retains the applicable
support statements and their conditions. The vocabulary is:

| Support fact       | Meaning and data basis                                                                                          |
| ------------------ | --------------------------------------------------------------------------------------------------------------- |
| `supported`        | An applicable support record places the target at or after addition and before removal; carries `since` when known. An unknown addition version retains uncertainty rather than proving support for every historical version |
| `partial`          | Implemented with named limits (`partial_implementation` and its notes)                                          |
| `flagged`          | Available only behind a runtime flag; preserves BCD `type`, `name`, and optional `value_to_set`; default-enabled state is unknown, not supplied by BCD      |
| `prefixed`/`aliased` | Available under a vendor prefix or alternative name; the exact spelling is the fact, not general support       |
| `unsupported`      | An explicit negative record, or `version_added` after / `version_removed` at or before the target version             |
| `unknown`          | No decidable record: feature absent from the dataset, target family/version absent from the dataset, or the needed statistic is not carried |

Removal is an exclusive upper bound on a support interval: added at 10,
removed at 20 supports an otherwise unqualified record at 19, but not 20 or
21. Evaluate each statement's interval before its conditions; preserve all
conditions and alternative statements rather than dropping a flag when a
record is also partial or prefixed. An indeterminate boundary cannot prove
unconditional support at a particular target. The [admitted BCD schema](https://raw.githubusercontent.com/mdn/browser-compat-data/v5.7.6/schemas/compat-data.schema.json)
defines these version and flag fields.

- **SR1 — unknown never coerces.** `unknown` is never reported as supported
  or unsupported, never hidden behind an empty list, and never answered by
  falling back to Baseline status, a newer data version or another browser's
  record. Negative, partial, conditional and unsupported results are
  truthful states, not routes for hiding unfinished scope.
- **SR2 — Baseline is provenance, not a predicate.** A web-features
  Baseline status (`newly`/`limited`/`widely` available, with dates) is
  feature-level summary provenance carried beside the answer. It is never a
  support predicate over a configured target set: a widely-available feature
  with an unsupported, flagged or partial record on one configured target
  answers for that target from its record. Equating Baseline status with
  project compatibility is a forbidden design; the discriminating case is
  bound in the [case table](../evidence/WBC0/cases.md) and owned by
  WBC1-AC2 and WBC6-AC2.
- **SR3 — evidence provenance.** Every support answer carries the snapshot
  identity and the dataset record(s) it came from, so a later dataset
  upgrade can explain every changed answer. Aggregated answers ("supported
  in all configured targets") are derived views that state their
  per-target population; an aggregation over any `unknown` member reports
  the unknown, not a clean pass.
- **SR4 — recognition consumes existing owners.** Feature recognition
  (WBC3/WBC4) attaches exact ranges through the existing syntax, style and
  semantic owners; there is no regex or text-match recovery for feature
  identity, no whole-context helper and no second semantic authority.
  Ambiguous or dynamic access (WBC4) stays explicitly ambiguous; guarded
  usage is modelled as conditional evidence, not resolved by guessing the
  guard.

## Required capability matrix

Every row is required downstream work, even though its current producer is
unimplemented. `unsupported` is an observable answer for a specific
feature/target, not completion of a row.

| Capability                        | Existing source/authority and limits                                                                                     | Receiving owner and acceptance                |
| --------------------------------- | ------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------- |
| Pinned data ingestion/generation  | Reserved `compat/data` home (`Filled by WBC1`); no loader, corpus or script exists at this head                            | WBC1-AC1/AC2/AC3 (`scripts/web-compat`, `test-corpora/web-compat`) |
| Dataset identity, licensing       | Exact inputs and licenses below; WBC1 implements licensed ingestion and publishes snapshot identity                          | WBC1-AC1/AC4                                  |
| Project target query              | Reserved `compat/targets` home; `compat` section is a new catalog row; captured inputs belong to PM1/SM1 under CFG0         | WBC2-AC1…; PM/CFG retain capture and identity  |
| Runtime environment profiles      | Reserved `realms/profiles/browser`; Window/worker distinctions are ENV3's, not target-set members                           | ENV3                                          |
| HTML/CSS feature recognition      | Existing CSS/HTML syntax and style owners (prefix tolerance exists at the parse layer only); reserved `compat/html_css`    | WBC3-AC1…                                     |
| JS/Web API recognition            | Existing TS/JS semantic owners and realms; reserved `compat/js_web_api`                                                    | WBC4-AC1…                                     |
| Explanations and actions          | Reserved `compat/explain`, `verter_protocol/src/compat`, `verter_lsp/src/features/compat`; no speculative polyfill or rewrite | WBC5-AC1…                                     |
| Accuracy/update qualification     | None; cold/warm equality over dataset upgrades, target edits, flags/partial cases, offline determinism                     | WBC6-AC1/AC2 (`verter_session/tests/cases/web_compat`) |
| Public delivery                   | None; `examples/reference/guides/compatibility.md` and `recipes/compatibility.json` are WBC7's artifacts                   | WBC7-AC1…; DOC1 retains executable examples   |

## Actual consumer inventory and cutover

Verified at this head by repository search.

| Current route                                                                | Evidence and present behaviour                                                                                                        | Exactly one surviving owner; disposition                                                                                   |
| ---------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------- |
| `crates/verter_session/src/compat/{mod,data,targets,html_css,js_web_api,explain}/mod.rs` | Reserved module homes, each documented `Filled by WBC1`…`WBC5`; no code                                                                 | Each WBC node fills its own home (MSK1 declaration); no cutover, no second compat surface outside them                      |
| `pnpm-lock.yaml` transitive entries: `@mdn/browser-compat-data@5.7.6`, `browserslist@4.28.1`/`4.29.4`, `caniuse-api@3.0.0`, `caniuse-lite@1.0.30001774`/`30001777`/`30001819`, `update-browserslist-db` | Pulled only through dev tooling of `packages/unplugin` (`@farmfe/core@1.7.11` → `farm-browserslist-generator@1.0.5`; `@babel/helper-compilation-targets`); no workspace package depends on them directly; no Verter code reads them | Not a data authority: unowned, floating toolchain state. WBC1 pins its own explicit ingestion and must not resolve data through these copies. Lockfile integrity pins keep their operational purpose; no change here |
| `crates/verter_css_syntax/src/parser.rs` (`keyframes`, `-webkit-keyframes`)  | Vendor-prefixed at-rule acceptance at the syntax layer                                                                                  | Stays syntax-layer tolerance; it proves parse acceptance, never support. WBC3 attaches support evidence through owners       |
| `crates/verter_mcp/src/baseline.rs`                                          | Diagnostics-only baseline (known-issue filtering); shares the spelling "baseline" with web Baseline, nothing else                       | Unrelated; retained by its owner. Product copy must disambiguate the two "baseline" senses (WBC5/WBC7 wording obligation)    |
| `crates/verter_svelte_conformance/src/model.rs::CompileTarget`               | Client/server compile-backend axis in the Svelte conformance model                                                                     | Naming constraint on WBC2: browser-target identity must not overload this public name                                        |
| tsconfig / `package.json` capture (`CF-D06`, `CR21`)                         | PM1 owns captured tsconfig/package.json lineages; root `tsconfig.base.json` `"target": "es6"` is Verter's own toolchain config         | PM1 retains capture; WBC2 consumes captured facts. PM/CFG own project and config identity                                    |

Deletion population **in this decision: empty**. No compatibility query
surface, dataset reader or target resolver exists at this head (no
`browserslist` key in any workspace `package.json`, no `.browserslistrc`, no
`browserslist.config.*`, no web-features dependency, no compat dependency in
any `packages/*/package.json`), so the boundary is genuinely additive; no
route is displaced and none is invented to delete.

## Exact versions and source evidence

### Admitted inputs

These are intentional initial data pins, not claims of current browser
coverage. BCD 5.7.6 supplies the reviewed version/flag schema, including
unknown-version support; web-features 3.44.0 supplies grouping and Baseline
provenance only. Their release dates need not coincide: a feature mapping
missing from this BCD remains unknown, never supplemented from Baseline.
The older BCD release is not selected through the toolchain dependency that
happens to share its version. A later upgrade is an explicit data change
subject to WBC6 qualification, never a floating substitution.

| Dataset | Exact release and official artifact | Content identity (SHA-512 SRI of compressed artifact) | Payload and license |
| --- | --- | --- | --- |
| MDN browser-compat-data | `@mdn/browser-compat-data@5.7.6`; [npm artifact](https://registry.npmjs.org/@mdn/browser-compat-data/-/browser-compat-data-5.7.6.tgz) | `sha512-7xdrMX0Wk7grrTZQwAoy1GkvPMFoizStUoL+VmtUkAxegbCCec+3FKwOM6yc/uGU5+BEczQHXAlWiqvM8JeENg==` | `package/data.json`, browser releases and support records; `package/LICENSE`, CC0-1.0; upstream MDN browser-compat-data |
| Web Platform DX web-features | `web-features@3.44.0`; [npm artifact](https://registry.npmjs.org/web-features/-/web-features-3.44.0.tgz) | `sha512-K/+YJI7uVE/b4G8SU67/F/4j81/ajJ6MfBCZyuQaqyuPlVXF8cd6dLqBznXIWJI9nwnvuY2dapKK08OHzTjYww==` | `package/data.json`, validated against `package/data.schema.json`; `package/LICENSE.txt`, Apache-2.0; upstream Web Platform DX |

WBC1 retains these upstream names and license texts with generated output,
identifies modifications, and preserves applicable attribution and notices.
It verifies artifact integrity before ingestion. Its recipe identity covers
the generator and schema inputs; the BCD schema is the release-tagged schema
linked above, pinned as `sha256:e5faa5c194391a0cc2cbf6e9e78bf2e2de10cc5762337c06ff5c7da13d9de97d` (raw schema bytes). This contract
admits no caniuse, usage-share or regional-statistics authority. Snapshot
packaging, generated bytes and executable conformance remain WBC1 work.

### Existing candidate inventory

These are repository inventory facts, not supported-version declarations.

| Source                       | Candidate identity                                                                                                                              | Meaning and receiving obligation                                                                                                   |
| ---------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------ | ----------------------------------------------------------------------------------------------------------------------------------- |
| `pnpm-lock.yaml`             | BCD `5.7.6`; browserslist `4.28.1`/`4.29.4`; caniuse-lite `1.0.30001774`/`30001777`/`30001819` (all transitive, toolchain-only)                   | No support or data-version claim; admitted inputs above are independent of these toolchain copies                                  |
| `packages/*/package.json`    | No direct browserslist/BCD/web-features/caniuse dependency in any workspace package                                                               | There is no installed data authority to inherit; WBC1 adds the explicit pin or vendored corpus it admits                            |
| `crates/verter_session/src/compat/` | Six reserved module homes with fill-owners WBC1–WBC5                                                                                              | The implementation boundary; contract-to-code drift is a failing review case, not a silent pass                                     |
| Root `tsconfig.base.json`    | `"target": "es6"` (Verter's own toolchain)                                                                                                        | Not a product target fact; user-project targets come only from captured configuration                                              |

## Public delivery and acceptance

Producers' obligations (specified here, delivered by WBC1–WBC7): each
operation registers through the existing VIM/DX exposure contract with exact
operation, producer, host execution class and replay case; public
compatibility surfaces state the exact dataset versions behind every answer,
the permissions model (static read-only; no execution, no network), the
uncertainty vocabulary above and migration notes at any dataset or schema
change. WBC5's diagnostics and target-change plans cite per-target evidence
and never insert polyfills or rewrite behaviour speculatively. WBC7 checks
completed artifacts — the guide, recipe, protocol and LSP surfaces with
DOC1-tested examples — not templates or screenshots.

| Acceptance | Evidence in this decision                                                                                                                                                             |
| ---------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| WBC0-AC1   | Sole-owner statement, full capability matrix and verified consumer inventory with one surviving owner per route. The executable import/visibility boundary belongs to WBC1's `compat::data` surface. |
| WBC0-AC2   | `SR1`/`SR2` fix the Baseline-versus-target and missing-data laws; the discriminating case is specified in the case table and owned by WBC1-AC2 (data level) and WBC6-AC2 (fixture level). No duplicate test here. |
| WBC0-AC3   | No state, query, map or scheduling code changes, so fresh/incremental, edit/revert, cancellation and ordering execution are not applicable here. `TR5`, `DR2` and `DR3` bind the later proofs on WBC1/WBC2; WBC6 owns dataset-upgrade cold/warm equality. |
| WBC0-AC4   | Dataset-version visibility, permissions, uncertainty vocabulary, exposure registration and DOC1-tested examples are specified above for each producer; WBC7 verifies actual artifacts. This decision makes no product support claim. |
| WBC0-AC5   | No hot path, runtime or allocation behaviour changes and no new budget or measurement. The additive deletion set is explicit; later cost comparisons follow the standing measurement rule.        |

Later cost comparisons follow the standing measurement rule: equivalent
answers and work counts/growth against the baseline, measured on the same
assigned worker/session when compared. Timings, allocation and RSS are
evidence, not machine-specific acceptance thresholds; unavailable metrics are
reported as unavailable.
