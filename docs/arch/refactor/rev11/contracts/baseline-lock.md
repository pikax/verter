# Baseline Lock Contract

**Status:** Normative implementation-entry contract.  
**Package state:** `A0` locks the entry checkout. No implementation baseline is locked until `A6` accepts the exact post-Gate-0 lineage and refreshes all affected evidence.

# 1. Rule

Gate 0 uses two explicit source points:

- `EntryCheckoutSha`, captured at `A0`, proves what was initially inspected;
- `ImplementationBaselineSha`, accepted at `A6`, is the exact post-command-fix, post-harness, post-safety, post-instrumentation lineage on which later charters and baselines rely.

No non-local architecture cutover begins without the `A6` implementation baseline lock. A branch name, short SHA alone, remote webpage, “latest main,” or “architecture-equivalent successor” is insufficient.

The historical `9af553dd…` evidence file describes the source used to design the plan. It is not permission to implement against that SHA or a current branch without verification.

# 2. Required lock record

```toml
schema_version = 0
status = "LOCKED"

[repository]
remote = "https://github.com/pikax/verter.git"
branch = "main-or-explicit-branch"
entry_checkout_sha = "FULL_40_HEX_SHA_FROM_A0"
implementation_baseline_sha = "FULL_40_HEX_POST_GATE0_SHA_ACCEPTED_BY_A6"
implementation_baseline_tree = "GIT_TREE_OID"
short_sha = "SHORT_IMPLEMENTATION_SHA"
dirty = false
untracked_count = 0
submodule_state = "none-or-exact-state"
open_architecture_changes = ["PR/branch/commit and disposition"]

[locks]
cargo_lock_sha256 = "..."
pnpm_lock_sha256 = "..."
other_lockfiles = []

[toolchain]
rustc = "..."
cargo = "..."
nextest = "..."
node = "..."
pnpm = "..."
platform = "..."
architecture = "..."

[verification]
canonical_rust_pair_proven = true
typescript_builds_proven = true
napi_proven = true
wasm_proven = true
corpus_commands_proven = true
raw_evidence_uri = "..."
```

Record exact external TypeScript/provider/framework/compiler versions used by affected tests and benchmarks.

# 3. Open-change disposition

Before freezing, inventory open or queued architecture-affecting changes touching compiler, syntax, semantic, cache, input/snapshot, provider, CSS, framework, protocol, or public API boundaries. For each, choose:

- include before freeze;
- exclude and rebase/reconcile later;
- abandon;
- explicitly coordinate as a predecessor/dependent block.

Do not implement a new architecture while an unaccounted parallel change is rewriting the same owner.

# 4. Canonical command proof

For every canonical command record:

- exact command and working directory;
- environment/features;
- exit code;
- executed test/case count;
- skipped/ignored count;
- exact binaries/packages/fixtures;
- raw output digest.

A green command that executed zero intended work is a failure.

# 5. SHA change and record location

The accepted lock record is an immutable evidence artifact. It is not required to contain the SHA of a commit that embeds the record itself; `implementation_baseline_sha` names the exact code/evidence candidate evaluated by A6, and the record is addressed by its own digest. A later documentation-only commit that stores the record does not silently become the implementation baseline.

Any implementation-baseline SHA change requires:

1. a new lock record;
2. an architecture-affecting diff;
3. refreshed `current-tree-reconciliation.md` rows;
4. rerun non-vacuous command proof;
5. review of affected measurements, gates, and charters.

Unaffected historical evidence may remain cited but never silently substitutes for current source.
