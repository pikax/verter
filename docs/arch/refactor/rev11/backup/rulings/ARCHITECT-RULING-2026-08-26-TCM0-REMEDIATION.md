# Ruling

TCM0 is not acceptible under its current charter. It becomes acceptible only after the acceptance instrument is rebuilt, the charter is atomized and re-ratified, the evidentiary claims are reconciled, and all three mandates pass against one frozen final candidate.

The evidence package is repairable, but its acceptance spine is not. Preserve the useful package observations and architecture decisions; rebuild the machinery that turns them into claims of closure.

Disposition of the 36 reported findings:

- **Must close:** 32
- **May remain only as named, ratified residue:** 4
- **Not findings:** 0

The later digest re-pin repaired charter identity, but not the false prose claiming TCM0 did not edit those charters. That architectural breach remains open.

## Instrument ruling

“Derive, do not declare” is correct and binding.

The stated floor—“an instrument that discloses a limit forces bounded status”—is necessary but insufficient. It catches disclosed limits while still trusting the author to disclose them. It also cannot detect an omitted claim, an incomplete proof universe, or a proof citation that merely exists.

The current free-form charter and proof prose are not machine-comparable. The solution is not natural-language inference. The rescope must decompose the charter into stable, atomic claim IDs. Proof instruments report which atoms they exercised; the validator computes coverage.

The replacement register must contain claim IDs and proof references, but no author-set status field. Any input `status` field must be schema-invalid. The human-readable [closure register](/Users/carlosrodrigues/Documents/dev/verter/docs/arch/refactor/rev11/evidence/TCM0/closure-register.md:1) becomes generated output.

A bounded row is trustworthy only when the validator establishes all of the following:

1. The claim ID and its complete atom set come from the digest-bound charter.
2. The proof uses an allowlisted, claim-specific adapter—not a generic “file exists” check.
3. The proof receipt binds the exact repository SHA/tree, package digest, fixture inputs, command, and instrument digest.
4. The execution completed with the expected terminal summary, nonzero selected work, no unexpected skips, and internally consistent counters.
5. Required negative controls ran through the canonical entry point, each mutation was proven unique and newly applied, and each produced the expected refusal.
6. Covered atoms come from the instrument’s receipt, not the register author.
7. Any disclosed limitation, partial result, unexecuted case, sample-only universe, or adapter incapable of proving totality forces `PROVEN-BOUNDED`.
8. The remainder is computed as `claim atoms − covered atoms`; it is not free-text that can be deleted.
9. Every remainder has a stable residue ID, an authorized owner, a concrete receiving-charter criterion, a resolution gate, and matching charter/authority digests.
10. The owner relationship is non-circular. A downstream block may receive residue only through a ratified scope transfer; naming a dependent in a comment is insufficient.
11. Missing dependencies, stale receipts, digest drift, contradictory counters, absent evidence, or an unrecognized proof kind produce `OPEN`/`REFUSED`.
12. A bounded proof is never independently acceptance-admissible. Acceptance requires either full proof or a valid ratified residue transfer for every computed remainder.

The replacement instrument must call its successful pre-review outcome `READY_FOR_MANDATES`, not `ADMISSIBLE`. It establishes structural readiness; it does not semantically certify prose. The three independent mandates remain the substantive acceptance authority.

## Required rescope

The present [TCM0 charter](/Users/carlosrodrigues/Documents/dev/verter/docs/arch/refactor/rev11/charters/TCM0.md:15) must be amended before TCM0 can be accepted.

The amended scope is:

- TCM0 still owns the exact package identity, current mapper protocol, configured and inferred project behavior, trust/external-code behavior, declaration/build/watch/incremental behavior, and the current semantic API surface. The missing current-package probes stay in TCM0 and must be executed.
- “Reproduce the hang” becomes “run a discriminating bounded reproduction attempt and record occurrence or non-occurrence without generalizing beyond the exercised topology.” Non-occurrence is bounded evidence, not a fabricated defect and not proof of absence.
- Feature ownership requires one row per `TypeProvider` method and one row per non-trait feature/background capability. Split ownership is legal only across explicit, disjoint predicates. The impossible “every call site” lexical universal is removed; a lexical inventory may be a one-time discovery aid but cannot be a landed acceptance guard.
- TCM0 must finish the diagnostic ownership rules, owner-sensitive terminal mask function, external-source partition, current concrete deletion/survival inventory, cache/lifecycle contract, and acyclic test specification.
- The already-ratified topology split stands: TCM0 owns candidates, metrics, harness, and selection rule; TCM2 and TCM3 own measured selection.
- The already-ratified performance ruling stands: TCM0 owns invariant bounds, workloads, measurement method, and comparison rules—not dedicated-machine absolute numbers. Each implementation owner captures its pre-implementation comparison reference before changing the path.
- TCM0 names every mechanism that exists now. TCM1–TCM3 record types/codecs introduced or orphaned by their implementation; TCM4 verifies the accumulated manifest. That is a ratified accumulation contract, not TCM4 rediscovering TCM0’s current tree.
- Acceptance permits only the three residue records below. No other bounded row is admissible.

The named residues are:

| Residue | Finding entries | Owner and resolution gate |
|---|---:|---|
| `TCM0-R-HANG-TOPOLOGY` | C2, AD4 | TCM3 reruns the attach/hang probe before selecting editor-attached topology; TCM4 reruns it for the activated certified package. |
| `TCM0-R-TOPOLOGY-SELECTION` | C7 | TCM2 selects projection topology and TCM3 selects semantic topology through new numbered blocking exit criteria using TCM0’s locked harness and metrics. |
| `TCM0-R-IMPLEMENTATION-BASELINE` | C9 | TCM1–TCM3 capture current-path comparison measurements before implementation results exist; TCM4 verifies the activated result. No absolute-machine threshold is invented. |

`TCM0-R-HANG-TOPOLOGY` accounts for two of the 36 lane findings, so the residue count is four finding entries.

## Repair versus rebuild

Rebuild these rather than patching them:

- `closure-register.md` and `probes/closure-validator.mjs`
- `claim-sweep-universe.md`
- `receiving-coverage.md`, its derivation, controls, and generated report
- The landed lexical call-site/capability scanners and their generated universal claims
- The ownership ledger’s 31-row/44-method shape
- The committed transcript and regeneration contract
- `OPEN-GAPS.md`, the summary, and the downstream-correction/refinement narratives as closure authorities

Delete the tracked Python/POSIX control path and replace it with one portable Node self-test. Delete the name-keyed scanner guards; do not rename them and retain the same enforcement.

Preserve and repair:

- Package digest, `gitHead`, binary provenance, mapper request/response captures, and the useful semantic API probes
- The stale-snapshot characterization
- The cache/lifecycle contract
- The acyclic-invariant test specification
- The five projection classes
- Existing ratified ownership decisions, transcribed into the rebuilt one-method/one-capability ledger
- Concrete deletion and survivor entries
- Probe 10 after consolidating its failure accounting
- The diagnostic, projection, external-source, and performance documents after their false universals are removed

## Finding-by-finding disposition

`C` denotes conformance, `AR` architecture, and `AD` adversarial, in report order.

| ID | Disposition | Required resolution |
|---|---|---|
| C1 | MUST CLOSE | Add behavioral inferred-project, trusted/untrusted, declaration emit, build, watch, incremental, and `tsbuildinfo` probes. |
| C2 | RESIDUE | Record bounded non-reproduction as `TCM0-R-HANG-TOPOLOGY`; stop calling it reproduction or absence proof. |
| C3 | MUST CLOSE | Rebuild the ledger one method/capability per row; remove lexical universality and stale derivation. |
| C4 | MUST CLOSE | State deterministic Vue and Svelte attribution/suppression rules. |
| C5 | MUST CLOSE | Make owner a real terminal-mask input and generate the complete owner-sensitive table. |
| C6 | MUST CLOSE | Split mixed external-source rows until every atomic sub-shape has exactly one model. |
| C7 | RESIDUE | Bind measured topology selection to TCM2/TCM3 as `TCM0-R-TOPOLOGY-SELECTION`. |
| C8 | MUST CLOSE | Settle every extant deletion/survival disposition and install the exact future accumulation contract. |
| C9 | RESIDUE | Bind pre-implementation comparison measurements as `TCM0-R-IMPLEMENTATION-BASELINE`. |
| AR1 | MUST CLOSE | Replace the self-certifying validator. |
| AR2 | MUST CLOSE | Bind receiving obligations by stable criterion IDs, not proxy string matches. |
| AR3 | MUST CLOSE | Replace the false-positive shell controls with fail-closed Node controls. |
| AR4 | MUST CLOSE | Regenerate or replace the stale capability evidence. |
| AR5 | MUST CLOSE | Eliminate declared status vocabulary; status becomes output. |
| AR6 | MUST CLOSE | Delete landed name-keyed scanner guards and remove their universal claims. |
| AR7 | MUST CLOSE | Remove tracked Python invocation and provide a portable Node entry point. |
| AR8 | MUST CLOSE | Rewrite the false charter-edit claims; retain the later digest re-pin as identity evidence. |
| AR9 | MUST CLOSE | Correct the TCM2 criterion reference and bind the actual unresolved wire semantics. |
| AR10 | MUST CLOSE | Put the position-clamping obligation and test in the owning TCM2/TCM3 charters or remove the claim. |
| AR11 | MUST CLOSE | Re-ratify the final ownership evidence bytes after all rebuilding. |
| AR12 | MUST CLOSE | Replace nonexistent `successor-owned` tests with the actual TCM3 owner and criterion IDs. |
| AR13 | MUST CLOSE | Give deletion item 2 one disposition; remove simultaneous open/closed prose. |
| AR14 | MUST CLOSE | Reconcile the projection contract with the current row-15 disposition. |
| AR15 | MUST CLOSE | Replace hand counts with a recursively generated, digest-bound subject manifest. |
| AD1 | MUST CLOSE | Correct the landed-charter narrative and bind the final amendments through an actual authority act. |
| AD2 | MUST CLOSE | Remove stale validator-output claims; the later digest re-pin only partially resolves this. |
| AD3 | MUST CLOSE | Same instrument rebuild as AR1, including non-admissibility of an untransferred bounded row. |
| AD4 | RESIDUE | Same bounded hang non-reproduction as C2. |
| AD5 | MUST CLOSE | Replace the stale capability derivation. |
| AD6 | MUST CLOSE | Rebuild the transcript so its semantic assertions are regenerated byte-for-byte; isolate nondeterministic timings as run metadata. |
| AD7 | MUST CLOSE | Generate the evidence universe recursively and derive all counts from it. |
| AD8 | MUST CLOSE | Execute project-wide-reference and auto-import compositions; if they fail, reopen their architectural disposition. |
| AD9 | MUST CLOSE | Remove the every-call-site claim and satisfy the amended structural ownership contract. |
| AD10 | MUST CLOSE | Atomize the three mixed external-source rows. |
| AD11 | MUST CLOSE | Same exact deletion/survival and accumulation resolution as C8. |
| AD12 | MUST CLOSE | Use one failure counter and assert that visible failures, summary count, and exit status agree. |

None of the 36 is rejected as “not a finding.”

## Ordered acceptance sequence

1. **Land the instrument repair alone.**  
   **Done:** Replace the current parser with a status-free structured register, claim-specific proof adapters, a generated Markdown view, and a Node self-test. Do not adjudicate the other findings in this change. The repaired validator must report `REFUSED` on the current evidence.  
   **Verified by:** Mutations for omitted claims, added input status, removed residue/owner, missing dependency, stale receipt, existing-but-irrelevant proof, zero selection, skipped work, inconsistent counters, unapplied negative control, and disclosed limits. The original `PROVEN-BOUNDED → PROVEN` laundering mutation must be impossible because no status input exists.  
   **Evidence:** A machine-readable self-test receipt bound to the instrument SHA/tree, plus the live-tree `REFUSED` receipt. This completes the binding instrument-first precondition.

2. **Ratify the amended TCM0 contract and residue transfers.**  
   **Done:** Rewrite TCM0 into atomic stable claim IDs with the scope above; add exact receiving criteria to TCM1–TCM4; record the three residue IDs; replace the literal hang-reproduction requirement; preserve the earlier topology/performance rulings. Re-pin every amended charter through one authorized act. Move the ledger from `RESCOPE_REQUIRED` to `IN_PROGRESS` only after that act validates.  
   **Verified by:** Charter atom inventory parity, authority-registry digest equality, DAG ownership/non-circularity checks, and live program-state validation.  
   **Evidence:** The ratified rescope ruling/amendment, new charter digests, receiving criterion IDs, and a valid `IN_PROGRESS` ledger row.

3. **Rebuild the acceptance evidence foundation.**  
   **Done:** Replace the hand-counted claim universe with a recursive path-and-digest manifest; rebuild the ownership ledger as one row per trait method and non-trait capability; remove the name-keyed scanners and their generated universal artifacts; replace receiving-coverage string proxies with direct stable-ID references; replace the shell/Python controls with Node.  
   **Verified by:** Manifest regeneration equality, 44-method ledger parity against a one-time pinned-compiler/rustdoc inventory, unique owner predicates, no unclassified capability, and control tests for missing criteria/dependencies. The one-time discovery output may be retained as dated evidence; its scanner does not land as a guard.  
   **Evidence:** New subject manifest, rebuilt ownership ledger, stable receiving map, portable control receipt, and deletion record for the superseded scanners.

4. **Complete the package and semantic probes.** This may run concurrently with step 5.  
   **Done:** Exercise inferred projects, both trust states, external-code refusal/permission, declaration emit/maps, build, watch, incremental/`tsbuildinfo`, project-wide references across aliases/reexports, and auto-import edit composition. Repair probe 10’s counter and failure summary. Re-run all retained probes against the exact pinned package.  
   **Verified by:** Positive and negative controls per probe, consistent failure totals, bounded real-process watchdogs, zero unexpected skips, and exact package/version/digest checks.  
   **Evidence:** Fresh structured receipts and a regenerated transcript whose semantic portion is byte-reproducible; timings remain separately labelled run metadata.

5. **Repair the architectural contracts and handoffs.** This may run concurrently with step 4.  
   **Done:** Finish the framework diagnostic rules; implement the owner-sensitive mask function/table; atomize external-source cases; settle present deletion/survival rows; bind future accumulation; correct rows 25–26; bind position clamping; remove stale row-15, criterion-number, successor, charter-edit, and response-residue claims. Rewrite the summary and gap documents as references to the structured register, not second status stores.  
   **Verified by:** Exhaustive table recomputation, one-model-per-external-subshape validation, one disposition per deletion entry, direct receiving-criterion references, and absence of contradictory status prose.  
   **Evidence:** Corrected contract documents, regenerated tables, exact deletion/survival manifest, and updated TCM2–TCM4 criteria.

6. **Reconcile and prove readiness.**  
   **Done:** Join steps 4 and 5, regenerate every derived artifact, and account for all 36 findings: 32 closed and four mapped only to the three ratified residue records. No additional bounded claim may remain.  
   **Verified by:** The rebuilt validator emits `READY_FOR_MANDATES`; every generator is byte-stable on immediate rerun; all targeted probes and their negative controls pass; `node --check` passes for every changed `.mjs`; `git diff --check` passes. Do not run the workspace gate for this evidence-only block on this host.  
   **Evidence:** Final closure report listing derived status per atomic claim, proof receipts, residue bindings, and a zero-unaccounted-finding reconciliation table.

7. **Freeze the final substantive candidate.**  
   **Done:** Commit the reconciled state and call its literal SHA `R`. Bind `candidate_sha`, `candidate_tree`, charter digest, evidence digest, and the recursive subject-manifest digest to `R`; set TCM0 to `REVIEW` with all three mandates `PENDING`. Re-pin every changed ratified artifact.  
   **Verified by:** Use the literal range `557cc57b563084d00f8782ef5bd76e010c2a97d8..R`, never a branch name. The review subject must include all files under `evidence/TCM0/`, the summary, TCM0–TCM4 charters, the rescope act, and the relevant ledger/authority files—not merely the repair commit. The manifest must include every original 40-path subject member. Run the documented live program-state validator with authority and `effective-state.mjs`.  
   **Evidence:** One immutable review subject declaration containing `R`, its tree, the literal base SHA, exact paths and hashes, path count, and manifest SHA-256.

8. **Rerun all three mandates concurrently and blind against `R`.**  
   **Done:** Dispatch conformance, architecture, and adversarial reviews simultaneously with the identical frozen subject declaration. None sees another lane’s prompt, findings, or result.  
   **Verified by:** Each receipt contains `REVIEWED: R`, the same subject-manifest digest/path count, `RESULT: PASS`, and zero findings. Any substantive change after `R`, or any nonzero finding, invalidates all three and requires a new frozen candidate.  
   **Evidence:** Three independent PASS receipts. The known FAIL-binding rail defect does not weaken this step: PASS receipts can and must populate their three `reviewed_sha` fields with `R`.

9. **Record maintainer acceptance and verify the unlocked state.**  
   **Done:** In a ledger-only transition, record all three mandates `PASS` with reviewed SHA `R`, `maintainer_decision = "ACCEPTED"`, `status = "ACCEPTED"`, and `accepted_sha`/`accepted_tree` equal to `R`. No substantive TCM0 artifact changes after review.  
   **Verified by:** Run `node scripts/validate-program-state.mjs --dag docs/arch/refactor/rev11/program-dag.toml --state docs/arch/architecture-lock/ledger/program-state.toml --mode live --authority docs/arch/architecture-lock/ledger/authority-registry.toml`, then `node scripts/effective-state.mjs --dag docs/arch/refactor/rev11/program-dag.toml --state docs/arch/architecture-lock/ledger/program-state.toml --authority-registry docs/arch/architecture-lock/ledger/authority-registry.toml`. Both must succeed, TCM0 must derive `ACCEPTED`, and TCM1 must have no remaining TCM0 predecessor blocker.  
   **Evidence:** The accepted ledger record, three SHA-bound PASS mandates, accepted candidate identity `R`, and effective-state output showing downstream eligibility.

The only concurrency is the step-4/step-5 fork and the three blind reviews in step 8. Instrument repair, rescope ratification, final reconciliation, candidate freeze, and acceptance recording are serial gates.

===VERTER-RECEIPT-BEGIN===
LANE: tcm0-architect-ruling
RESULT: RULED
STEPS: 9
MUST_CLOSE: 32
RESIDUE: 4
===VERTER-RECEIPT-END===