# WBC0 acceptance cases

This is a review case table for the [contract](../../WBC0/contract.md), not a
new test suite, receipt or claim that downstream features execute today. The
compatibility domain is greenfield at this head: the existing-evidence rows
below are repository facts that bound what may be claimed, and the receiving
rows specify discriminating cases the named implementation nodes must make
executable. No test is added or duplicated here.

## Existing evidence and its limits

| Current source                                                                                             | Observed fact at this head                                                                                                                     | What it does not establish                                                                                       |
| ---------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------ |
| `crates/verter_session/src/compat/{mod,data,targets,html_css,js_web_api,explain}/mod.rs`                   | Six reserved module homes, each documented `Filled by WBC1`…`WBC5`; no implementation                                                            | Any loader, resolver, recognizer or answer surface; readiness is the controller's task state, not the declaration  |
| `pnpm-lock.yaml` — `@mdn/browser-compat-data@5.7.6`, `browserslist@4.28.1`/`4.29.4`, `caniuse-api@3.0.0`, `caniuse-lite@1.0.30001774`/`30001777`/`30001819` | Transitive entries of `packages/unplugin` dev tooling only (`@farmfe/core@1.7.11` → `farm-browserslist-generator@1.0.5`; `@babel/helper-compilation-targets`); no workspace package depends on them directly | A pinned data authority, a licensed snapshot, or any data version this train may claim (contract `DR1`)          |
| `crates/verter_css_syntax/src/parser.rs` — `keyframes`, `-webkit-keyframes`                                 | The syntax layer accepts vendor-prefixed at-rule spellings                                                                                      | Support semantics for any prefix or target; recognition must come from owners (contract `SR4`)                    |
| `crates/verter_mcp/src/baseline.rs`                                                                        | A diagnostics-only baseline store (known-issue filtering)                                                                                       | Anything about web Baseline; product copy must disambiguate the two senses                                        |
| `crates/verter_svelte_conformance/src/model.rs::CompileTarget`                                             | A client/server compile-backend axis                                                                                                            | A browser-target identity; WBC2 must not overload the public name                                                 |
| Workspace configuration search                                                                             | No `browserslist` key in any `package.json`, no `.browserslistrc`, no `browserslist.config.*`, no web-features/caniuse/BCD dependency, no compat section in docs | Any current capture or query behaviour to preserve or migrate; the deletion population is empty                   |
| `docs/arch/kernel/configuration.md` (`CR19`–`CR23`), `identities.md` (`R04`–`R08`), WDX0 evidence-class policy | Accepted predecessor contracts this train consumes                                                                                              | Their implementation is not this train's to claim; capture and identity stay PM/CFG's                              |

## Receiving executable cases

Each case must exercise the owned public operation with its exact
source/project/config basis and the pinned snapshot identity, with
independent healthy controls. Negative outputs retain typed statuses and
reasons rather than appearing as empty success. Rows specify required
behaviour; they are not new check commands.

| Case                       | Input/counterexample                                                                                                                                                       | Required result and receiving acceptance                                                                                                                                                                                        |
| -------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Baseline is not support    | A feature whose web-features Baseline status is widely available, configured target set includes a browser/version where BCD records it unsupported, flagged or partial      | The per-target answer comes from the record: that target answers unsupported/flagged/partial; no surface answers "supported everywhere" from the badge. WBC1-AC2 (data level); WBC2-AC1 (target set); WBC6-AC2 (pinned fixture replay). |
| Missing data stays unknown | Feature absent from the pinned snapshot; target version newer than the dataset; schema-mismatched record                                                                     | `unknown` with reason, never supported and never unsupported; the failure names the absent feature/target or the schema mismatch. WBC1-AC2.                                                                                       |
| Condition preservation     | Records with `flags[]`, `prefix`, `alternative_name`, `partial_implementation`, `version_added: true`, `version_removed`                                                      | Each survives ingestion as a distinct typed fact; none collapses into a boolean or a bare version. WBC1-AC1/AC2.                                                                                                                  |
| Removal boundary | One otherwise unqualified record added at 10 and removed at 20, targets 19, 20, 21 | Supported at 19; unsupported at 20 and 21. An alternative valid statement is evaluated separately. WBC1-AC1/AC2; WBC6-AC2. |
| Flag provenance | A schema-valid flag with type/name and optional value_to_set, no default-enabled field | All supplied fields survive; default-enabled state remains unknown. WBC1-AC1/AC2. |
| Admitted source inventory | The exact BCD and web-features artifacts in the contract; changed artifact bytes; feature grouping with no BCD mapping | Verify integrity and licenses before generation; reject changed bytes. Missing BCD mapping stays unknown regardless of Baseline. WBC1-AC1/AC2/AC3; WBC6-AC2. |
| Offline determinism        | Same snapshot and inputs queried twice; snapshot absent on a prepared-input host; a `> 0.25%`-style query whose statistic the snapshot does not carry                          | Byte-identical answers; `NeedInputs` (not empty success); typed unsupported/unknown with the reason — no fetch, no guessed statistic. WBC1-AC1/AC3; WBC6-AC1.                                                                       |
| Target capture precedence  | `compat` section override versus captured `.browserslistrc`/package.json `browserslist`/tsconfig; `browserslist.config.js` with mixed literal target facts and non-literal forms; shareable config unresolvable | Explicit config wins with reasons; all-literal static capture is `Complete`, mixed literal/dynamic retains literals as `Partial` with unresolved forms, and no extractable literals is `Uncaptured`. Unauthorized execution cannot erase partial capture; unresolvable extends is a typed failure, never `defaults`. WBC2-AC1.                                                   |
| Typed target identity      | A range or floating spelling in captured config; two contexts (Window and a worker) over one engine                                                                          | No range becomes a target identity (VID0 `R07`); contexts stay separately identified (ENV3), never folded into one target. WBC2-AC1; ENV3.                                                                                          |
| Recognition through owners | Authored feature with exact range; dynamic/ambiguous access; guarded usage                                                                                                   | Ranges and identities come from existing syntax/semantic owners, never regex or text matching; ambiguity stays typed; guards are conditional evidence. WBC3-AC1; WBC4-AC1.                                                          |
| Upgrade equality           | Dataset upgrade (new snapshot identity), target edit/revert and unrelated-section edit/revert, each compared cold versus warm                                                                     | New snapshot identity changes snapshot-derived answers only; incremental equals fresh. An unrelated-section edit/revert preserves target values and value fingerprints but advances query identity and returned source provenance; moved spans re-anchor, and reverted digests never restore an old revision. WBC1-AC3; WBC6-AC1/AC2.                                                                                         |
| Truthful actions           | A diagnostic offering a fix on an `unknown` or `flagged` target; a plan to add a polyfill                                                                                    | Actions cite per-target evidence and stay within verified alternatives; no speculative polyfill or behaviour rewrite. WBC5-AC1.                                                                                                    |
| Freshness claim            | A public surface answering without naming the pinned data versions; a support answer presented as executed browser behaviour                                                  | Every answer names its dataset versions; data-derived static predictions are never labelled runtime observation (WDX0 classes). WBC7-AC2/AC4; DOC1 for tested examples.                                                            |

## Verification applicability

WBC0 changes the contract, this case table and their exact-path entries in
the existing CI-inert registry. No query state, dataset,
cache, runtime, public DTO or code behaviour is changed. The contract now
selects exact upstream data artifacts; it does not install those dependencies.
Consequently there is no executable TDD delta or freshness validator to run,
and no runtime/incremental/performance result claimed. Repository cheap
format/whitespace checks apply to the authored documents; actual commands,
platform, candidate identity and outcomes belong to this turn's report, not
a committed transcript. The workflow runs the canonical gate on the published
head. Exact data releases and source artifacts are selected in this
contract; ingestion, generated corpora and public examples remain WBC1 and
WBC7 obligations, not placeholders accepted as finished products here.
