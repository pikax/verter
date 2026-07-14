# Block 1 (next) — Gate Integrity

**Status:** ratified as the next block. **User-ruled to go BEFORE the architecture block**, reversing an earlier
recommendation.

## Why this goes first

The architecture block's acceptance criteria are literally *"tests that discriminate the actual seam."* **This
codebase's gates were caught certifying falsehoods at least three times** — two theatre gates; three of nine
"TDD with a RED-before proof" fixes guarded by tests that pass with the feature removed; a decision suite that
**asserted the bug as correct**; and a base failing-count that was wrong every single time it was quoted, because
nobody had provisioned the fixtures the suite needs to run at all.

**We would otherwise be verifying the largest change in the plan with instruments we know are lying.** Fixing the
gates first is what makes the next block's acceptance mean anything.

## Authority

**[`../gate-integrity-ledger.md`](../gate-integrity-ledger.md) is the authority** for what this block owes. Every
mechanism cut from the orchestration-rules commit has a row there with an **owner**, a **resolution gate**, and
**named acceptance tests**. Read it before touching any of it.

## Scope

### A. Make gates prove they executed

- A **tree-derived verification-surface manifest** — the runner **must not be able to narrow its own universe**.
  (Attestation alone is insufficient: *a receipt faithfully attests whatever incomplete universe the runner
  defines for itself.* The design needs **execution attestation + independent discovery parity + per-surface
  mutation proof.**)
- An **attesting canonical driver** and an **`if: always()` CI aggregator**.
- **`gate_contract_integrity`** (GI-3) with its mutation cases.
- **GI-4:** promote **`Verification Must Prove Execution`** from `(MANDATORY)` to `(CRITICAL)`. **This cannot land
  before its guard exists** — an unguarded `(CRITICAL)` heading fails the R6 meta-guard
  (`every_critical_rule_in_docs_has_registered_guard`). *The rule whose thesis is "a gate that cannot prove it ran
  is a failure" currently ships at the one tier the meta-guard does not check. Close that.*

### B. Fix the gates that are known to be lying

- **Two `if: false` required E2E jobs** in `.github/workflows/ci.yml` (dead since 2026-03-16; with the harness
  defect below, there has been **no e2e gate at all** for months).
- **The closed spec universe** — `packages/vue-vscode/package.json` hand-lists 8 of 20+ tracked specs;
  `activationGate.spec.ts` and ~11 others sit in **no gate at all**.
- **Raw source-text `.contains()` guards** — they produced two theatre gates and violate Verter's own
  no-string-based-semantic-logic rule. A guard that greps for a literal also matches that literal **in a doc
  comment**.
- **The gitignored fixture `node_modules`** (`packages/vue-vscode/e2e/fixtures/single-project/node_modules` and
  siblings). Without them ~7 real-provider tests `return` early and **score PASS with zero assertions**. This is
  why every baseline number was wrong. Solve it properly — provisioning must be part of the gate, not folklore.
  `VERTER_REQUIRE_TSSERVER=1 VERTER_REQUIRE_TSGO=1` already exist to make the skip hard-fail; **the gate should
  set them.**
- **The VS Code e2e harness scores a MISSING run-summary as a PASS** (the launcher returns before the extension
  host finishes). An untouched fixture reported `PASSED` with **zero tests observed**.
- **A helper-timeout inversion** — 20,000 ms budgets under a 15,000 ms mocha timeout.

### C. Build the mechanisms the rules mandate but do not have

From the ledger: the **launcher (GI-5)**, the **containment object (GI-6)**, the **verdict grammar (GI-7)**, the
**banner validator (GI-8)**, the **capability probe (GI-9)**, the **red-gate exclusion (GI-11)**, **stale-lock
recovery (GI-12)**, the **history purge (GI-13)**.

**Containment (GI-6) is the highest-value item**, and it is not optional hygiene — the naive form is *proven* to
be a no-op that reports success:

- **`taskkill //F //T //PID "$!"` kills nothing.** In Git Bash `$!` is the **MSYS pid**, not the Windows pid
  (`/proc/<pid>/winpid`). It prints *"process not found"*, exits **128**, and the tree survives.
- **`taskkill //F //T` does not reap descendants — and lies.** Given the *correct* winpid it prints
  `SUCCESS … terminated`, exits **0**, and **every child keeps running** (3/3 observed). **Confirming the leader
  is a false green.**

A **Windows Job Object / Linux cgroup** makes containment a **property of the child** rather than a *search of the
process table*. **macOS has no kernel-enforced equivalent** — its bar is a supervisor process, and the ledger
states that limit rather than hiding it. Enumerate-and-confirm is a stopgap with four disclosed residuals.

### D. The 5 pre-existing failures — but FIRST, re-establish the baseline

**Do not start from `5 of 118`. Establish it.** The tree that number was measured on contained **no tsgo** — no
`@typescript/native-preview` in the fixture or at root, no tsgo binary anywhere — so the claimed **54 tsgo** tests
cannot have sourced their engine from it. They either took it from **elsewhere on the machine** (unverified, and
if it was the Native Preview install, the baseline is entangled with the very bug under investigation) or they
did not run. **A baseline whose engine provenance is unknown is not a baseline** — and this is the fourth time
this number has moved. **First deliverable of this section: re-measure, and record WHERE EACH ENGINE CAME FROM.**
It is the same class as the gitignored fixtures below, one layer down: a suite that reports a count without
proving what it actually executed against. There is **no pre-provisioned tree** to inherit — see the provisioning
recipe in [`README.md`](README.md), and note that a *missing* fixture makes tests silently PASS with zero
assertions rather than failing.

The base then fails some set of real-provider tests (last measured: **5** — completion ×2, hover, rename, a
completion/edit race). Under the **STRICT** gate default (see
[`04-open-decisions.md`](04-open-decisions.md), GI-15) **a nonzero gate blocks with no exclusion list** — so this
block must **fix or formally disposition** those 5 (owner + gate per row) before anything else can land behind a
green gate. **This is the bootstrap. Do it deliberately, not by adding an exclusion list** — an exclusion list is
precisely the mechanism by which a gate quietly stops testing things.

## Method that works — keep it

**Extract every fenced snippet from the committed files and EXECUTE it**, with discriminating **positive AND
negative controls**. The orchestration-rules block's harness (42/42) proved three landed rules false; **reading
them had proved nothing**. Its load-bearing negative control strips `errexit`/`pipefail` from the gate fence and
shows the same red run then exits **0** — proving both options are load-bearing rather than decorative.

**And prove your own harness is non-vacuous.** That same harness, on its first run, reported **"ALL FENCES PARSE"
having parsed none** — its extraction wrote hidden dotfiles that a `*.sh` glob never matched. *Assume your harness
can do this to you, and prove it didn't: report fences found / executed / passed.*

## Acceptance

- Every gate in the tree **proves it executed** (non-zero executed count, resolved substrate, matched project,
  present build artifact).
- **Revert the wiring of any gate and it must go RED.** A gate that stays green when the thing it tests is removed
  is not a gate.
- `gate_contract_integrity` lands with its mutation cases, and **GI-4's promotion to `(CRITICAL)` lands with it**.
- The two `if: false` jobs are live or explicitly retired with an owner and a gate.
- No test surface exists that CI does not run.
- The 5 pre-existing failures are fixed or dispositioned.
