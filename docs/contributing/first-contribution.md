# First Contribution Walkthrough

An end-to-end walkthrough of the smallest real contribution: adding a lint
rule to `verter_diagnostics`. It exercises the whole loop — failing test
first, minimal fix, canonical gate — while teaching the interfaces you are
allowed to build on. The ownership rules behind every "don't" below are on
the [architecture contracts](./architecture-contracts.md) page.

## What you will touch

| Concern          | Owning interface                                             |
| ---------------- | ------------------------------------------------------------ |
| Rule contract    | `LintRule` trait in `crates/verter_diagnostics/src/rules/mod.rs` |
| Registration     | `register_builtin_rules` in the same file                    |
| Rule context     | `crates/verter_diagnostics/src/context.rs` (`LintContext`)   |
| Diagnostics      | `crates/verter_diagnostics/src/diagnostic_set.rs` (`DiagnosticSet`) |
| Source positions | `crates/verter_span/src/lib.rs` (`Span`, SFC-absolute bytes) |

An existing rule to copy from:
`crates/verter_diagnostics/src/rules/reactivity/no_ref_as_operand.rs`
(inline `#[cfg(test)]` module included).

## Step 1 — write the failing test first

Open the rule file you will create under
`crates/verter_diagnostics/src/rules/<category>/` and start with the test
module, not the implementation. A rule test needs both assertions — see
the [Testing Guide](./testing.md) for the repository-wide pattern:

- a positive case: source that must produce the diagnostic (assert the
  rule name and the reported `Span` you expect);
- a negative case: near-identical source that must NOT produce it.

Run it and watch it fail for the right reason (the rule does not exist /
is not registered yet). That failing run is your discriminating evidence;
without it you cannot tell a passing test from a vacuous one.

## Step 2 — implement the rule

Implement `LintRule`: `name()` (kebab-case, stable — it is public
surface), `category()`, `default_severity()`, and the `check_*` method(s)
for the analysis passes your rule reads (`check_script` receives a
`ScriptAnalysisSnapshot`, template/style rules receive their own
snapshots). Report through `LintContext`; never `println!`, never a
side-channel.

Positions are `Span` values from the snapshot — SFC-absolute byte ranges.
Do not compute line/column yourself and do not parse the source text with
string search to "find" a construct: the analysis snapshot is the parsed
truth, and [source identity](./source-identity.md) explains why byte
spans are the only stable currency.

## Step 3 — register and run

Add one `registry.register(Box::new(...))` line to
`register_builtin_rules` in `crates/verter_diagnostics/src/rules/mod.rs`
(keeping the category grouping), then iterate:

```bash
cargo test -p verter_diagnostics        # targeted iteration
node scripts/gate.mjs                   # canonical provider-free Rust gate
```

`node scripts/gate.mjs` is the repository's canonical local verification
(see `CLAUDE.md` → Testing); a contribution is not done until the gate's
receipts are complete. The registered-rule count is itself tracked: the
docs reference harness (`pnpm --filter docs check`) fails if the live
registry count drifts from the generated-reference plan, so a rule added
without its count refresh is caught, not silently accepted.

## Step 4 — what you must NOT do

These are the three ways tutorial code rots into architecture debt; each
has a ratified owner already:

- **No filesystem access of your own.** Rules receive parsed snapshots;
  if you genuinely need file access, the sole authority is the
  `WorkspaceAccess` trait in `crates/verter_workspace/src/traits.rs`
  (with `NativeFs` as the only `std::fs` boundary). A rule that opens
  files directly has no review path to merge.
- **No private caches.** Reuse flows through the session's existing
  stores — the memo and artifact authorities such as
  `crates/verter_session/src/file_artifact_store.rs`. A rule-local
  memo table is a second semantic authority; the narrowing contracts in
  `tests/architecture-health/ARH1/products/dependency-contracts.json`
  exist precisely to retire those.
- **No test-only hooks as API.** `test_`-prefixed scheduler/session seams
  are recorded test-only surfaces under ratified gating. Production code
  (including rules) builds on the retained public surfaces listed in the
  contracts page, not on seams that exist so tests can drive scheduling.

## Step 5 — update the owning documentation

If your rule adds user-visible behavior, the lint rule reference is a
generated page bound to the registry — regenerate rather than hand-edit
(the docs build fails on drift, per Step 3). For anything else, follow
the repository rule: update the owning document, keep conventional
commits (`feat(diagnostics): ...`), and include both test assertions in
the PR checklist.

## Where to go next

- [Architecture contracts](./architecture-contracts.md) — owner
  boundaries for every crate you just avoided duplicating.
- [Query lifetimes and determinism](./query-lifetimes.md) — before
  touching anything cached.
- [Testing](./testing.md) — the full test-design guide.
