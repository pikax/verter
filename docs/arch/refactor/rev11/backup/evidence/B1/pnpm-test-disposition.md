# `pnpm test` failure disposition

**Candidate tree checked:** `f69c03b63b037e37da01f5824b4a477d0cbb871d` (this block's tip at the time
of this note).
**Baseline tree checked:** `ff3728ec069a8fd2f8d427687fcf33bf88ea9f44` (the block base — the accepted
tip of `program/architecture-lock` this block was dispatched against).

## Claim

The `@verter/typeinfo` vitest failures and the `vue-conformance-oracle` `check:style-pseudo` line
recorded in `command-proofs/07-pnpm-test.txt` are **pre-existing at the block base and unrelated to
this block's diff**. Two separate findings:

1. The three `@verter/typeinfo` failures are genuinely pre-existing — byte-identical assertion
   failures reproduce at the baseline SHA.
2. The `vue-conformance-oracle` `check:style-pseudo` line in the captured log is **not a real
   failure of that check** — it is `pnpm -r --parallel`'s first-fail teardown noise. The command
   itself passes cleanly, standalone, on both the baseline and the candidate tree.

Neither disposition required any change inside this block's allowed write set; the failures are
outside the closure `verter_identity` touches (this block's diff is Rust-only: a new
`verter_identity` crate plus `verter_scheduler`/`verter_session` test/doc changes — see `git diff
ff3728ec0 f69c03b63 --stat`, zero TS/JS/napi/wasm files touched, and `verter_identity` has zero
consumers anywhere in the workspace: `grep -rln verter_identity crates/*/src` returns only
`crates/verter_identity/src/lib.rs` itself).

## Finding 1: the three `@verter/typeinfo` vitest failures are pre-existing

### Proof

Reproduced in a scratch worktree checked out at the exact baseline SHA:

```sh
git worktree add --detach /tmp/verter-b1-baseline ff3728ec0
cd /tmp/verter-b1-baseline
pnpm install --frozen-lockfile
```

The native `.node` binding and the `@verter/type-ir` TS build artifacts are prerequisites the
scratch worktree does not carry from a bare checkout. Since `verter_identity` has zero consumers
(above), the native binding built from the candidate tree is behaviorally identical to one built
from the baseline tree for every code path these tests exercise, so the already-built
`packages/native/dist/` from the candidate worktree was copied in verbatim (no Rust recompilation
avoided any semantic difference — the crate that changed is unreferenced) rather than re-running a
full native release build under this session's resource cap:

```sh
mkdir -p /tmp/verter-b1-baseline/packages/native/dist
cp -R <workspace-root>/packages/native/dist/* /tmp/verter-b1-baseline/packages/native/dist/
```

`@verter/type-ir` and the rest of the TS build graph were built genuinely from the baseline source:

```sh
cd /tmp/verter-b1-baseline
pnpm run build:ts
```

Then the exact three failing specs were run directly:

```sh
cd /tmp/verter-b1-baseline/packages/typeinfo
npx vitest --run tests/extra-imports-structured.spec.ts tests/resolve-symbol.spec.ts tests/vue-instance-props.spec.ts
```

### Result (baseline, `ff3728ec0`)

```
 FAIL  tests/extra-imports-structured.spec.ts > typeinfo evaluate with structured ImportSpec > named-import with localAlias + typeOnly resolves the renamed symbol
AssertionError: expected undefined to be defined
 ❯ tests/extra-imports-structured.spec.ts:58:19

 FAIL  tests/resolve-symbol.spec.ts > TypeInfoSession.resolveSymbol > Expanded mode resolves a non-generic alias body to an Object descriptor
AssertionError: expected undefined to be defined
 ❯ tests/resolve-symbol.spec.ts:42:19

 FAIL  tests/vue-instance-props.spec.ts > TypeInfoSession Vue instance props worked example > evaluates InstanceType<typeof default>['$props'] against a real .vue SFC scope
AssertionError: expected [] to include 'msg'
 ❯ tests/vue-instance-props.spec.ts:61:29

 Test Files  3 failed (3)
      Tests  3 failed | 4 passed (7)
```

Same three specs, same assertions, same failure lines, same error messages, and the same passing
count for the surrounding tests as `command-proofs/07-pnpm-test.txt` records for the candidate tree
(`Test Files 3 failed | 8 passed (11)` across the full `packages/typeinfo` suite; `3 failed | 28
passed (31)` tests — the 3-spec subset run here shows `4 passed` because it excludes the other 8
passing spec files, not because the passing count differs).

### Disposition

**Pre-existing, unrelated to this block.** Escalate as inherited debt; not fixed here — fixing it
would mean editing `@verter/typeinfo` source, which is outside this block's allowed write set
(`docs/arch/refactor/rev11/evidence/A6/B1-context-packet.md` §4) and is scope creep the packet
forbids (§5: "No scope widening or unrelated cleanup. A defect found outside the closure is reported
with its evidence, not fixed here.").

## Finding 2: the `vue-conformance-oracle check:style-pseudo` line is teardown noise, not a real failure

### Proof

`command-proofs/07-pnpm-test.txt` shows the top-level `pnpm test` invocation (`pnpm -r --parallel run
test`) hitting `ERR_PNPM_RECURSIVE_RUN_FIRST_FAIL` on `@verter/typeinfo` (line 378 of that file), then
immediately printing the `vue-conformance-oracle` package's `check:style-pseudo` command banner (lines
418-419) followed directly by `ELIFECYCLE Test failed` / `EXIT: 1` with **no** actual check output
(no `goldens check OK`, no row-count line, no assertion) between the banner and the exit. That is the
signature of `--parallel` tearing down the still-running `vue-conformance-oracle` job on the first
sibling failure, not of `check:style-pseudo` completing and failing.

Run standalone, in isolation, on both trees:

```sh
cd packages/vue-conformance-oracle
node gen-vue-goldens.mjs --check
node gen-vue-style-pseudo-oracle.mjs --check
```

**Baseline (`ff3728ec0`, `/tmp/verter-b1-baseline`):**
```
vue conformance oracle: vue@3.6.0-rc.1, @vue/compiler-dom@3.6.0-rc.1, @vue/compiler-sfc@3.6.0-rc.1, @vue/compiler-vapor@3.6.0-rc.1, esbuild@0.28.0
goldens check OK: 286 committed artifacts match a fresh run
EXIT check: 0
Vue style pseudo oracle: 49 rows match @vue/compiler-sfc
EXIT style-pseudo: 0
```

**Candidate (`f69c03b63`, this worktree):**
```
vue conformance oracle: vue@3.6.0-rc.1, @vue/compiler-dom@3.6.0-rc.1, @vue/compiler-sfc@3.6.0-rc.1, @vue/compiler-vapor@3.6.0-rc.1, esbuild@0.28.0
goldens check OK: 286 committed artifacts match a fresh run
EXIT check: 0
Vue style pseudo oracle: 49 rows match @vue/compiler-sfc
EXIT style-pseudo: 0
```

Both exit 0 on both trees.

### Disposition

**Not a real failure — pre-existing `--parallel` first-fail teardown artifact, present on both
trees, unrelated to this block.** No fix required; nothing in this block's diff touches
`vue-conformance-oracle` or its inputs. The underlying blocker for a genuinely green top-level `pnpm
test` is Finding 1 (the real `@verter/typeinfo` failures), which is the same inherited debt.
