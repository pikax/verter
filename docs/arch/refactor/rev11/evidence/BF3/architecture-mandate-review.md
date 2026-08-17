# The architecture mandate over the block

The closing round commissioned two review seats. This block is foundational class, so acceptance
requires three mandates. The missing seat was an ORCHESTRATOR SCOPING ERROR, not a seat finding, and
this file is the mandate that was owed.

## The seat, and why this one

**Codex `gpt-5.6-sol`, reasoning effort `high`**, run as an external CLI (`codex exec`, prompt on
stdin) in a dedicated worktree. Not a subagent of the actor who assembled the delta, and it authored
none of the text or code it judged.

**Disclosure: this model also sat on the closing conformance round**, where it returned BLOCKING on
the `AT-2` authority chain. The alternative seat, Grok, sat on that same round and returned LAND on
the same question, so no external seat is free of prior contact with this block. Codex was chosen
knowing that, for a reason that runs the safe direction: its prior conclusion was BLOCKING, so any
self-consistency pull biases it toward blocking again. A PASS from a seat biased to block is strong
evidence; a BLOCK from it is honestly actionable. Grok's prior on the same question was LAND, and a
lenient seat re-affirming leniency would have been the failure mode worth avoiding. Each `codex exec`
invocation is a fresh process with no memory of the earlier round.

**The mandate was the whole block, not a delta.** The prompt required the seat to enumerate every
numbered charter procedure item and every sentence of "Required exits" INDEPENDENTLY — counting them
itself rather than trusting any count in the evidence — and to cite specific per-item evidence it had
personally verified: `path:line`, a test item it could point at, or exact output of a command it ran.
An item carried on the record's own say-so was to be `NOT-EVIDENCED` by default. The prompt was
neutral ("is this correct?", never "confirm this"), stated that `NOT-EVIDENCED` is a legitimate
verdict, and supplied the maintainer acts as authority context without indicating any expected
outcome. It also forbade running the memory-heavy full gate and required any plant to be proven
applied and restored byte-identically.

## Round 1 — outcome

`ARCHITECTURE VERDICT: NOT-EVIDENCED`, with seven findings, five at P1. The seat ran six targeted
suites (20, 8, 3, 24, 12, 22 tests — all non-vacuous), executed every `#[ignore]`d target
individually and reproduced each recorded failure, ran `cargo fmt --all --check` and the ledger
validator, and authored no mutation. Its verbatim report is reproduced below.

Its findings were not a re-litigation of the closing round. Three were new and specific: a genuine
defect row carrying no correction-oriented regression, an over-claim of universal exit closure over a
residual the record itself calls open, and a maintainer act whose byte enumeration is narrower than
the edit it authorizes.

### Disposition of each finding

| id | rank | disposition |
|---|---|---|
| `BF3-A2` — TR-1 has no correction-oriented regression | P1 | **FIXED.** A shape-neutral `#[ignore]`d target added and proven RED. |
| `BF3-A1` — exhaustion asserted over an open residual | P1 | **FIXED as a record correction.** The over-claim is withdrawn and the residual's exact bounds are stated. |
| `BF3-A6` — evidence documents contradict the new act | P2 | **PARTLY FIXED, partly recorded.** The dangling link is this file. Two prose statements are deliberately left byte-unchanged, with the reason recorded. |
| `BF3-A4` — the act's byte scope is narrower than the edit | P1 | **RECORDED AND ESCALATED.** Not closable at track level; needs the maintainer. |
| `BF3-A3` — the AT-2 ignored characterization is a stub | P1 | **REJECTED, with evidence.** The test discriminates; two independent seats proved it RED under a real injected error. |
| `BF3-A5` — the live ledger does not bind this delta | P1 | **OUT OF SCOPE by ownership.** The ledger is written by the program orchestrator, never by a track. |
| `BF3-A7` — trailing whitespace on committed lines | P3 | **REJECTED as intentional.** The lines are inside verbatim seat reports. |

### `BF3-A2` — the fix, and why it is shaped this way

TR-1 is a genuine ratified defect: NAPI returns a null response for a missing node where WASM throws.
Its only test was `the_transports_serialize_a_missing_node_differently`, which asserts BOTH current
shapes — so it goes RED if EITHER is corrected. A test that fails on the fix cannot be the acceptance
gate for the fix. Every other genuine row in this block carries an `#[ignore]`d correct-behaviour
target beside its green characterization; TR-1 was the only one that did not, and charter procedure
item 6 requires one for every genuine defect.

Added: `the_transports_report_a_missing_node_the_same_way`
(`crates/verter_session/src/framework/transport_route_equivalence_tests.rs`), `#[ignore]`d.

It is deliberately **shape-neutral**. Which spelling survives — an absent response or a typed throw —
is `BRT0`'s design decision, and asserting one here would pre-empt the correction owner. What it
asserts is what the portable public contract owes regardless: the same staleness guard the
characterization uses (the in-process host must still answer `Missing`, or the comparison is
measuring something else), that neither transport publishes a product for a node that does not exist,
that the two outcomes are EQUAL, and that if the agreed answer is the error shape it stays the typed
`MissingVirtualNode` one rather than an anonymous failure.

Proven RED today, at the parity assertion and not before it:

```
cargo test -p verter_session --lib --features transport-authoritative \
  the_transports_report_a_missing_node_the_same_way -- --ignored --test-threads=1

running 1 test
test ...::the_transports_report_a_missing_node_the_same_way ... FAILED

assertion `left == right` failed: the transports still spell a missing node differently:
  napi {"outcome":"missing"},
  wasm {"message":"HostError::MissingVirtualNode: /probe/Server.svelte","outcome":"error"}
  left: String("missing")
 right: String("error")
```

The staleness guard and both no-product assertions pass before it, so the failure names the parity
gap rather than an unrelated precondition. It passes under EITHER correction, which is what makes it
a usable gate for `BRT0` instead of a second characterization. The full suite stays green at
`running 23 tests` → 21 passed, 2 ignored.

This adds no production correction — it is a test, and the transport divergence itself remains
`BRT0`'s to fix. The ratified `TR-1` row and `BRT0.md` are NOT edited: a ratified row's gating column
is amendable only by maintainer act, and `BRT0`'s procedure already says to "enable or add the
correct public-boundary parity assertion and prove it RED", which this target now satisfies as the
"enable" arm.

### `BF3-A3` — why this is rejected rather than fixed

The finding says the `AT-2` `#[ignore]`d characterization is a stub under the repository's Stub
Prevention rule because it is ignored AND passes AND is admitted not to prove one property.

The rule's own test is whether an assertion would catch the bug the change was written to detect —
and this one does. Two independent seats planted a real error into the successful response and both
drove it RED:

- Codex, closing round 2: *"`AT2-OK-ERROR` | injected an Error into successful `VirtualFileResponse`
  at `virtual_file_pipeline.rs:1121` | marker HEAD=0, worktree=2; SHA changed | RED at test line 1293
  | yes; original SHA restored"*.
- Grok, same round: *"C2-inject | site 3 `errors.push("REVIEW-PLANT-C2-INJECT-ERROR")` | ... | ignored
  test RED at `:1304` `assert_publishes_cleanly`"*.

The property it does NOT prove — that the construction READS its error list rather than writing a
hardcoded empty one — is disclaimed in its own doc comment in the same words the seats used, and its
name was changed to stop claiming it. A test with a real body, assertions that go RED under a real
plant, and an explicit statement of its own limit is the opposite of a stub; the anti-pattern the rule
names is an assertion that holds regardless of the code under test.

The `#[ignore]` + passing combination is a category mismatch with this suite's other ignored targets,
and the source says so in those terms. It is also exactly what the maintainer act directs: carry the
hazard *"as an `#[ignore]`d characterization test"*. Grok reached the same conclusion independently in
the closing round — *"a genuine precondition test, not a stub"*. Recorded as a rejected finding rather
than argued away.

### `BF3-A5` and `BF3-A7` — why these are not this track's to close

`BF3-A5` observes that the live ledger's `evidence_digest` does not match the working-tree landing
record. That is expected and structural: the ledger is written by the program orchestrator after a
track proposes fields, never by the track itself, and the digest is recomputed at that point. The
seat's second observation is sharper and is passed upward unchanged: the validator printed `OK` over
that mismatch, so it does not catch a stale evidence binding. That is a gate-integrity observation
about the validator, not about this block's content, and it belongs to the gate-integrity ledger.

`BF3-A7` reports trailing whitespace on committed lines of
[`exhaustion-closure-reviews.md`](exhaustion-closure-reviews.md). Those lines sit inside verbatim
reproductions of external seat reports, where trailing spaces are the original bytes (in Markdown a
double space is a line break). Stripping them would edit a verbatim quotation to satisfy a linter.
Kept, deliberately.

---

## Round 1 — verbatim report

````
ARCHITECTURE VERDICT: NOT-EVIDENCED

PROCEDURE ITEMS

item 1 — EVIDENCED — `crates/verter_session/src/compile/map_equality_tests/svelte_official_conformance_gate.rs:1-54` drives the shipped `VerterHost` route and authoritative harness entry. I ran `cargo test -p verter_session --lib --features bf2-authoritative svelte_official_conformance -- --test-threads=1 --nocapture`: `running 20 tests`; `17 passed; 3 ignored`; the six client-cell table, six server refusals, and committed-cell-record comparison were printed and the run exited 0.

item 2 — EVIDENCED — the client driver asserts exactly six committed cells and iterates each (`svelte_official_conformance_gate.rs:264-290`); every emitted cell requires parse/link/structural/diagnostics/mapping to report `ran` and runtime to report only `ran`/`not-applicable` (`:369-392`). The server driver asserts six committed cells and exact typed code/message (`:516-557`). The live 20-test run passed both functions and printed official package version `5.56.8`; the refusal is defined at `:174-178` and no production refusal code changed in `git diff b75fcebc33e3a100bbfff7af62fe2edceb4fcaf0..HEAD`.

item 3 — EVIDENCED — `the_gate_detects_a_planted_defect_on_every_applicable_axis_family` proves pristine pass, byte change, marker absent-before/exactly-once-after, and RED outcomes for parse, link, structural, diagnostics, and two mapping mutations (`svelte_official_conformance_gate.rs:993-1271`); `the_runtime_comparison_detects_a_planted_wrong_render` uses the same mount comparator and a still-mounting wrong-render candidate (`:1274-1315` onward). Both passed in my nonzero 20-test run. These are in-memory plants, so they cannot leave the worktree dirty.

item 4 — NOT-EVIDENCED — substantial route execution is real: my runs produced product inventory `running 24 tests; 22 passed; 2 ignored`, batch `running 12; 10 passed; 2 ignored`, and transport `running 22; 21 passed; 1 ignored`; the transport suite reflects over built NAPI/WASM/bundler artifacts (`transport_route_equivalence_tests.rs:971-1017`, `:1126-1160`, `:2144-2203`). But the retained atomicity inventory itself records a fact-empty reachability residual as `UNKNOWN, not closed` and “an open proof gap” (`docs/arch/refactor/rev11/evidence/BF3/dispositions.md:120-128`); its public search explicitly does not prove unreachability (`:165-173`). The controlling consult says that residue must be demonstrated or conclusively closed before exhaustion (`at2-disposition-ruling.md:94-105`). Broad green route counts do not turn that gap into exhaustion.

item 5 — EVIDENCED — the disposition table classifies SV-1..SV-4, RT-1, AT-1, amended AT-2, CSS-1, TR-1, RA-1 and RA-2 (`dispositions.md:19-33`) and separately remeasures BND-1/BND-2 (`:220-259`). I executed the named suites and explicitly ran the ignored targets: SV-1/SV-2/SV-3 all failed at their named assertions; SV-4 failed because TypeScript observed `{}`; RT-1 failed with Vue-shaped batch bytes and a missing refusal; AT-1 and CSS-1 failed at their named publication/product assertions; BND-2 failed because the inline map was null. AT-2's actual construction is visible at `crates/verter_session/src/host_compile.rs:743-771` and its genuine refusal arms publish empty products at `:787-824`.

item 6 — NOT-EVIDENCED — TR-1 is a genuine defect but its only test asserts the defective divergence (`dispositions.md:31`; `transport_route_equivalence_tests.rs:1253-1309`): NAPI must be `missing` and WASM must be `error`. Correcting either to the other makes this test RED. `BRT0.md:31-36` admits a correct public-boundary parity assertion still has to be added. Thus BF3 did not add a correction-oriented independently discriminating regression for every genuine defect. Independently, the AT-2 `#[ignore]` artifact passes and is known not to discriminate the production error-list read (`svelte_batch_route_tests.rs:1219-1244`; my ignored batch run: `running 2 tests`, RT-1 failed, AT-2 passed), contrary to Stub Prevention.

item 7 — EVIDENCED — every genuine row has a named correction owner, resolution gate and acceptance ID in `dispositions.md:23-31,256-259`; the owner charters exist at `charters/BA0.md`, `BS0.md`, `BCSS0.md`, and `BRT0.md`. `git diff --name-status b75fcebc33e3a100bbfff7af62fe2edceb4fcaf0..HEAD` shows only test/evidence/inventory/probe files plus BA0/ledger, no compiler/session/route/transport/CSS production implementation. No guard, production refusal, withhold path, retraction, or removal ID was added.

REQUIRED EXITS

sentence 1 “The full retained inventory has actual results.” — NOT-EVIDENCED — `dispositions.md:120-128` leaves one retained reachability question UNKNOWN/open, and `at2-disposition-ruling.md:98-103` expressly says this prevents exhaustion. The live product/batch/transport counts above prove execution, not the missing result.

sentence 2 “UNPROVEN records an open proof gap...” — NOT-EVIDENCED — the record correctly calls the residue an open proof gap (`dispositions.md:122-128`) but nevertheless claims no exit remains NOT-EVIDENCED (`landing-record.md:854-857`). That is exactly counting an unproven residue as exhaustion.

sentence 3 “Every genuine failure has exact...” — NOT-EVIDENCED — the rows provide request/route/profile/product/domain, classification, owners and IDs, and the production diff adds no guard/removal ID, but TR-1 lacks an independently discriminating correction regression: its only test requires the divergence (`dispositions.md:31`; `transport_route_equivalence_tests.rs:1284-1300`).

sentence 4 “FC-ATOMIC-001 remains non-vacuous...” — EVIDENCED — `a_genuinely_failing_batch_entry_publishes_no_partial_product` ran green in my 12-test batch invocation; it drives duplicate-canonical, compile-error, typed-other-error and panic classes on applicable lanes, proves class entry, requires no product for failure, and carries a successful neighbour (`dispositions.md:130-163`). `a_refused_combined_request_publishes_no_product_at_all` is a real RED boundary target: my ignored product run failed because an IDE projection was published beside the server-generate refusal.

sentence 5 “Route-parity tests, harness mutation controls...” — NOT-EVIDENCED — route parity and harness plants ran, but the correction-owner regression set is incomplete for TR-1 (`BRT0.md:23-36`), and the AT-2 characterization is a passing, admitted non-discriminator for the response-to-entry conversion (`dispositions.md:193-205`).

sentence 6 “If no genuine failure exists...” — VACUOUS — the antecedent is false: the ratified table contains genuine SV/RT/AT/CSS/TR failures and the post-ratification table contains BND-2 (`dispositions.md:23-31,256-259`).

sentence 7 “BF3 may close as an audit only...” — EVIDENCED — AMD-009 §7 is explicitly ratified by the maintainer act (`maintainer-ruling-section7-ratification.md:61-86`); BA0/BS0/BCSS0/BRT0 exist and each has BF3 as predecessor (`program-dag.toml:81-103`); B2/B3 list all four as mandatory predecessors (`:117-127`). This sentence's prerequisites exist; it does not itself establish BF3 acceptance.

sentence 8 “B2 and B3 stay locked...” — EVIDENCED — the DAG lists `BV0`, `BF3`, `BA0`, `BS0`, `BCSS0`, and `BRT0` for both (`program-dag.toml:117-127`), and the live ledger records BA0/BS0/BCSS0/BRT0/B2/B3 `LOCKED` (`program-state.toml:363-469`).

SCOPE AND ABORT/RESCOPE CONSTRAINTS

objective — exhaust the retained inventory — NOT-EVIDENCED — the fact-empty AT-2 reachability residual remains explicitly UNKNOWN/open (`dispositions.md:120-128`).

objective — distinguish genuine defects from harness/route artifacts — EVIDENCED — exact dispositions are recorded at `dispositions.md:23-33,256-259`; my ignored-target executions reproduced every row class described above, while BND-1's public pinned-entry control passed in the 22-test transport run.

objective — dispatch every genuine failure to its immediate root-cause correction owner — EVIDENCED — `dispositions.md:23-31,256-259` names BS0, BRT0, BA0 and BCSS0 and their acceptance IDs; the four owner charters exist and repeat those allocations.

objective — add no production retraction or defect-recognition mechanism — EVIDENCED — `git diff --name-status base..HEAD` contains no production implementation file; the only `packages/` change is `packages/unplugin/scripts/probe-bundler-route.mjs`, a test probe, and the Rust changes are test-gated modules/inventory.

runtime exclusion — Vue VDOM/Vapor/SSR runtime rows stay outside BF3 — EVIDENCED — `framework_product_surface_inventory.json:13-15` excludes Vue runtime-output correctness; the product tests only exercise Vue publication contracts. No Vue runtime compiler file changed.

no-correction ceiling — no compiler/session/route/transport/CSS/conformance correction — EVIDENCED — the base-to-HEAD name/status and zero-context production diff show no such implementation change; all behavior defects remain RED when their ignored targets are executed.

abort/rescope — repair beyond an existing immediate owner — VACUOUS — no observed genuine defect lacks a named existing owner (`dispositions.md:23-31,256-259`), so the condition requiring a further rescope did not arise.

abort/rescope — correct harness/oracle defects before disposition — EVIDENCED — BND-1/BND-2 were remeasured at the public entries (`dispositions.md:220-259`), the probe verifies built-output freshness (`:250-254`), and my full transport filter ran 22 nonzero tests successfully.

abort/rescope — never retraction, fixture identity, generated-output string scanning, or second authority — EVIDENCED — no production file changed; searches of added `crates/packages/scripts` lines found no plan identifiers or phase archaeology, and the conformance oracle uses parsed structure (`svelte_official_conformance_gate.rs:594-670`) while transport coverage enumerates actual built exports rather than scanning source names.

ARCHITECTURE-RULE COMPLIANCE

Stub Prevention (`CLAUDE.md:562-578`) — NOT-EVIDENCED — the rule forbids a characterization that does not fail pre-change/pass post-change. AT-2 is deliberately `#[ignore]` plus passing, and its own source admits a `Vec::new()` replacement stays green (`svelte_batch_route_tests.rs:1219-1244`); my ignored run confirmed it passes. TR-1's only characterization enforces the defect and fails on either correction (`transport_route_equivalence_tests.rs:1253-1309`). A maintainer act authorizing the AT-2 row does not expressly waive this binding repository rule.

Verification Must Prove Execution (`CLAUDE.md:475-485`) — NOT-EVIDENCED — all six documented targeted filters selected nonzero work in my runs (20, 8, 3, 24, 12, 22), and artifact-derived transport enumeration is real. But `suite_census.rs:34-48` admits the general all-modules/external-universe attestation hole, and the claimed exhausted atomicity surface retains UNKNOWN. Nonzero targeted execution cannot prove the missing universe/result.

Testing-Hermeticity (`CLAUDE.md:487-491`) — EVIDENCED — the changed tests/probe contain no `.integration-tests/repos`, developer-home, or external-corpus path; all six targeted suites ran from repository fixtures and local installed/built artifacts. No network fetch was observed.

No phase archaeology in production code (`CLAUDE.md:493-501`) — EVIDENCED — no production Rust implementation changed; a search of added `crates/packages/scripts` lines found no BF3/AMD/rev11/phase/cutover vocabulary.

Landed guards are structural, never name-keyed file scanners (`CLAUDE.md:469-471`) — EVIDENCED — suite presence is tied by Rust item identity and an independently re-executed libtest listing (`suite_census.rs:17-48,63-87`); transport surfaces are enumerated from built artifacts (`transport_route_equivalence_tests.rs:971-1017,1126-1160,2144-2203`). The added mechanisms do not grep production source for named functions.

Compiled-Output Conformance (`CLAUDE.md:170-180`) — EVIDENCED — the live official gate ran the pinned package and checks parsed/token-normalized structure, real-package link, diagnostics, mappings and mounted behavior, not cosmetic byte equality; source shows AST parsing/binding resolution (`svelte_official_conformance_gate.rs:594-670`) and independent axis plants (`:1017-1271`).

No program vocabulary in commits/source (`CLAUDE.md:625-630`) — EVIDENCED — the two subjects are `test(core): attribute the bundler cross-file recompile write` and `chore(arch): record the closed route-attribution gap and two remaining acceptance blockers`; searches of both commit text and added source found no program revision/block identifier. Program-document paths are exempt.

FINDINGS

BF3-A1, P1, `docs/arch/refactor/rev11/evidence/BF3/dispositions.md:120-128` — exhaustion is asserted while a retained public reachability residual is UNKNOWN/open. Concrete case: the recorded zero-fact unchanged-byte sequence may still reach last-known-good mixed output; the public search did not reproduce it and explicitly is not a proof. `at2-disposition-ruling.md:98-103` requires demonstration or conclusive closure, but `landing-record.md:854-857` declares all exits closed. Fixing the proof/evidence belongs to THIS block; if the path is demonstrated, the production outcome correction belongs to BA0.

BF3-A2, P1, `docs/arch/refactor/rev11/evidence/BF3/dispositions.md:31` — genuine defect TR-1 has no correction-oriented regression. Concrete failing case: change WASM missing-node serialization from `error` to NAPI's `missing`, or NAPI to WASM's `error`; `the_transports_serialize_a_missing_node_differently` fails at `transport_route_equivalence_tests.rs:1284-1300`, so the supposed gate rejects either parity correction. `BRT0.md:34-36` confirms the real parity assertion remains future work. Adding the ignored public-boundary target belongs to THIS block; the transport correction belongs to BRT0.

BF3-A3, P1, `crates/verter_session/src/framework/svelte_batch_route_tests.rs:1219-1247` — the AT-2 ignored characterization is an acknowledged non-discriminating passing artifact, violating Stub Prevention. Concrete failing case: replace `host_compile.rs:745-751` with `let errors = Vec::new()`; the test remains green because it separately reads the host response and only exercises inputs without error-severity success diagnostics. The evidence itself records that outcome at `dispositions.md:193-205`; my `--ignored` batch run confirmed `1 passed; 1 failed`, with AT-2 the pass. Removing/replacing the false gate or obtaining an explicit rule waiver belongs to THIS block; a typed outcome/conversion correction belongs to BA0.

BF3-A4, P1, `docs/arch/refactor/rev11/evidence/BF3/maintainer-act-at2-amendment.md:46-52` — the AT-2 authority exists but its actual byte footprint exceeds its exact scope. The act authorizes only the dispositions row and `BA0.md` lines 28 and 37. `git diff --unified=0 base..HEAD -- BA0.md` shows a third changed hunk at current `BA0.md:59-62` (Required exits). Those bytes are semantically related but are not one of the exact named locations. The current files are byte-identical to commit 9104 (`git diff --quiet 9104e0be7 -- <two paths>` exit 0; blobs `abc6cf2...` and `b5f2850...`), which proves no post-act drift but not authority for the extra pre-act hunk. Authority expansion or byte reversion belongs to THIS block's governance/docs; no production layer owns it.

BF3-A5, P1, `docs/arch/architecture-lock/ledger/program-state.toml:341-360` — the uncommitted act/evidence delta is not bound by the live ledger. The ledger says its evidence digest is SHA-256 of raw `landing-record.md` and records `d2d652ce...`; that is the HEAD file hash, while the reviewed working-tree landing record hashes to `fb04e4d6...`. The validator still prints `OK ... validated 62 blocks`, demonstrating it does not catch this binding failure. Rebinding the evidence and updating the live state belongs to THIS block.

BF3-A6, P2, `docs/arch/refactor/rev11/evidence/BF3/dispositions.md:57-68` — the evidence documents contradict the new act: dispositions still says the naming-act objection is unresolved, while `maintainer-act-at2-amendment.md:67-76` and `landing-record.md:863-877` say it is closed; `dispositions.md:299` says “no amendment” after `:37-55` says the row was amended; ledger notes still list the naming act as an outstanding blocker (`program-state.toml:360`); and `exhaustion-closure-reviews.md:236` links to absent `architecture-mandate-review.md`. These are record-integrity defects. Fix belongs to THIS block's evidence/ledger only.

BF3-A7, P3, `docs/arch/refactor/rev11/evidence/BF3/exhaustion-closure-reviews.md:51-52,172` — `git diff --check base..HEAD` reports trailing whitespace on these committed lines. Fix belongs to THIS block's documentation.

PLANTS YOU AUTHORED

none | no worktree file was mutated | n/a | I executed the committed in-memory parse/link/structural/diagnostics/mapping/runtime plants and inspected the recorded AT2 hardcode plant, but authored no mutation | n/a

WHAT YOU DID NOT CHECK

I did not rerun `node scripts/gate.mjs`, per instruction, nor independently rerun the recorded full workspace nextest/direct-libtest/shipped-cfg surfaces, clippy matrix, release check, or all TypeScript tests. I did run `cargo fmt --all --check` (exit 0), the six targeted Rust filters, all relevant ignored filters, and the live program-state validator.

I did not regenerate all official goldens or rebuild every NAPI/WASM/unplugin artifact independently; the transport tests' local freshness/load checks passed against the present artifacts.

I did not independently authenticate the maintainer's identity outside the repository; per mandate I treated the recorded maintainer acts as authority, then checked their text, scope, hashes and application.

I did not open every production function cited by the 6,314-line evidence corpus. Findings above rely only on code/tests/acts I opened and commands I ran; unchecked claims are not used to award EVIDENCED.

git status --porcelain:

```text
 M docs/arch/refactor/rev11/evidence/BF3/at2-deviation-memo.md
 M docs/arch/refactor/rev11/evidence/BF3/exhaustion-closure-reviews.md
 M docs/arch/refactor/rev11/evidence/BF3/landing-record.md
 M docs/arch/refactor/rev11/evidence/BF3/maintainer-standing-ruling-bugs-and-types.md
 M docs/arch/refactor/rev11/evidence/BF3/test-invocations.md
?? docs/arch/refactor/rev11/evidence/BF3/maintainer-act-at2-amendment.md
```

ARCHITECTURE VERDICT: NOT-EVIDENCED
````

---

## Round 2 — the fix delta, and the items round 1 left open

The same seat, a fresh process, on the tree as fixed. Its mandate was the fix delta plus a
re-judgment of every item round 1 marked `NOT-EVIDENCED`, plus a CORRECT/WRONG ruling on each of the
four findings that were not fixed. The prompt said explicitly that re-affirming its earlier position
was a legitimate answer, and it authored its own plant rather than re-running any recorded one.

`ARCHITECTURE VERDICT: BLOCKING`, **no new findings**, and all four dispositions upheld as CORRECT —
including the two rejections of its own round-1 findings.

What moved:

| item | round 1 | round 2 |
|---|---|---|
| procedure item 4 | `NOT-EVIDENCED` | **EVIDENCED** |
| procedure item 6 | `NOT-EVIDENCED` | **EVIDENCED** |
| Required exits sentence 2 | `NOT-EVIDENCED` | **EVIDENCED** |
| Required exits sentence 3 | `NOT-EVIDENCED` | **EVIDENCED** |
| Required exits sentence 5 | `NOT-EVIDENCED` | **EVIDENCED** |
| Stub Prevention | `NOT-EVIDENCED` | **EVIDENCED** |
| Required exits sentence 1 / "exhaust the retained inventory" | `NOT-EVIDENCED` | `NOT-EVIDENCED` |
| Verification Must Prove Execution | `NOT-EVIDENCED` | `NOT-EVIDENCED` |

The seat verified the TR-1 target does what it claims and introduces no defect — it reached the
parity assertion with the staleness guard and both no-product checks passing first, the existing
characterization still passes beside it, and the seat independently checked shape-neutrality in both
directions ("copying NAPI's missing response to WASM satisfies equality and skips the error-only
branch; copying WASM's typed error to NAPI satisfies equality and both `MissingVirtualNode` checks").

On Stub Prevention it did the work rather than taking the record's word: it planted
`REVIEW-R2-AT2-INJECTED-ERROR` into the HostBacked success arm, proved the marker absent-before and
once-after with a changed file SHA, drove the `AT-2` artifact RED, restored the file to its original
SHA, and re-ran green. Its conclusion — *"BF3-A3's proposed `errors = Vec::new()` case changes a
property this test expressly does not claim; it is not a counterexample to the stated test"* — is a
reversal of its own round-1 finding, reached by experiment.

### Why this round is still BLOCKING

Two items remain, and neither is closable at track level.

**1. `BF3-A4` — the maintainer act's byte enumeration (P1, the acceptance blocker).** The seat ruled
the RECORDED-AND-ESCALATED disposition CORRECT and restated the requirement: *"only the maintainer
can confirm coverage or direct reversion."* It confirmed the two authorized paths are byte-identical
to `9104e0be7` (identical blob IDs on both sides), so there is no drift — the question is authority
over one pre-existing hunk, not a new edit.

**2. Required-exits sentence 1 and the "exhaust the retained inventory" objective.** The `AT-2`
reachability residual is recorded as UNKNOWN, and the controlling independent consult says a residue
must be demonstrated or conclusively closed before exhaustion is claimed. The seat's precise reading:
the naming act *"changes AT-2's classification and item-6 consequence, but does not amend this BF3
exit; its stated effect names item 6 only."* So the act moved item 6 and left the exhaustion exit
where it was. Closing it needs either a conclusive structural proof of unreachability — real
investigation, not a record edit — or a maintainer act ruling that a recorded, bounded, dispatched
open proof gap satisfies that exit. Neither is a track-level decision, and the delta deliberately
does not pretend otherwise: withdrawing the universal-closure claim was the honest half, and the seat
credited it as such while leaving the exit open.

The third `NOT-EVIDENCED` row, **Verification Must Prove Execution**, is not a finding against this
block. The seat located it exactly: the repository already records that class as an open
gate-integrity row (`docs/arch/gate-integrity-ledger.md`, GI-21) with its owner and resolution gate
outstanding, and `CLAUDE.md` says of that rule in its own words that it currently fails its own test.
No block closes it by running more targeted filters.

### What the seat did NOT re-run

Stated in its own report rather than inferred: the correction targets for SV-1 through SV-4, RT-1,
AT-1, CSS-1 and BND-2 were carried from round 1 (their code is unchanged by the delta, and round 1
reproduced each named failure individually); procedure item 7, Required-exits sentences 7 and 8, the
runtime exclusion, the abort/rescope constraints and the remaining architecture-rule rows were
carried as EVIDENCED from round 1 with the no-production-change half independently re-checked this
round. The canonical full gate, the nextest/direct-libtest pair, the shipped-cfg surface, the clippy
matrix, the release check and the TypeScript workspace tests were not run this round, by instruction.

---

## Round 2 — verbatim report

````
ARCHITECTURE VERDICT: BLOCKING

FIX DELTA

- TR-1 target — the change does what it claims and introduces no defect. `crates/verter_session/src/framework/transport_route_equivalence_tests.rs:1327-1395` adds a separate ignored correction target without weakening the green characterization at `:1253-1309`. I ran the new target exactly: `running 1 test`; exit 101; `0 passed; 1 failed`; it reached `:1378` and reported `left: "missing"`, `right: "error"`. Therefore the host-Missing guard (`:1341-1358`) and both no-product checks (`:1363-1375`) passed first. The old characterization separately ran `1 passed`, and the full filter ran `23 tests` -> `21 passed; 2 ignored`. The target is shape-neutral in the claimed, bounded sense: copying NAPI's missing response to WASM satisfies equality and skips the error-only branch; copying WASM's typed error to NAPI satisfies equality and both `MissingVirtualNode` checks (`:1383-1394`). The probe normalizers emit only `missing`, `published`, or `error` (`packages/native/scripts/probe-transport-surface.mjs:59-75`; `packages/wasm/scripts/probe-transport-surface.mjs:55-71`), while the existing host-equivalence rail rejects any non-missing-shaped result and any product (`transport_route_equivalence_tests.rs:377-399`). `cargo fmt --all --check` exited 0.
- Landing-record correction — accurate, no new defect. The universal sentence “No charter exit criterion is now recorded NOT-EVIDENCED” was removed by the diff. Current `landing-record.md:858-879` expressly records the AT-2 residual as open/UNKNOWN and says it is not counted as exhaustion. This is an honest record correction, not proof that the residual is closed; my re-judgment below therefore leaves the broader exhaustion objective and exit sentence 1 NOT-EVIDENCED.
- Named act and scope discrepancy — accurate but deliberately non-curative. `maintainer-act-at2-amendment.md:27-70` records an act naming AT-2 and authorizing the four substantive clauses. `git diff --quiet 9104e0be7..HEAD -- dispositions.md BA0.md` exited 0; blob IDs were identical on both sides (`dispositions.md` `abc6cf2a6187b2f259438786cfe2fb885f669314`; `BA0.md` `b5f285035e58a24a81640857eecf1b74c7445654`). The act itself records that its locator enumerates only BA0 lines 28 and 37 while `git diff --unified=0 b75fcebc3..HEAD -- .../BA0.md` shows three hunks, including `@@ -54,5 +59,5 @@` (`maintainer-act-at2-amendment.md:98-118`; `landing-record.md:901-919`). It does not pretend to resolve that discrepancy; it correctly leaves a blocker needing maintainer action.
- Deliberately unchanged prose — claim verified. `dispositions.md:57-68` still says the naming objection is unanswered and `:297-303` still says “no amendment”; the new act labels both historical/superseded and explains why its byte scope does not authorize editing them (`maintainer-act-at2-amendment.md:120-128`). The same blob check proves `dispositions.md` and `charters/BA0.md` are byte-identical to `9104e0be7`. This preserves confusing historical prose, but the explicit supersession/read order prevents the delta from presenting it as current authority.
- Delta boundary — `git diff --name-status 9104e0be7..HEAD` contains one test module and seven BF3 evidence documents only; no compiler/session/route/transport/CSS production correction was introduced. `git diff --check 9104e0be7..HEAD` produced no output.

RE-JUDGED ITEMS

- procedure item 4 — EVIDENCED — `crates/verter_session/src/framework/framework_product_surface_inventory.json:1-8` defines the retained reachable product/route inventory and records each bundler alias as DRIVEN with route identity/publication evidence. I reran all named surfaces: official conformance `20` -> `17 passed, 3 ignored`; PublicApi/TSC/declaration `8` -> `7 passed, 1 ignored`; IDE `3` -> `3 passed`; product/route `24` -> `22 passed, 2 ignored`; batch `12` -> `10 passed, 2 ignored`; transport `23` -> `21 passed, 2 ignored`. This establishes the narrower procedure-4 set of retained reachable-success products and public/default routes. It does not establish the broader exhaustion objective below.
- procedure item 6 — EVIDENCED — the named act validly reclassifies AT-2 as a latent, reachability-unproven hazard rather than a genuine defect (`maintainer-act-at2-amendment.md:41-44,67`), so item 6 imposes no defect-regression duty on AT-2. For its stated precondition artifact, the pristine ignored test passed; my proven injection of an error into the HostBacked success entry made it fail at `svelte_batch_route_tests.rs:1315`, and after restoration it and the live warning-only control both passed. TR-1 now has the separately named RED correction target at `transport_route_equivalence_tests.rs:1327-1395`. The other genuine-row RED outcomes are carried from round 1 as itemized below because their code is outside `9104e0be7..HEAD`.
- Required exits sentence 1, “The full retained inventory has actual results.” — NOT-EVIDENCED — the retained AT-2 row still carries reachability unproven and an explicitly UNKNOWN residual (`dispositions.md:29,120-128`). The controlling consult says that even after a maintainer amendment, recording this residual UNKNOWN is an open proof gap and requires demonstration or conclusive source/structural closure before exhaustion is claimed (`at2-disposition-ruling.md:94-105`). The new act changes AT-2's classification and item-6 consequence, but does not amend this BF3 exit; its stated effect names item 6 only (`maintainer-act-at2-amendment.md:65-70`). Procedure 4's known reachable surface inventory is driven; the broader retained conformance inventory is not exhausted.
- Required exits sentence 2, “UNPROVEN records an open proof gap and cannot count as exhaustion.” — EVIDENCED — the delta now does exactly this: `landing-record.md:858-879` calls the residual open/UNKNOWN, says it is not counted as exhaustion, bounds it, and dispatches it to BA0 without claiming closure.
- Required exits sentence 3, “Every genuine failure has exact ...” — EVIDENCED — `dispositions.md:23-31` supplies the request/class/owner/gate/acceptance mapping; the named act removes AT-2 from the genuine-defect set; the new TR-1 target supplies the only regression missing in round 1 and was independently RED at its parity assertion. The act-authorized AT-2 row and BA0 row/paragraph preserve the named owner and acceptance ID. The separate under-inclusive BA0 Required-exits hunk remains the BF3-A4 governance blocker below, but does not erase those authorized fields.
- Required exits sentence 5, “Route-parity tests, harness mutation controls...” — EVIDENCED — current executions selected nonzero transport, product, batch, and official-conformance work; the official suite's committed axis-mutation tests ran green; the new correction-oriented TR-1 target is RED; and the AT-2 precondition artifact independently discriminated the injected mixed entry. The genuine correction-owner regression set is therefore complete under the named reclassification.
- objective, “exhaust the retained inventory” — NOT-EVIDENCED — same concrete residue and controlling ruling as sentence 1: `dispositions.md:120-128` is still UNKNOWN/open and `at2-disposition-ruling.md:98-103` says it must be demonstrated or conclusively closed before exhaustion is claimed. Correctly recording and dispatching an open proof gap is not exhausting it.
- Stub Prevention — EVIDENCED — `CLAUDE.md:562-578` forbids assertions that hold regardless of the bug they claim to detect. The AT-2 artifact narrowly claims only the success-response precondition and expressly disclaims independent error-list extraction (`svelte_batch_route_tests.rs:1198-1244`). It passed pristine, failed after a proven success-entry error injection at `:1315`, passed after byte-identical restore, and its live unignored control passed. BF3-A3's proposed `errors = Vec::new()` case changes a property this test expressly does not claim; it is not a counterexample to the stated test. The TR-1 target independently fails on the actual parity defect. No stub remains among the tests used for these claims.
- Verification Must Prove Execution — NOT-EVIDENCED — all targeted filters ran nonzero work and the transport/build prerequisites were fresh enough for those tests to execute, but the mandatory rule requires independently tree-derived universe parity, not a binary's self-declared universe (`CLAUDE.md:475-485`). `suite_census.rs:34-48` admits it cannot see a target the runner never invoked, a cfg-gated module it was never given, or an unregistered suite. The repository records that still-open class as GI-21 with owner/tests outstanding (`docs/arch/gate-integrity-ledger.md:43`). My nonzero counts do not close it.

DISPOSITION REVIEW

BF3-A3 CORRECT — the rejection is supported. The test's actual contract is the upstream precondition (`svelte_batch_route_tests.rs:1198-1244`), not proof that site 3 reads rather than hardcodes its list. Plant `REVIEW-R2-AT2-INJECTED-ERROR` made the named artifact RED at `:1315`; pristine/restored runs were GREEN. The earlier `Vec::new()` counterexample attacks the expressly disclaimed property, so it does not show that this characterization is a stub.

BF3-A4 CORRECT — the disposition as RECORDED AND ESCALATED is right. The act's exact scope names the AT-2 row and BA0 lines 28/37 (`maintainer-act-at2-amendment.md:46-52`), while the historical edit has a third Required-exits hunk. The act/landing record explicitly refuse to infer coverage and say acceptance must wait (`maintainer-act-at2-amendment.md:98-118`; `landing-record.md:901-919`). This is a P1 acceptance blocker in THIS block's governance/docs; only the maintainer can confirm coverage or direct reversion.

BF3-A5 CORRECT — the ownership disposition is right, although the stale binding fact remains. Governance makes the program orchestrator the sole `program-state.toml` writer (`governance.md:181`). Current command evidence: landing-record SHA-256 is `97a65b26211b6b522de592a4ba6b2fd3a932910409abe0e7d8224bbc2025255e`, ledger BF3 digest is `d2d652ceea287ead809c6d352b538b944c6f45b1b81a59e9e42c08a99fb03863`, yet `validate-program-state.mjs` exits 0 with `OK ... validated 62 blocks`. Rebinding must occur in the program-orchestrator transition after this seat, and the validator weakness belongs to gate integrity, not a BF3 track edit.

BF3-A7 CORRECT — `git diff --check b75fcebc3..HEAD` reports trailing whitespace at `exhaustion-closure-reviews.md:54,55,175`, but those lines are within sections explicitly introduced as verbatim reports (`:45-51`, `:160-175`); the spaces are Markdown hard breaks. `git diff --check 9104e0be7..HEAD` is empty, so this fix delta neither adds nor alters them. Rewriting quoted bytes is not required.

CARRIED FROM ROUND 1

- Carried EVIDENCED/RED, not individually rerun with `--ignored` this round: SV-1, SV-2, SV-3, SV-4, RT-1, AT-1, CSS-1, and BND-2 correction targets. Their relevant code is unchanged by `9104e0be7..HEAD`; round 1 individually reproduced each named failure. This round reran their surrounding official/product/batch/transport default suites and independently ran the only new target, TR-1.
- Carried EVIDENCED where outside the delta and not re-run end-to-end: procedure item 7; Required-exits sentences 7 and 8 (sentence 6 remains VACUOUS); the runtime exclusion and abort/rescope constraints; Testing-Hermeticity, No phase archaeology, structural landed-guard, compiled-output conformance, and no-program-vocabulary rows. Current name/status inspection independently corroborated the no-production-change portions. Procedure items 1-3 and Required-exits sentence 4 were not merely carried: their official/product/batch test items executed in this round's targeted runs.
- Not carried as fresh evidence: the canonical full gate, full nextest/direct-libtest pair, shipped-cfg surface, clippy matrix, release check, and complete TypeScript workspace tests. They were not run this round; `node scripts/gate.mjs` was not run by instruction.

NEW FINDINGS

none

PLANTS YOU AUTHORED

R2-AT2-INJECT | added `errors.push("REVIEW-R2-AT2-INJECTED-ERROR")` immediately after the HostBacked success arm's diagnostic collection in `crates/verter_session/src/host_compile.rs` | marker absent before (`0`), exactly once after (`1`); SHA `13f2cd52dfa8b87daa856fc553636a70785df0c014db1f5c0ff6eda974153730` -> `a227ccd8cfb87b2d0e135f811921b05621e1a6b1798c184e39700bb925a020b0` | named ignored test exited 101, `running 1 test`, RED at `svelte_batch_route_tests.rs:1315` because the successful entry reported the planted error; after restore the ignored test and live warning-only control each ran `1 passed` | restored SHA `13f2cd52dfa8b87daa856fc553636a70785df0c014db1f5c0ff6eda974153730`, marker `0`, `git diff --exit-code -- host_compile.rs` clean

git status --porcelain: EMPTY
````
