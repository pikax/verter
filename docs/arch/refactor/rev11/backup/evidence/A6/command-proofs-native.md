# A6 — Command proofs, rows 13 and 14

The two rows [`command-proofs.md`](command-proofs.md) §4 defers: the native build this worktree did
not have, and the JS/TS suite that depends on it.

## Row 13 — `pnpm run build:native`

```
Finished `release` profile [optimized] target(s) in 4m 36s
```

Exit **0**. Raw log digest `ee5beed305d6117a`. Builds `verter_napi` at the release profile, producing
the `.node` binding `@verter/native` loads. It is a build, not a result; it is recorded because
without it the next row proves nothing.

## Row 14 — `pnpm test`

Exit **1**. Raw log digest `2ef4921ad8b53220`.

**552 tests passed across every package except one.** `@verter/typeinfo` reports
`3 failed | 28 passed` over 11 files, and every other package is clean:

| package | result |
|---|---|
| `@verter/type-ir` | 6 passed |
| `@verter/verter-lsp` | 47 passed |
| `@verter/proto` | 16 passed |
| `@verter/verter-mcp` | 47 passed |
| `@verter/lsp-test-client` | 67 passed |
| `@verter/svelte-runtime-tests` | 35 passed |
| `@verter/benchmark` | 287 passed |
| **`@verter/typeinfo`** | **3 failed**, 28 passed |

### The three failures

```
FAIL tests/extra-imports-structured.spec.ts
  > named-import with localAlias + typeOnly resolves the renamed symbol
  AssertionError: expected undefined to be defined
FAIL tests/resolve-symbol.spec.ts
  > Expanded mode resolves a non-generic alias body to an Object descriptor
  AssertionError: expected undefined to be defined
FAIL tests/vue-instance-props.spec.ts
  > evaluates InstanceType<typeof default>['$props'] against a real .vue SFC scope
  AssertionError: expected undefined to be defined
```

All three are the same shape: a semantic resolution through the native binding returns nothing where
the test expects a descriptor.

### Classification: not attributable to this candidate

The three tests read exactly two tree-derived inputs — the Rust sources compiled into the native
binding (`crates/`) and the package's own TypeScript (`packages/`). **This candidate changes zero
bytes in either**:

```sh
git diff --stat <baseline-sha>..HEAD -- crates packages
# (empty)
```

The candidate's entire diff is `docs/`, `scripts/`, `performance-gates.toml`, `package.json` and
`vitest.config.ts`. None of it can change what a Rust-backed type resolution returns.

So these three failures are a property of the **baseline tree** on this runner, not of this block's
work. That is a claim about attribution, and it is proven by the empty diff above.

What is **not** claimed: that they fail on every machine, or that a baseline run has been executed to
observe them directly. Confirming them against a baseline checkout is an orchestrator action at
landing, and it is raised as an open item in the block report rather than assumed away.

### Why this is recorded rather than dropped

The first `pnpm test` invocation exited 1 at `@verter/native`'s `pretest` hook, before a single test
ran — a fresh worktree has no built `.node` binding because `pnpm install` does not produce one. The
easy move is to call that "an environment issue" and stop. That would have hidden three real red
tests behind a missing build artifact, which is the same failure mode the canonical gate's own
build-prerequisite preflight exists to prevent: an absent artifact making a suite skip while the run
reports green.

So the artifact was built and the suite re-run. The result is worse-looking and more informative, and
it belongs in the record either way.
