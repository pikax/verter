# Freshly resolved state — 2026-08-12

All identities below were resolved during package preparation; no dispatch-supplied
SHA or status was treated as authority.

## Writable package worktree

- path: `<PACKAGE_WORKTREE>` (the dispatched `verter-rescope-bf` worktree)
- branch: `work/framework-conformance-rescope`
- post-rebase base commit: `b3249d13d07806a14a4307954dfcc459cf7301ac`
- post-rebase base tree: `57e412549c24c903877b471000569c99591a49fc`
- reviewed pre-fix candidate: `ce1d0e4688af1b5bd548b6b68286632cc0f7ede8`
- reviewed pre-fix tree: `1ff1f83d8e994b6f1169b0b209c9f557c23f4728`
- local `program/architecture-lock`: the same base commit/tree
- `origin/program/architecture-lock`: `ff3728e1768d5ad09123c2221e3847150c6d9723`

The local program line contains the accepted B1 implementation and its recorded
ledger transition. This package is rebased directly onto that local integration
authority.

## Protected worktrees

`<PRIMARY_WORKTREE>` (the dispatched `verter` path) was observed only through
read-only Git inspection. It was on `program/architecture-lock` at
`b3249d13d07806a14a4307954dfcc459cf7301ac`; it was not written.

The former B1 worktree no longer exists, and `refs/heads/work/b1-neutral-contracts`
is absent. B1 was accepted at
`03b2fdbfc6d12452824768d9e389a5f6f3d680df`, tree
`7f8230066735db17650b5d594a95d597540b3729`; the following
`b3249d13d07806a14a4307954dfcc459cf7301ac` ledger commit records that acceptance.
No B1 file, evidence, index, or status is changed by this package.

## Program facts before amendment

- A0–B1: `ACCEPTED` in the tracked live ledger.
- B1: accepted SHA/tree exactly as recorded above; no separate worktree or branch.
- amended DAG/template/live block universe: 56 identical IDs.
- template validation: PASS.
- live validation: PASS.
- amended DAG digest:
  `335e0863ba1f21473a24befc0093dc01bad4f065ff03e6716c113448be054489`.

The ledger's historical top-level `current_block` field is not rewritten by this
proposal. B1's accepted identity comes from its ledger row, not from a removed
worktree.

## Existing compatibility facts

The root development manifest pins Vue/compiler-sfc `3.5.34`, Svelte `5.56.3`, and
TypeScript `7.0.2`. The existing Vue conformance package pins `3.6.0-rc.1`; existing
Svelte oracle/goldens use `5.56.3` and include known-divergence machinery. These are
historical domains only. They cannot supply AMD-005 expectations or acceptance.

The repository also contains workspace TypeScript declarations at exact `6.0.3`, a
bundled TSGO domain at `7.0.2`, and an exact native-preview build-tool dependency.
AMD-005 preserves those distinct owners.

## Commands

Identity resolution used read-only `git rev-parse`, `git status`, `git worktree list`,
`git tag`, `git show`, and source searches. Initial program validation used:

```sh
node scripts/validate-program-state.mjs --dag docs/arch/refactor/rev11/program-dag.toml \
  --state docs/arch/refactor/rev11/templates/program-state.template.toml --mode template
node scripts/validate-program-state.mjs --dag docs/arch/refactor/rev11/program-dag.toml \
  --state docs/arch/architecture-lock/ledger/program-state.toml --mode live
```

No compiler build, Cargo test, Node test, benchmark, or gate was run to prepare this
state record.
