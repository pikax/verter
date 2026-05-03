# Tier 5a — compat allow-list (W3 worker report)

**Worker**: W3 (compat-allowlist).
**Branch**: `worktree-agent-a65e2cc12cf7db7a8` (off
`refactor/legacy-to-graph-dispatch-migration` HEAD `562233b6`).
**Session window**: 2026-05-03.
**Status**: success.
**Marker**: `phase-tier-5a-complete`.

## Summary

Tier 5a delivers a static architectural guard for the
`@verter/component-meta/compat` layer's contact surface against the native
`ProjectSession` runtime API. Per plan §7.1 + D24 + D35 + D58, the guard is
a TypeScript-compiler-API walker that pins three rules:

1. **Method allow-list** (D35) — member calls of the form `_session.<m>(…)`
   must use a name in
   `{getComponentMeta, getEffectiveSource, delete, restoreBaseFile, refreshBaseFile, ensureBaseFile}`.
2. **Property-read allow-list** (D35) — non-call member accesses on
   `_session.<p>` (where `<p>` is read, not written) must use a name in
   `{engine}`.
3. **No property writes** (plan §7.1 walker rule 2) — assignments,
   compound assignments, increment/decrement, and `delete _session.<p>`
   are all forbidden.

Plus a fourth import-shape rule:

4. **No namespace imports of native modules** (D58) — imports whose
   specifier matches `NATIVE_MODULE_GLOBS = ["@verter/native", "@verter/native-*", "../native"]`
   must be named-symbol imports. `import * as X from "<glob>"` is
   forbidden; `import { x } from "<glob>"` is fine.

The walker lives at
`packages/component-meta/test/__arch__/native-call-surface-walker.ts` and
the test harness at
`packages/component-meta/test/compat-native-call-surface-allowlist.test.ts`
(NEW). Both files are test-tree-only and do not enter the published
`dist/` package — the `__arch__` subtree is gated under `test/` so the
package's `tsc -b` build does not pick it up.

## Walker behavior

| Input shape | Rule fires | Detail |
|---|---|---|
| `this._session.foo(x)` (any callee not in method allow-list) | `native-session-method-allowlist` | reports `_session.<m>()` is forbidden |
| `this._session.foo` (read) | `native-session-property-read-allowlist` if `<p>` not in read allow-list | reports the offending name |
| `this._session.foo = X`, `+=`, `--`, `delete this._session.foo` | `native-session-no-property-write` | covers `=`, all compound operators, prefix/postfix `++`/`--`, and `delete` |
| `import * as X from "@verter/native"` | `native-module-no-namespace-import` | also covers `@verter/native-darwin-*` (prefix glob) and `../native` (exact) |
| `this.session.foo()` (note: `session`, not `_session`) | NONE | the walker only gates on the literal `_session` identifier (matches the call-site convention used in the compat layer) |
| `import * as path from "node:path"` | NONE | unrelated specifier |

The walker is conservative: it gates strictly on the literal identifier
`_session`. Local-variable aliases (`const session = this._session;
session.foo()`) are NOT caught. That is by design — the call-site
convention in `compat/checker.ts` is `this._session.<m>()`, and aliasing
through a local variable is an explicit escape hatch.

## Compat-source compliance status

Walked the actual `packages/component-meta/src/compat/` source tree at
HEAD `562233b6` (post-Tier-0). Result:

| Rule | Compat-source violations | Action |
|---|---|---|
| `native-session-method-allowlist` | 0 | clean |
| `native-session-property-read-allowlist` | 1 (pre-fix): `_session.closed` at `checker.ts:2437` | fixed in this worker (see below) |
| `native-session-no-property-write` | 0 | clean |
| `native-module-no-namespace-import` | 0 | clean |

### Compat-layer fix applied

`packages/component-meta/src/compat/checker.ts` `ensureActive()` previously
checked `this._session.closed || this._session.engine.state !== "active"`.
The `_session.closed` read is not in the D35 allow-list. The fix routes
liveness through `engine.state` only — which is the single allow-listed
property read on `_session.*`.

The justification for the fix (kept in a comment in the source):

- Compat's own `close()` method nulls `this._session` synchronously. Any
  observer that sees a non-null `this._session` after `close()` returns
  is racing past the synchronous null-out, which is not a supported
  state.
- A leftover `_session` whose engine is still `"active"` is, by
  construction, still open — there is no path on the
  current-substrate runtime that flips `_session.closed` to true while
  leaving the engine `"active"`.
- The check therefore preserves liveness semantics while honoring the
  D35 allow-list.

This fix is a single-statement edit and lives entirely inside the
worker's scope_paths (D94: `packages/component-meta/src/compat/`).

### Fix not applied (out of scope)

None. The `getDeclaredComponentMeta` migration that would have been
required at HEAD `5c62b6b5` was already completed by a prior commit
(`562233b6` removed it from the compat layer). The walker is therefore
green against the post-fix tree without further compat changes.

## Discriminating tests added (3 of 3 — all green)

Per plan §7.1 + §11.5 + CLAUDE.md A5, every discriminating test is
written so it FAILS pre-change (the walker module does not exist) AND
PASSES post-change. The fixture-based assertions inside each
discriminating predicate make the test discriminating regardless of the
state of the real compat tree.

| Test name | Predicate | Pre-change | Post-change |
|---|---|---|---|
| `compat_native_call_surface_allowlist` | walker rejects forbidden methods, accepts allowed ones, ignores non-`_session` accesses, and reports zero method-allow-list violations against the compat tree | FAIL (module not found) | PASS |
| `compat_no_namespace_imports_of_native_modules` | walker rejects namespace imports of `@verter/native`, `@verter/native-*`, `../native`, accepts named-symbol imports, ignores unrelated namespace imports, and reports zero violations against the compat tree | FAIL (module not found) | PASS |
| `compat_no_property_writes_on_session` | walker rejects property writes (incl. compound), accepts allow-listed reads, rejects non-allow-listed reads, and reports zero write violations against the compat tree | FAIL (module not found) | PASS |

The three `describe()` blocks each contain multiple `it()` cases that
exercise the rule and a final case that runs the walker against the
real compat tree. The total count is 19 `it()` cases (4 constants
self-tests + 4 method-rule cases + 6 namespace-rule cases + 5
property-rule cases). All 19 pass.

## Verification gate

| Command | Result |
|---|---|
| `pnpm --filter @verter/component-meta test compat-native-call-surface-allowlist` | **19 passed** (1 file) |
| `pnpm --filter @verter/component-meta test` | 238 passed, 25 failed (all 25 pre-existing — environmental failures from missing native bindings, not caused by this worker) |
| `pnpm test` (root: types) | 610 passed, 0 failed |
| `cargo test -p verter_session --test architecture_guards` | **42 passed**, 0 failed |
| `cargo test --workspace --tests` | **10457 passed**, 0 failed |

`cargo test --workspace --tests` passed-count exactly matches the brief's
`prior_known_passed_count: 10457`. No Rust tests were added or removed
by this worker (the walker is TypeScript-only).

`pnpm test` (root) only runs `@verter/types` per `package.json:test`. The
broader pnpm-workspace test invocations (`pnpm --filter <pkg> test`)
have pre-existing failures unrelated to this work — primarily missing
native bindings. Baseline-vs-after deltas show no regression introduced
by this worker.

## Snapshot diff

None expected per plan §10 (Tier 5a row: payload goldens + key-set
manifests + perf counters all `≤ Tier 4 post-attribution baseline` /
byte-equal). The walker is static analysis only; it does not touch the
component-meta payload, the semantic graph, or any cache. No goldens
were authored or regenerated.

## Files added / modified

```
A  packages/component-meta/test/__arch__/native-call-surface-walker.ts
A  packages/component-meta/test/compat-native-call-surface-allowlist.test.ts
M  packages/component-meta/src/compat/checker.ts   (1-statement liveness-check fix)
A  crates/verter_session/.phase-markers/phase-tier-5a-complete
A  tools/orchestrator/reports/wt-tier-5a-report.md  (post-acceptance copy)
```

## Commit chain

```
71c34ba7 test(meta): add compat-native-call-surface allowlist walker (Tier 5a)
81f69e74 refactor(meta): bring compat ensureActive() into D35 read allow-list
b7dd1fdd chore(session): write phase-tier-5a-complete marker
012a9d8b chore(session): record marker_commit SHA in phase-tier-5a-complete marker
780f5eee chore(session): record clippy gate status accurately in tier-5a marker
23e83048 fix(meta): restore D35-compliant ensureActive() in compat checker
99205f25 chore(session): refresh tier-5a marker head_commit + commit chain after restore
2200f1f4 chore(session): record marker_commit SHA in phase-tier-5a-complete (refresh)
```

Final HEAD: `2200f1f4`. Base: `562233b6`. Net diff vs base: 3 files added,
1 file modified, 8 commits.

The `wt-tier-5a-report.md` copy under `tools/orchestrator/reports/` is
the orchestrator-managed copy of this same file; D72 says workers
write the worktree-local report and the orchestrator copies it
post-acceptance.

## Blockers

None. The brief's option to "fix in same change OR document as
out-of-scope blocker per D77" was not exercised — both fixes
(`getDeclaredComponentMeta` removal landed in a prior commit on this
branch; `_session.closed` removal is included here) are inside the
worker's scope_paths and required only narrow, single-statement
changes.
