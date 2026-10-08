# Projection availability repair ratification

Date: 2026-10-08
Verdict: RESCOPE

## Context

The projection classifier discards its availability arm at production delivery. The partial candidate only withholds aborts inferred from accumulated reason classes and exempts flow returns. The existing flow preparation path also uses UNSTABLE_STATE when preparation refuses but an independent evaluation produces a usable value. Direct function-return and audited flow consumers do not all pass through flow_return_fact. Thus neither blanket withholding nor the exemption satisfies the required distinction.

The operator rulings skr-facts-1-value-withholding-scope and skr-facts-1-flow-unstable-state-abort require bounded usable approximations, adopted-dependency quality, and a new abort/proof-refusal prerequisite. Existing SKR-READS and SKR-FACTS-2 own broader envelope and final-consumer migrations; neither supplies this prerequisite before the repair.

## Intent contract

### TC-REV11-SEMANTIC-CLOSURE-8P1

#### Context and intent contract
Repair R1, production projection results discarding typed availability. Apply the operator's 2026-10-08 rulings skr-facts-1-value-withholding-scope and skr-facts-1-flow-unstable-state-abort. Consumers must receive the producer's Complete, Approximate or Unavailable fact, or an execution abort. Blanket withholding of every unfinished value is superseded by documented, bounded producer interpretations. This is still the original repair task; ratification does not establish implementation acceptance.

#### Stand-down disposition
This ruling removes the candidate-only abort helper, FlowReturn exemption, associated reason-injection knob and test. The remaining repair is intentionally not implemented before its prerequisite. This cleanup is not evidence that R1 is fixed.

#### Prerequisite and ownership
Implementation resumes only after SKR-ABORT-1 lands: its A1-A4 deliver the separate abort channel, evidence-based flow refusal classification and enforced dispatch/public-flow boundary. Keep the existing TC-REV11-SEMANTIC-CLOSURE-8 predecessor and add SKR-ABORT-1. Discard the archived partial candidate when restarting from main; its overloaded-reason abort helper, FlowReturn exemption and reason-injection test are not a foundation to carry forward.

This task retains projection availability adoption, fact-derived summaries, affected consumer integration and correction of discarded-read contamination. SKR-ABORT-1 exclusively supplies the missing abort/proof-refusal distinction and its flow propagation. SKR-READS retains its general delivery-envelope, receipt and cost work; SKR-FACTS-2 retains broader component/final-result consumer and reuse migration. Their existence does not defer any affected projection path or exactness defect needed for this repair. SKR-LIMITS keeps waiting on this repair alone; no downstream task is recreated.

#### Changes and scope
Use the shared type engine: semantic_query/fact_result.rs, project_semantic_dispatch/{projection_fact,mod,walk,flow_return_fact,conditional_decision}.rs, semantic_query_memo/producer.rs and the affected dispatch/session consumers and tests. Follow actual module layout when paths have moved. Update owning producer-guarantee documentation. Preserve one authority for each demanded fact and its adopted dependencies.

#### Acceptance
- R1.1: Production dispatch adopts and carries the fact through delivery and affected consumers. Complete carries the exact demanded answer, including exact empty/open domains and checker-deferred conditionals. Unavailable carries no usable value. Compatibility partial/reason summaries derive from the adopted fact; observing only is_err(), or retaining an independent boolean quality authority, fails acceptance. Prove through actual dispatch, including cold delivery, a retained complete repeat and singleflight followers.
- R1.2: Approximate values are offered only with a documented guarantee established by that producer's typed outcome and adopted evidence: proven member/key presence subsets, positional typed gaps preserving independently exact positions, or the pinned checker's explicit recovery. Neither a reason-code allowlist nor finding a marker somewhere in a value establishes this guarantee. Unfinished values without one are Unavailable. Discriminating dispatch controls must separate usable gaps/recoveries from an unsupported unfinished value with similar causes, and preserve TS2589/TS2590 recovery and member/sibling parity assertions.
- R1.3: Correct affected discarded-read contamination here. A declined failed probe cannot taint an independently exact answer; an adopted incomplete dependency must degrade the dependent fact. Test both directions, including the affected ReturnType/deep-input and closed-Pick paths and exact sibling behavior. Relabeling independently exact answers Approximate fails.
- R1.4: Conditional branch selection, runtime classification and other proof consumers remain exact-only. Presence does not prove member values/modifiers or absence. Tests must reject approximate evidence at these boundaries while preserving independently exact controls and existing macro diagnostics.
- R1.5: Consume SKR-ABORT-1's typed abort disposition without a FlowReturn exemption. Cancellation and genuinely stale/torn execution return no value through dispatch, enclosing projections or public flow consumers and admit no candidate. A usable proof-refused flow value remains explicitly Approximate and non-admitted. Preserve existing legitimate refusal-retention semantics without promoting refusals or partial wrappers to Complete.
- R1.6: Extend actual-boundary tests with a demonstrated planted bypass that fails (availability discarded, abort value forwarded, or discarded-probe taint restored, as appropriate to each changed boundary). Document each producer guarantee and the prerequisite interface. Preserve semantic assertions rather than rewriting them to bless regressions. Update architecture import/cutover records when the test/consumer population changes.
- R1.7: Run cargo fmt --all -- --check; cargo clippy --workspace --all-targets -- -D warnings; focused producer/dispatch/consumer tests; node scripts/compile-contracts.mjs for the affected public/typed boundaries; and node scripts/gate.mjs with all required receipts. Run affected architecture-health verifiers and their behavioral controls. Failed or unavailable required local checks remain unresolved.

#### Legacy deletions
Remove the production classifier-only check that discards availability, competing quality setters on affected paths, discarded-probe taint propagation, blanket ValueRequired withholding without interpretation, and any abort bypass/exemption remaining after prerequisite integration. Do not add a second resolver, a reason-based availability policy or a compatibility branch preserving the defective route.

#### Evidence and measurement
Use current code, behavioral tests and compile/dependency contracts as proof. Do not use commit hashes, commit URLs, historical Git objects, ancestry, fixed test counts or stored transcripts as durable acceptance evidence. Runtime candidate/base identities and genuine dependency pins remain operational inputs. Preserve the operator's 2026-10-04 measurement rule: equivalent answer classes and deterministic work counts/growth ratios govern acceptance; any requested comparison is interleaved on the same assigned worker. Time, allocation and memory measurements are advisory here; unavailable measurements are reported and do not block. SKR-PERF-0, SKR-LIMITS and L2 retain production time/memory calibration; L1 retains the soak. No benchmark or soak is introduced by this ruling.


## Changes

Add SKR-ABORT-1 after SKR-FACTS-1 and make TC-REV11-SEMANTIC-CLOSURE-8P1 wait for it, retaining its existing conformance predecessor. Existing downstream tasks, beginning with SKR-LIMITS, continue waiting on this repair. Restart the repair from main after the prerequisite; discard the archived partial candidate. This preserves the current task identity and does not approve its original finding.

### SKR-ABORT-1

#### Context and intent contract
Separate execution aborts from flow proof refusals before projection facts become delivery authority. The operator's 2026-10-08 skr-facts-1-flow-unstable-state-abort ruling identifies UNSTABLE_STATE serving incompatible roles: genuinely invalid execution and refused preparation/proof beside an independently valid evaluated value. A blanket abort or blanket usability classification is unsound.

#### Inputs, scope and ownership
Predecessor SKR-FACTS-1 supplies FactResult and ExecutionAbort (AC1); use their landed vocabulary and existing flow evaluator/proof machinery. No unowned substrate or future envelope rewrite is a prerequisite. This task delivers the missing distinction to TC-REV11-SEMANTIC-CLOSURE-8P1.
Own the minimal typed abort transport and cause classification in semantic_query, request/build observation and semantic_query_memo boundaries, plus project_semantic_dispatch/{flow_return,flow_return_fact,flow_solve,flow_slice_driver,mod}.rs and affected direct flow consumers, including execute_function_return_source and verter_session/src/host_flow_return_audit.rs. Update related tests, consumer/import records and owning documentation. General projection interpretation, projection summary cutover and non-flow discarded-read repairs remain TC-REV11-SEMANTIC-CLOSURE-8P1; general receipt/envelope/reuse migration stays with SKR-READS/SKR-FACTS-2.

#### Acceptance
- A1: Provide a typed execution-abort disposition separate from semantic incompleteness/proof refusal, carried from the originating observation to dispatch settlement and flow consumption. Cancellation, shutdown and genuine superseded/torn value evidence cannot be laundered into a fact by a compatibility summary, discarded optional proof probe or FlowReturn key. A compile/capability boundary or behavioral guard must reject a planted abort-to-value bypass.
- A2: Classify every affected flow preparation/finalization cause from its actual validity evidence. Audit missing-demand, stale-basis, foreign-provenance, obligation-mismatch and typed failure paths individually. A proof refusal may yield Approximate only when the evaluated value has independently valid evidence and a documented bounded interpretation; otherwise yield Unavailable or the genuine abort. Do not relabel all existing UNSTABLE_STATE sites as usable. Preserve the unserved-preparation/successful-evaluation control as explicitly Approximate and non-admitted.
- A3: Enforce the distinction at the real dispatch result boundary, execute_flow_return, direct function-return consumers and the audited/public flow API, including consumers that never call flow_return_fact. Genuine aborts return no value and admit no shared candidate; successful proof-refused values remain observable as Approximate with their causes. No FlowReturn exemption or raw-step bypass survives. A clean retry recovers.
- A4: Discriminating tests drive real producer causes through dispatch and public flow boundaries: cancellation, genuinely stale/torn evaluated evidence, independently valid evaluation after refused preparation, and unproven/invalid-value refusal. Exercise producer and singleflight-follower delivery, enclosing composition and non-admission; preserve exact controls. Plant an abort-forwarding/boundary bypass and require failure; plant a blanket proof-refusal-as-abort classification and require the usable-value control to fail. Restore and obtain green controls. Compile escape tests or typed capability enforcement prevent bypassing the required disposition.
- A5: Document the typed interface, the validity proof for each usable refusal and its consuming boundaries. Required compact disposition is production state; optional histories obey semantic-observe policy. Run cargo fmt --all -- --check; cargo clippy --workspace --all-targets -- -D warnings; focused engine/session tests; node scripts/compile-contracts.mjs; node scripts/gate.mjs with all receipts; and affected architecture-health verifiers/controls. Tests are hermetic and use durable behavioral names.

#### Legacy deletions
Remove the overloaded use of UNSTABLE_STATE as both an abort authority and usable proof-refusal class, abort-to-value raw delivery, and any FlowReturn exemption. Retain explanatory causes without letting them become a second availability authority. Reuse the existing shared engine and fact owner.

#### Evidence and measurement
Use current code, behavioral tests and compile/dependency contracts as proof. Do not use commit hashes, commit URLs, historical Git objects, ancestry, fixed test counts or stored transcripts as durable acceptance evidence. Runtime candidate/base identities and genuine dependency pins remain operational inputs. Preserve the operator's 2026-10-04 measurement rule: equivalent answer classes and deterministic work counts/growth ratios govern acceptance; any requested comparison is interleaved on the same assigned worker. Time, allocation and memory measurements are advisory here; unavailable measurements are reported and do not block. SKR-PERF-0, SKR-LIMITS and L2 retain production time/memory calibration; L1 retains the soak. No benchmark or soak is introduced by this ruling.


## Legacy deletions

Remove only the candidate's withhold_aborted_projection/abort_error helper, its memo settlement call, its FlowReturn exemption, the force_result_partial_reasons_for_tests knob and cold-build injection, and projection_abort_tests with its module wiring. Restore projection_fact module visibility. The existing producer facts and their tests remain. SKR-ABORT-1 owns replacement abort enforcement; this repair owns final projection availability adoption after it lands.

## Verification

Verify removal of the candidate-only abort bridge and its exclusive test scaffolding against the current source, then run Rust formatting, workspace Clippy, focused engine/session producer-fact and flow controls, and the configured architecture-health checks. Run applicable repository completion checks independently; report failures or unavailable checks in the controller response. No passing test count or historical commit identity is acceptance evidence.

The current repository has no roadmap ledger, charter tree, authority files or roadmap validator to amend or execute. This dated record is written at the ratification brief's explicit destination. The structured ratification-decision response supplies the complete replacement charter, addition and dependency change to the controller, which owns their application; this record introduces no repository DAG or validator.
