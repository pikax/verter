# Gate-Integrity Ledger

Debt ledger for the **gate-integrity block**.

**Owner block:** the gate-integrity block.
**Resolution gate:** that block's landing. Every row below closes there; no row may outlive it.

## Why this ledger exists

Rules landed whose mechanism does not yet exist, and both halves are deferrals under the
repository's own disposition rule (`CLAUDE.md` → Explicit finding disposition), which requires a
`DEFER` to carry a debt row naming the durable owner block, the resolution gate, and the
acceptance test:

1. **`Verification Must Prove Execution` (`CLAUDE.md`, MANDATORY)** — a gate that cannot prove it
   ran its intended surface is a FAIL. It is held today by §1a and confirm JUDGMENT only, and the
   tree contains live instances of the very class it forbids.
2. **Orchestration leg integrity (`/mom-cto-orchestration`)** — an earlier draft mandated a
   launcher (`codex_leg`), an atomic containment object, and a nonce-bound verdict record. **None
   of that was built.** A protocol that mandates a command which does not exist is worse than the
   working one it replaced: every leg fails the contract, so the contract gets ignored — which is
   the hollow-gate failure the rules exist to close. The rules landed; the mandates were cut back
   to exactly what the mechanism supports, and the machinery is owed here.

A review leg IS a gate. A leg whose verdict was established by a keyword scan is a gate that cannot
prove it ran — the same class as a test suite reporting success without executing. One owner, both
halves.

## A. Verification-surface rows

| Row | Class | Site / evidence | Acceptance bar + named test |
|---|---|---|---|
| **GI-1** | required job disabled | `.github/workflows/ci.yml:432` (`build-vscode-e2e`), `:473` (`vscode-e2e`) — both `if: false # Disabled: E2E tests are too flaky in CI`. A disabled required job produces no result, and a missing required-job result currently reads as a pass. | Both jobs run, or they leave the required set under a recorded ruling. Test: `gate_contract_integrity::disabled_required_job_fails_the_aggregator` — an `if: always()` aggregator FAILS on any missing / skipped / disabled / stale required-job receipt. |
| **GI-2** | self-declared test universe | `packages/vue-vscode/package.json` → `scripts.test` names **8** spec files; the package has **21** tracked `*.spec.ts`, so **13** (incl. `activationGate.spec.ts`) sit in no declared gate. Root `pnpm test` is `pnpm -r --parallel run test`, which runs that same 8-file script; CI runs only 4 named specs by path (its own comment: "Explicit file paths deliberately AVOID the vue-vscode package's pre-existing broken specs … that block a blanket package run"). | The package's primary gate is derived from independent discovery, not a filename list; the specs blocking a blanket run are fixed or visibly quarantined. Test: `gate_contract_integrity::every_tracked_spec_has_exactly_one_primary_gate` — parity between tree-discovered specs and gate-selected specs; fails on any orphan. |
| **GI-3** | the guard itself | `Verification Must Prove Execution` has no executable guard. | `gate_contract_integrity` lands: ONE registered suite exercising the exact canonical entry point against an independently tree-derived inventory, with per-surface negative controls for missing summary, disabled/missing job, invalid timeout nesting, zero selection, stale/missing build, missing fixture or unexpected skip, omitted/unowned test, **and a mutation that silently fails to apply**. Attestation alone does not satisfy it: a receipt faithfully attests whatever incomplete universe the runner defines for itself. Owed with it: the tree-derived verification-surface declaration, the attesting canonical driver emitting input-bound receipts, and the `if: always()` required-job aggregator. |
| **GI-4** | rule-tier promotion | `CLAUDE.md` → Verification Must Prove Execution ships `(MANDATORY)` — precisely the tier the R6 meta-guard (`every_critical_rule_in_docs_has_registered_guard`) does not check, since it scans `(CRITICAL)` headings only. A rule whose thesis is "a gate that cannot prove it ran is a failure" therefore shipped as a gate that cannot prove it ran. Landing it `(CRITICAL)` before its guard exists would FAIL the meta-guard, so `(MANDATORY)` is correct today and dishonest after GI-3. | **Acceptance criterion of this block.** In the same change that lands `gate_contract_integrity`: promote the rule to `(CRITICAL)` and add its own `CRITICAL_RULE_GUARDS` row. Test: the existing `every_critical_rule_in_docs_has_registered_guard` must pass with the new row. Never registered against the `Stub Prevention` row — a distinct invariant whose guards do not enforce these semantics. |

## B. Orchestration leg-integrity rows

Each row was MANDATED by rule text that shipped without a mechanism, and is now cut back to a
requirement the mechanism can meet. The rule text claims nothing stronger than what exists; these
rows restore the stronger claim once it is true.

| Row | Class | Cut back to (today) | Acceptance bar + named test |
|---|---|---|---|
| **GI-5** | reviewed launcher | The single `codex exec` form in `/mom-cto-orchestration` → codex Invocation, parameterised by the preflight-resolved scalars `$CODEX_MODEL`, `$CODEX_EFFORT`, `$CODEX_TIMEOUT`, `$CODEX_MAX_ATTEMPTS`, with the verdict written to its own file via `-o`. | ONE reviewed cross-platform launcher: resolves policy to scalars, spawns, enforces the timeout, joins, emits a receipt — referenced by every caller so no second copy can drift. Test SUITE (each bar gets its own case, not one uniqueness scan): `exactly_one_codex_invocation_form_in_tree`, `launcher_resolves_scalars_from_authority_only`, `launcher_enforces_timeout_and_joins`, `launcher_emits_receipt_on_every_outcome`. |
| **GI-6** | process containment | Ownership recording is scoped to dispatch SHAPE: a foreground leg has no `$!` to record (`timeout --kill-after` bounds it, the blocking call joins it); a DETACHED dispatch records `$!` before the wait, spawned under `setsid` where it exists. `terminate_recorded_tree` enumerates the descendant closure from the recorded root BEFORE killing, terminates the closure, and CONFIRMS every pid in it is gone. **Three platform facts were established by execution and are recorded here so no future round re-derives them:** (a) `$!` is the MSYS pid, which `taskkill` does NOT accept — fed it, taskkill prints `process not found`, exits 128, and kills nothing, so the correct call needs `/proc/<pid>/winpid`; (b) `taskkill //F //T` on the correct winpid reaps the LEADER ONLY — exit 0, `SUCCESS: … terminated`, with every descendant still running (observed 3/3 survivors); (c) therefore a leader-only confirmation returns SUCCESS over a live orphan tree. **Residuals, NOT asserted away — these are why enumerate-and-confirm is a STOPGAP, not the fix:** (i) a descendant that re-parents or spawns between enumeration and kill escapes the computed closure; (ii) a table read that is silently TRUNCATED yet well-formed is undetectable — nothing in `ps` output says how long it should have been, so no row validation can catch it; (iii) a dispatch that ends NORMALLY is joined by its marker, which proves the wrapper exited but NOT that it left nothing behind, and its children are re-parented beyond the reach of the recorded root; (iv) a PID is not a start identity (PIDs are reused). A search of the process table is not a containment object, and all four residuals vanish the moment containment is a property of the child instead. | A containment object created ATOMICALLY with the child, from which a descendant CANNOT unilaterally escape — **plus a process-start identity surviving PID reuse.** The bar is PER-PLATFORM, because no single primitive spans all three and pretending otherwise is how a rule becomes unimplementable on the platform nobody tested: **Windows** = Job Object (kernel-enforced, descendants inherit, escape requires an explicit privilege); **Linux** = cgroup (kernel-enforced, `cgroup.procs` enumerates, `cgroup.kill` reaps). **macOS has NO kernel-enforced equivalent** — no Job Object, no cgroup — so the Darwin bar is the strongest available and its limit is stated rather than hidden: a SUPERVISOR process owning the group, with `kqueue`/`NOTE_EXIT` (or equivalent) descendant tracking and reap-on-exit. **A bare POSIX session/process group is explicitly NOT the bar anywhere** and must not be offered as one: a child leaves its group with `setsid` at will, and a group neither detects nor reaps what survives a normally-exited wrapper — naming it "containment" would smuggle the same escape hatch back under a stronger word. Test SUITE (each platform gets its own case; a suite green on Windows only proves Windows): `containment_object_is_created_atomically_with_the_child`, `child_cannot_escape_the_containment_object_with_setsid` (Windows/Linux: MUST NOT escape; macOS: characterizes what the supervisor does and does not catch), `taskkill_receives_winpid_not_msys_pid`, `orphaned_grandchild_is_reaped_on_timeout`, `descendant_spawned_after_enumeration_is_still_reaped`, `descendant_leaked_by_a_normally_exited_wrapper_is_detected`, `recycled_pid_is_rejected_by_start_identity`, `cleanup_signals_containment_not_pid`. |
| **GI-7** | verdict record | The leg counts on a clean exit under its timeout, a matching banner, and a non-empty `-o` verdict file that is READ. The transcript is contaminated by construction — it contains the prompt echoed back — so a grep is diagnostic only. | A machine-readable verdict record bound to the dispatch nonce: grammar, PRODUCER (**the codex mandate must ASK for the record** — today it asks only for a verdict ending in `__DONE__`, so mandating the record without changing the mandate would deadlock every leg), parser, and the `CHECKPOINT-PROTOCOL.md` ledger fields. Producer and consumer land together or not at all. Test SUITE: `verdict_record_grammar_roundtrips`, `mandate_asks_for_the_record_it_requires` (producer/consumer integration — the gate that stops the deadlock), `nonce_mismatched_record_is_rejected`, `verdict_scan_of_a_timed_out_transcript_yields_no_verdict` (negative control: an observed timed-out leg carried **35** `__DONE__` occurrences and no final message). |
| **GI-8** | banner validator | The canonical fence performs the check mechanically: it extracts the banner region structurally (between the CLI's two `----` rules), requires the region to be CLOSED, matches model and effort WHOLE-LINE, and fails the leg on mismatch — a substituted reviewer is refused, not merely noticed. **An earlier draft of this row claimed a reformatted banner "fails rather than passes"; that was FALSE and the review caught it.** With only the opening rule present, the awk scan ran to EOF, so the whole transcript — prompt echo included — became "the banner region", and the echoed policy lines satisfied both greps while the real banner read `cheap-substituted`. The fence now requires a CLOSED region, requires it to carry the banner's structural keys (`workdir:`/`provider:`/`sandbox:`/`session id:`), and requires exactly one `model:`/`reasoning effort:` line inside it. **It is still not spoof-proof, and the fence says so:** it scrapes a stream that also contains the echoed prompt, so a sufficiently banner-shaped block in the prompt plus a malformed real banner defeats it. Two further attempts to close it by text alone both failed under review — and one of them (anchoring on "the first line is a rule") was factually wrong about the CLI, which prints `Reading prompt from stdin...` and a version line first, so it REJECTED EVERY REAL LEG until it was actually run. That is the row's own justification, twice over: a check whose failure modes are understood only by its author is not a validator, and text-scraping a contaminated stream is the wrong substrate for a gate. What remains missing is CENTRALISED, TESTED enforcement — the check lives in a fence each caller copies rather than in one validated component, so a caller can still omit it, and the delimiter shape is an observed CLI behaviour, not a versioned contract. | One shared validator every leg routes through, with the banner contract pinned. Test SUITE: `banner_mismatch_blocks_the_leg`, `prefix_impostor_model_is_rejected` (a `-sol-mini` must not satisfy `-sol`), `unclosed_banner_region_fails_closed` (the regression above), `slug_in_prompt_echo_does_not_satisfy_the_banner`, `caller_that_skips_the_banner_check_is_rejected`. |
| **GI-9** | attestation is not proof | `PROTOCOL.md` → Dispatch requires a recorded `CAPABILITY … result=PASS` line before the first Agent dispatch. **A recorded line is an assertion, not a proof** — nothing independently establishes transcript isolation, distinct identity, stop/continue control, or child spawning. This is the same class as GI-3 (a receipt attesting whatever universe the runner defines for itself). | An executable capability probe whose PASS is earned, not typed. Test: `gate_contract_integrity::capability_claim_without_probe_evidence_is_rejected`. |
| **GI-10a** | weak `VERIFIED` self-certification | `PROTOCOL.md` → Decision Admission concedes "weak evidence self-certified as `VERIFIED`" as a residual risk. Rule-File Integrity requires known violations to enter this ledger, not to live as rule-text carve-outs. | Requiring an evidence LOCATOR is necessary but NOT sufficient, and saying so is the point: fabricated or weak evidence can carry a commit, a command, and a result and still be self-certified. Two tests, because they close different halves: `admitted_evidence_carries_input_identity` (a `VERIFIED` label with no input-bound locator is REJECTED — closes MISSING evidence) and `admitted_evidence_is_independently_reproducible` (the recorded command, re-run by a DIFFERENT actor against the recorded tree, must reproduce the recorded result — closes UNREPRODUCIBLE evidence, which is what "self-certified" actually means). A locator check alone would leave the stated risk open while appearing to close it. |
| **GI-10b** | reopen-seam misclassification | The second-reopen circuit breaker concedes "seam misclassification is the residual gameable surface": renaming a symptom or shifting the review axis can dodge the breaker. Conceded in prose, owned by nobody. | The test presupposes a SEAM IDENTITY that does not exist yet, so the identity is the deliverable and the test is its check — naming the test without defining the oracle would be the stub this repo forbids. Owed: a reopen id derived from the FAILING CONTRACT (the guard/test/invariant name plus the rule it enforces), not from the symptom text, so two renamed symptoms over one contract collide by construction. Tests: `reopen_id_derives_from_contract_not_symptom_text`, `reopen_id_is_stable_across_symptom_rename`, `two_reopens_of_one_contract_trip_the_breaker`. (An "accepted-risk ruling" is NOT an alternative bar — a row whose closure condition is "someone decides it is fine" is not a gate, and offering it as an option is the carve-out this ledger exists to refuse.) |
| **GI-12** | stale heavy-gate lock | `PROTOCOL.md` → Active Regime / Capacity defines the global heavy-gate lock as an atomic `mkdir` with a `trap`-based release. A HARD-killed holder fires no trap and leaves the lock behind, blocking every subsequent gate. Two facts make automatic recovery harder than it looks, and both are stated rather than assumed away: (1) **the lock can be OWNERLESS** — `mkdir` is the test-and-set, so a holder killed between acquisition and writing `$LOCK/owner` leaves a lock with no owner record, and any recovery keyed on that record has nothing to key on; (2) **a dead owner PID does not mean the gate WORKLOAD is gone** — its `cargo`/test descendants can outlive it and, once re-parented, cannot be derived from the owner at all, so reclaiming on "owner is dead" can start a second heavy gate on top of a live one. Today recovery is a human who confirms no gate workload from that run remains. | Automatic stale-lock recovery keyed on the recorded owner's start identity (never on PID alone, which is reused) AND on the workload being gone — plus an explicit ownerless-lock case. Test SUITE: `stale_lock_from_dead_holder_is_reclaimed`, `live_holder_lock_is_never_stolen`, `ownerless_lock_from_kill_during_acquisition_is_reclaimable`, `lock_is_not_reclaimed_while_the_holders_workload_still_runs`. |
| **GI-13** | plan-end history purge | `PROTOCOL.md` → Repo Cleanliness defers purging scratch/report clutter from git history to a destructive history-rewrite. An earlier draft named the owner as "a named destructive-history block" — which is a requirement to CREATE an owner, not an owner, and is exactly the unowned deferral this ledger refuses. | **Owner: this block** (the gate-integrity block), like every other row here — it owns SCHEDULING the purge and landing the guard. Resolution gate: this block's landing. The destructive rewrite itself is a user-authorized action at execution time (the destructive-operation STOP boundary applies) — user authorization is a PRECONDITION of performing it, never a substitute for owning it. Test: `gate_contract_integrity::no_scratch_or_report_paths_in_landed_history`. |
| **GI-11** | red-baseline vs. green-gate | Two bars once coexisted — "zero NEW failures versus the live baseline" and Confirm's "full gate GREEN" — which cannot both hold against a red baseline, so an agent could satisfy whichever it preferred and truthfully claim "the gate". A second draft tried to reconcile them by letting an ENUMERATED ruled failure be "excluded" so confirm could still see a pass; that was worse, because no mechanism distinguishes an enumerated failure from a new one (the `errexit` fence exits nonzero for both), so "excluded" meant *a human calling a red run green*. **Now collapsed to the fail-closed bar:** a nonzero gate BLOCKS, there is NO exclusion and NO override, and no wording makes a red gate green. A pre-existing failure is a classification, not a permission. | Owner: this block. Resolution gate: this block's landing. **Closure is CONDITIONAL on the GI-15 ruling, and this row must not demand machinery that ruling may forbid.** If STRICT is ratified: GI-11 closes by proving no exclusion path exists — tests `un_enumerated_failure_fails_the_gate`, `no_gate_path_reports_green_on_a_nonzero_result`, `no_agent_supplied_allowlist_is_honoured_by_the_runner`. If an EXCLUSION is ratified: GI-11 additionally owes the runner-side mechanism — the gate runner itself skips only an explicitly enumerated, ruled entry, so the actor being gated never adjudicates its own failure — test `enumerated_entry_is_excluded_by_the_runner_not_by_the_agent`. Until the ruling, the rule text grants no exclusion at all. |

## Raised in review, FIXED rather than deferred

Two apparent contradictions were raised, found to be REAL, and repaired in the rule text — they are
recorded here so the disposition is visible and neither was silently absorbed:

- **`docs/arch` plan content vs. the no-plan-vocabulary rule.** The rule read "conventional commit
  **diffs**/messages … contain no plan/phase vocabulary", which would forbid committing the very
  `docs/arch` plans and debt ledgers the same protocol REQUIRES — this file among them. Now scoped
  to production source, code, comments, tests, and commit MESSAGES, with `docs/arch` exempt by
  construction.
- **Byte-identical `cmp` of design mirrors vs. the CRLF/LF normalization invariant.** The invariant
  was written unqualified, so it genuinely did forbid the raw `cmp` the mirror gate mandates. It is
  now scoped to byte-equality over CHECKED-OUT TEXT (which a checkout may rewrite); a same-checkout
  identity check between two files written by one run is a raw byte compare by design.

## Open decisions ESCALATED to the ratifying authority

Two items are risk-acceptance / policy calls rather than text defects, and an implementing agent
cannot self-certify them. **Each now has an OPERATIVE default written into the rules, so the
protocol is decidable today** — an undecidable rule is not "strict", it is one that every agent
resolves in whichever direction suits it, which is the hollow gate this ledger exists to close.
What is escalated is whether to CHANGE the default, not what to do in the meantime:

1. **May an ATTESTATION authorize Agent dispatch?**
   - *Operative today:* YES. A recorded `CAPABILITY … PASS` authorizes Agent dispatch; a missing,
     failed, or stale record means ABSENT and forces the `claude -p` fallback.
   - *Why it is escalated:* that line is an assertion, not a proof (GI-9). The coherent alternative
     — refuse Agent dispatch until the GI-9 probe exists — forces every dispatch onto `claude -p`.
     That is a risk-acceptance call. The default above is the status quo, stated plainly rather than
     dressed up as verification.
2. **Red baseline vs. green gate (GI-11).**
   - *Operative today:* the STRICT, FAIL-CLOSED bar. **A nonzero gate BLOCKS. There is no exclusion,
     no allowlist, and no override.** A pre-existing failure is a classification, not a permission: it
     does not lower the bar, and no wording makes a red gate green. (An earlier draft of this row said
     an enumerated failure "is excluded" — that was the contradiction itself, since no mechanism
     distinguishes an enumerated failure from a new one and "excluded" therefore meant *a human
     calling a red run green*. It is withdrawn.)
   - *Why it is escalated:* whether the project will ever accept a weaker bar — an exclusion for
     genuinely ruled pre-existing failures — is a project-policy call. Fail-closed is the default
     because the weak bar's failure mode, a red suite that quietly becomes permanent, is unbounded.
   - *Consequence for GI-11:* its closure is CONDITIONAL on this ruling. If the strict policy is
     ratified, GI-11 closes by CONFIRMING no exclusion path exists anywhere in the gate. Only if an
     exclusion is ratified does GI-11 owe the runner-side mechanism that implements it. GI-11 must not
     unconditionally demand machinery that this ruling may decide should never exist.

**Resolved, not escalated:** who owns the history purge (GI-13). This block owns it, like every
other row here. The user authorizes the destructive rewrite at execution time; that authorization is
a PRECONDITION of performing it, never a substitute for owning it.

**These two are themselves tracked deferrals, and carry the same three fields every other row does** —
otherwise "escalated" would be a bucket things fall into and never leave, which is the unowned
deferral this ledger refuses:

The OWNER of each row below is **this block** — as it must be, because the deferral rule demands a
durable owner BLOCK, and a person is not a block: "the user owns it" is how a decision waits forever
with nobody accountable for asking. What the block owns is OBTAINING and RECORDING the ruling; the
DECIDER is the ratifying authority, because an implementing agent cannot self-certify a risk
acceptance. Owner and decider are different roles, and conflating them is what produced the unowned
row in the first place.

**A user ruling does NOT bypass the governance gate, and this is not the pin-repair case.** These are
POLICY questions whose answers get written into rule text — and that rule-text change is a
rule-bearing adoption like any other, so it still requires prior neutral codex-architect approval
before it lands (`PROTOCOL.md` → GOVERNANCE). The user decides the policy; codex still reviews the
edit that encodes it. The single case where codex approval is unobtainable is the unavailable model
pin, which is structurally different because codex cannot run at all without it. Nothing here extends
that to policy rulings, and the ledger must never be the place a governance gate quietly goes missing.

| Row | Decision | Owner (accountable) | Decider | Resolution gate | Closes when |
|---|---|---|---|---|---|
| **GI-14** | May an attestation authorize Agent dispatch? | **This block** — owns putting the question, with both options and their consequences, and recording the answer. | Ratifying authority (the user). | Ruled BEFORE this block lands. It may not land carrying an unruled dispatch-authority question, because GI-9's probe design depends on the answer. | The ruling is recorded in this ledger (date + rationale) and the operative rule in `PROTOCOL.md` → Dispatch matches it. Test: `gate_contract_integrity::dispatch_authority_ruling_is_recorded_and_matches_protocol_text`. |
| **GI-15** | Red baseline vs. green gate | **This block** — owns putting the question and recording the answer. | Ratifying authority (project policy). | Ruled BEFORE this block lands — GI-11's mechanism cannot be built against an unruled bar. | The ruling is recorded here and the operative bar in `PROTOCOL.md` → Verification Gate matches it. Test: `gate_contract_integrity::red_baseline_ruling_is_recorded_and_matches_protocol_text`. |

Neither open item is resolved by an agent editing the text until a reviewer stops objecting — that
would be optimising the gate instead of the artifact, which is the failure this whole change exists
to end.

## Ruling status — HONEST, and not yet clean

Governance (`/mom-cto-orchestration` → GOVERNANCE) requires a CLEAN unprimed codex approval before
a rule-bearing change LANDS. **This ledger does not claim one, and the change has not landed.**
Recorded plainly:

- The change is COMMITTED ON A BLOCK BRANCH, unpushed and unmerged, PENDING RATIFICATION. It is
  prepared, not landed. Nothing here asserts otherwise.
- Successive unprimed codex architecture legs were run against it (`gpt-5.6-sol`, reasoning effort
  `xhigh`, startup banner verified on each). Each returned `CHANGES REQUIRED`; each round's
  findings were adopted, and the rows above are what remained after adoption.
- A `CHANGES REQUIRED` verdict is **not** a clean approval and is not presented as one. These rows
  are the RECORDED DISPOSITION the deferral rule demands (owner, resolution gate, acceptance test,
  per row). A disposition is not a substitute for the approval the governance rule demands.
- Final governance approval therefore remains OUTSTANDING and belongs to the ratifying authority,
  not to the agent that wrote the change. An implementing agent cannot self-certify.

The governing principle these legs established, and which is adopted here: **a mandate whose
mechanism does not exist is not a rule — it is a gate everyone will learn to skip.** The rules are
stated at full strength; the missing mechanism is tracked with an owner, a resolution gate, and a
test, rather than asserted in prose or quietly relaxed.

No row here may be closed by attestation alone, and none may be closed by a green run whose plant
was never proven to have applied (`/mom-cto-orchestration` → Plant Verification).
