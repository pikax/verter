# C1 source-policy/evidence authority consult

Act as an unprimed architecture authority. Inspect the repository and current
candidate `f8191bac45436d6618d397866d206c1898dab376` directly. Do not assume the
orchestrator's diagnosis or preferred remedy.

## Question

C1 is otherwise approaching final verification, but the exact command

```text
cargo nextest run -p verter_source_policy_gate -E 'test(tracked_files_contain_no_machine_specific_path_markers)' --no-fail-fast
```

reports nine tracked files containing the guard's known developer-home marker.
Seven are C1 diagnostic or authority prompt/raw-output artifacts. Several are
digest-bound by registered architecture rulings. Two are pre-existing trunk
rulings unrelated to C1. The guard claims that no tracked file except its own
source may contain any known marker.

Determine the actual invariant, whether this is a candidate defect, a
governance/evidence defect, or a guard-scope defect, and prescribe the smallest
sound operative resolution. Do not silently weaken the portability invariant,
rewrite digest-bound evidence without re-ratification, or make C1 absorb
unrelated cleanup without ruling on ownership.

Non-exhaustive options to assess:

1. Keep exact evidence in-tree and introduce a structurally narrow,
   content-addressed evidence exception.
2. Move raw exact artifacts outside the tracked tree while retaining an
   in-repo digest/manifest and sufficient reproducibility evidence.
3. Normalize or recreate the artifacts and formally re-ratify every affected
   digest and ruling.
4. Scope this guard to production/portable artifacts and use a separate
   integrity mechanism for authority evidence.
5. Reject all of these and state a better remedy.

Inspect the complete nine-file inventory, relevant rulings/registries, and the
guard implementation. Distinguish immediate C1 acts from trunk/program acts.
Give exact required edits, re-registration/digest consequences, tests, and
mutation proof if any. State whether C1 may proceed after those acts.

End with exactly one receipt:

```text
LANE: c1-source-policy-evidence-authority
REVIEWED_SHA: f8191bac45436d6618d397866d206c1898dab376
VERDICT: PASS|FAIL
BLOCKERS: <none or concise list>
OPERATIVE_ACTS: <ordered concise list>
RATIONALE: <concise invariant-based rationale>
```
