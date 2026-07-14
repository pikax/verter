# Orchestration-rules block — state at hand-off

**Historical branch:** `block/orchestration-rules`.
**Shape:** exactly ONE commit off `c6f50174d`. Unpushed, unmerged, no attribution trailer.
**Governing decision:** *the rules LAND, the unbuilt machinery is CUT.* A protocol that mandates a
command which does not exist is worse than the working one it replaced — every leg fails the
contract, so the contract gets ignored.

## What this block established (do not re-litigate)

The rules are stated at full strength; the machinery they cannot yet back is TRACKED, not mandated.
Everything cut has a row in `docs/arch/gate-integrity-ledger.md` with an owner, a resolution gate,
and named acceptance tests. Read that ledger before touching any of it.

**Three mandated rules were found to be non-implementable as written, by RUNNING them rather than
reading them.** Each is now corrected in the rule text, and each is the same defect the block exists
to close — *an assertion about a mechanism, confidently made, false the moment it is executed*:

1. **`taskkill //F //T //PID "$!"` kills nothing.** `$!` is the MSYS pid; `taskkill` needs the
   Windows pid (`/proc/<pid>/winpid`). Fed `$!` it prints `process not found`, exits 128, and the
   tree survives. The "terminate only your own recorded PID tree" rule was a NO-OP on this platform.
2. **`taskkill //F //T` does not reap the tree.** With the correct winpid it prints
   `SUCCESS … terminated` and exits 0 while every descendant keeps running (observed 3/3 survived).
   Leader-termination is not containment, and confirming the leader is a FALSE GREEN.
3. **A naive banner check fails closed on the truth.** The CLI prints `Reading prompt from stdin...`
   and a version line BEFORE the banner, so anchoring on "the first line is a rule" REJECTED EVERY
   REAL LEG. A check whose failure modes are understood only by its author is not a validator.

## Verification harness (KEEP — this is the shape that works)

`bash <scratchpad>/verify.sh` — extracts every ` ```bash ` fence from the changed files and EXECUTES
the load-bearing ones with discriminating positive AND negative controls. 42/42 at hand-off.
The load-bearing negative control: strip `errexit`/`pipefail` from the gate fence and the same red
run exits **0** — proving both options are load-bearing rather than decorative.

Rebuild it if lost. Reading a fence is not testing it; three landed rules were false and only
execution found them.

## Gates at hand-off

- Snippet harness: **42/42**.
- R6 meta-guard (`every_critical_rule_in_docs_has_registered_guard`): **7/7**.
- `tracked_paths_are_portable`: **12/12**.
- Unprimed codex review (`gpt-5.6-sol`, `xhigh`, banner verified, verdict artifact read — never
  grepped): **CHANGES REQUIRED**, zero P0s. The last three P1s are FIXED in this commit
  (`process_table` discarding `ps`'s exit status; `WAIT-PROTOCOL` using `kill -0` as an exit oracle
  that `PROTOCOL` calls unsound; "Never push" vs. the prescribed force-push operation).

**The review has never returned APPROVED.** Successive rounds fixed everything found and the next
round found new, finer items in ~60KB of adversarially-reviewed prose. That is the honest state:
zero P0s for the last several rounds, and a residual tail of P1/P2 wording-and-mechanism findings.
Do not represent this as a clean approval — governance requires one before ADOPTION, and it does
not exist yet.

## TODO for the next agent

1. **Re-run the unprimed review** against the final tree. Fix what it finds. It BLOCKED every round
   so far; treat "no P0s" as progress, not as clearance.
2. **The two escalated decisions are the USER'S, not an agent's** (ledger GI-14 / GI-15). Both have
   an operative default written into the rules so the protocol is decidable TODAY, and both must be
   ruled before the gate-integrity block lands:
   - May an ATTESTATION authorize Agent dispatch? (operative today: yes, risk-accepted, not a proof)
   - Red baseline vs. green gate? (operative today: STRICT — a nonzero gate BLOCKS, no exclusion)
   A user ruling does NOT bypass the governance gate: the rule-text edit encoding it still needs
   prior neutral codex approval.
3. **The gate-integrity block is the owner of every cut mechanism** — the launcher (GI-5), the
   containment object (GI-6), the verdict grammar (GI-7), the banner validator (GI-8), the
   capability probe (GI-9), the red-gate exclusion (GI-11), stale-lock recovery (GI-12), the history
   purge (GI-13). It also owes `gate_contract_integrity` (GI-3) and the promotion of
   `Verification Must Prove Execution` from `(MANDATORY)` to `(CRITICAL)` (GI-4) — which cannot land
   before its guard exists, or the meta-guard fails.
4. **Containment is the highest-value item.** Enumerate-and-confirm is a stopgap with four disclosed
   residuals (GI-6). A Windows Job Object / Linux cgroup makes containment a property of the child
   instead of a search of the process table. macOS has no kernel-enforced equivalent — its bar is a
   supervisor process, and that limit is stated rather than hidden.

## Rules of engagement that were earned the hard way

- **A leg whose output artifact you cannot produce DID NOT RUN.** Grep is diagnostic only. The
  transcript is contaminated by construction — it contains the prompt echoed back — so a scan can
  hand back a verdict the leg never rendered (a timed-out leg was observed carrying 35 occurrences
  of `__DONE__` and no final message).
- **A green planted run means the plant failed until proven otherwise.** `perl`, `sed` and `grep`
  all exit 0 on a non-match.
- **Never a name/pattern/port kill** (`taskkill /F /IM`, `pkill`, `killall`, `Stop-Process -Name`) —
  it reaches sibling legs and the user's own sessions. Recorded-tree only.
- **Scoped `git add` by path.** Never `git add -A`. No PM/plan vocabulary. No attribution trailer.
- **The half-applied fix is the recurring failure mode of this work.** Four separate times a defect
  was fixed in one file and left live in another. After every fix, grep the whole tree for the
  pattern you just corrected.
