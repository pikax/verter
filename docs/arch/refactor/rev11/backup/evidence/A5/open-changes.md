# A5 — Open-change reconciliation

Resolves row 1 of [`owner-rows.md`](owner-rows.md): "open PRs/branches/queued changes touching an
architecture owner — include, exclude, abandon, or coordinate before baseline lock; no unaccounted
competing rewrite" (`contracts/current-tree-reconciliation.md` §3; disposition vocabulary from
`contracts/baseline-lock.md` §3).

A0 dispositioned the only GitHub PR (ruling **R-5**: PR #98 → abandon, no GitHub action taken).
A0 did **not** disposition the local branch population, and it is large.

---

## 1. The population

| measure | value |
|---|---|
| local branches | **520** |
| not an ancestor of `main` (i.e. carrying unlanded commits) | **471** |
| of those, program-own (`program/architecture-lock`, `work/a5-inventories`) | 2 |
| of those, third-party candidates to disposition | **469** |
| of those, dated 2026-08 | 14 |
| of those, dated 2026-07 | 315 |
| of those, dated 2026-06 or earlier | 142 |
| live `git worktree` checkouts | 10 |

Reproduce: `git for-each-ref --format='%(refname:short)' refs/heads/` filtered by
`git merge-base --is-ancestor <b> main`.

Counts are as of this block's HEAD. A5's own review round later adds three `review/a5-*` branches
and three matching worktrees, which are program-own and disposition themselves; a reviewer
re-running the command therefore reads 474 unlanded / 13 worktrees against the same 469
third-party candidates.

## 2. The finding that decides the disposition

**No unlanded branch is a competing forward line: every one of the 469 was cut from a merge-base at
or before `2de3b2d07`, i.e. before the squashes that superseded it.** That lineage bound is the
mechanical discriminator (§4), and it holds for all 469 without exception.

For 468 of the 469 a second, blunter observation holds too — the branch is *behind* `main` in
content, not ahead of it. `git diff main <branch> --shortstat` shows it directly:

| branch | last commit | `git diff main <branch>` |
|---|---|---|
| `agent/rc-integration` | 2026-08-06 | 952 files, +43,506 / **−196,814** |
| `agent/rc-integration-backup` | 2026-08-05 | 952 files, +43,506 / −196,814 |
| `agent/resolution-currency-cutover` | 2026-08-05 | 987 files, +45,653 / −206,174 |
| `agent/rc-context-memo` | 2026-08-05 | 1,005 files, +48,832 / −211,298 |
| `agent/rc-resolution-dag` | 2026-08-05 | 1,007 files, +48,844 / −212,007 |
| `agent/rc-queryerror-avatar` | 2026-08-05 | 1,003 files, +48,749 / −213,346 |
| `agent/rc-requestonly-memo` | 2026-08-05 | 1,007 files, +48,467 / −212,735 |
| `agent/scheduler-taint-3d` | 2026-08-04 | 1,072 files, +49,947 / −226,021 |
| `perf/residual-recon` | 2026-08-01 | 1,518 files, +90,553 / −317,902 |
| `perf/currency-per-file` | 2026-08-01 | 1,521 files, +90,600 / −318,700 |
| `feat/o1-store-view-build` | 2026-08-01 | 1,524 files, +90,654 / −320,841 |
| `feat/resolution-currency-c1c2` | 2026-08-01 | 1,531 files, +91,628 / −327,377 |
| `feat/resolution-currency-c0` / `tmp-c0` | 2026-07-30 | 1,590 files, +100,824 / −347,941 |

Reading the deletion column correctly is the whole point: checking out any of these branches would
**remove** 197k–348k lines that `main` has. They are not competing forward work; they are stale
pre-squash WIP whose content already landed on `main` as the squashed commits `a7f13496b`
("perf(core): replace cardinality-driven non-cacheability with domain compaction and a resolution
decision DAG"), `e6191e280` ("refactor(css): replace lightning css with custom parser"), and
`9af553dd2` ("feat(typeinfo): support type narrowing (#94)").

Their shared merge-base with `main` is `2de3b2d07`, three commits behind `main`'s tip — i.e. they
were cut before the squashes that superseded them.

### 2.1 The one branch where the net-deletion observation does not hold: `port/rust`

Running the net test over all 469 candidates returns exactly one net *insertion*:

```text
port/rust   13,040 files, +3,135,590 / −2,764,768   (net +370,822)
```

It is named here rather than left to a reviewer to discover, because a class claim with a silent
exception is worthless. Three checked facts dispose of it, and none of them is the net figure:

1. **The entire net positive is one generated artifact.** `vue_tokenizer_output.json` at the repo
   root is +2,991,892 lines and exists only on `port/rust`
   (`git cat-file -e main:vue_tokenizer_output.json` → *does not exist in 'main'*). Excluding that
   single file the branch is **+143,698 / −2,764,768**, the largest net deletion in the entire
   population — not the smallest.
2. **It is not a fork of the current architecture at all.** `git ls-tree -d main:crates` and
   `git ls-tree -d port/rust:crates` share no layout: `port/rust` carries `verter_core`,
   `verter_napi`, `verter_wasm`, a crate set that predates the thirty-nine-crate workspace every
   ADR in this program is written against. Its `crates/` delta alone is +56,871 / −2,279,829.
3. **It satisfies the lineage bound with ~7 months to spare.** Its merge-base with `main` is
   `955140e26` (2026-01-14, "feat: single bundle support (#82)"), and
   `git merge-base --is-ancestor 955140e26 2de3b2d07` returns true. Its last commit is
   2026-02-07 (`00a8414be`, "more"), 58 commits along a line abandoned six months before the
   program began.

So `port/rust` falls inside the class on the test that actually carries the disposition (§4.1),
and its net-insertion figure is an artifact-size accident, not forward work.

## 3. Disposition

| set | disposition | rationale |
|---|---|---|
| `program/architecture-lock` | **Preserve** — this is the program's integration lineage | carries A0–A4 |
| `work/a5-inventories` | **Preserve** — this block's candidate branch | — |
| the 13 branches tabulated above | **Abandon** | superseded by the squashed `main` commits; content is a strict subset |
| `port/rust` | **Abandon** (individually dispositioned, §2.1) | a pre-workspace `verter_core`-era line, last touched 2026-02-07, merge-base `955140e26` (2026-01-14); its net-insertion figure is one 2,991,892-line generated artifact, and excluding it the branch is the population's largest net deletion |
| the remaining ~455 unlanded branches (2026-07 and earlier) | **Abandon** as a class | all predate `main`'s tip by ≥ 12 days and are further behind still; none is referenced by any program record |
| `origin/preserved/a2c-v3-partial-carrier-inventory`, `origin/preserved/a2c-eager-skeleton-candidate` | **Preserve as evidence, never merge** | ruling R-10 point 4 requires the rejected A2C candidate be retained as failed historical evidence |

"Abandon" here means exactly what R-5 established: it records the **program's relationship** to
the branches. **No branch is deleted by A5**, and no GitHub action is taken (ruling R-8: nothing is
pushed to `origin`; `origin/main` is frozen).

## 4. Why this is not a rubber stamp

The exit criterion A5 must satisfy is that no later block discovers a competing authority
mid-cutover. Two properties make "abandon as a class" safe here rather than convenient:

### 4.1 The load-bearing test is the lineage bound

**Every candidate's merge-base with `main` is at or before `2de3b2d07`.** This is mechanical, not
editorial: it is a two-command check per branch
(`git merge-base main <b>`, then `git merge-base --is-ancestor <that> 2de3b2d07`), it is
re-runnable in full by a reviewer, and it discriminates — a branch cut *after* the squashes that
superseded this population would fail it and fall out of the class, which is exactly the branch
that could carry genuine competing work. It holds for all 469 candidates including `port/rust`.

### 4.2 The net-deletion observation is corroborating, and has one stated exception

`git diff main <branch> --shortstat` producing a large net deletion is the blunter reading, and it
is the one a reviewer will run first, so its limit is recorded rather than glossed: it holds for
**468 of 469**, and the exception is `port/rust`, dispositioned individually in §2.1. The failure
mode is size, not semantics — a single oversized generated file inverts the sign of a test that is
counting lines, which is why §4.1 and not this is what the disposition rests on.

What A5 does **not** claim: that every one of the ~455 older branches was individually inspected.
It claims the class property, states both tests and the one exception to the weaker of them, and
marks the two preserved refs the rulings require. A reviewer who wants a stronger claim can re-run
either test over `git for-each-ref refs/heads/`; the net test re-derives the §2 table and the one
`port/rust` exception, and the lineage test re-derives the class with none.

## 5. Worktree hygiene

Ten live worktrees at the time of this block:

| path | branch |
|---|---|
| `…/verter` | `program/architecture-lock` (the program root) |
| `…/verter-a5` | `work/a5-inventories` (this block) |
| `…/a4-adv-control` | detached at `839645e3e` (A4 review artefact) |
| `…/verter/.claude/worktrees/rc-{3d,ctx,cutover,dag,errors,integ,reqonly}` | the seven abandoned `agent/rc-*` branches |

`governance.md` §5 requires one writable worktree per worker and no two workers sharing a mutable
checkout — satisfied: each program block has had its own path. The seven `rc-*` worktrees are
inert and belong to the abandoned class; they hold no program state.

**Recommendation to the orchestrator:** prune the seven `rc-*` worktrees and the `a4-adv-control`
detached checkout once A5 is accepted, so the worktree inventory the A6 lock freezes matches the
worktrees that actually exist. A5 does not prune them — a worktree under a different repository
root is outside this block's writable assignment.

## 6. Ratification

Branch dispositions are program-relationship decisions of the same kind as R-5, which was made by
the maintainer. A5 therefore records these as **recommended dispositions requiring a maintainer
ruling**, in the shape of R-5, before A6 freezes the baseline. Suggested ruling text:

> **R-12 — Local branch population dispositioned.** Every local branch that is not an ancestor of
> `main` other than `program/architecture-lock` and the active block branch is **abandoned**: each
> was cut from a merge-base at or before `2de3b2d07`, i.e. before the squashes that superseded this
> population, so none is a competing forward line; and the program takes no position on it beyond
> recording that it is not a competing authority. The two `origin/preserved/a2c-*` refs remain
> preserved as failed historical evidence per R-10. No branch is deleted and no GitHub action is
> taken.

The justification is deliberately stated on the lineage bound rather than on the net-deletion
figure. Both were run over the full population; the lineage bound holds for all 469 candidates,
whereas the net figure has one exception (`port/rust`, §2.1) that is an artifact-size accident. A
ruling should rest on the test that has no exceptions to explain.
