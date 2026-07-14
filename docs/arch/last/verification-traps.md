# Verification traps — how this repo and this toolchain hand you a FALSE GREEN

Every trap below cost real time, and each one produced a confident, wrong report that a later
reviewer had to catch. Read this before you trust any "it's clean" claim, **including your own**.

## The tooling traps

### `cmd | tee f` returns TEE's exit status, not the command's

```bash
cargo clippy --workspace -- -D warnings 2>&1 | tee /tmp/clippy.txt   # ALWAYS exits 0
```

`tee` succeeds, so the pipeline succeeds, so the command "passed". **Every "clippy is clean" report
in this effort was this artefact.** The truth, once someone checked properly: the implementation
branch is **red** under `-D warnings` with 83 errors — and had been all along, at its base as well
as its tip.

Fix: `set -o pipefail`, or check `${PIPESTATUS[0]}`, or capture to a file and check the status
separately. The project's own convention of piping long commands through `tee` for later grepping is
fine — but the **status** must come from the command, not the pipeline.

### macOS has no `timeout` binary

Wrapping a command in `timeout 540 …` on macOS yields **exit 127 and empty output**. There is no
`timeout` and no `gtimeout` on this machine (verified). An empty output file plus a nonzero status
reads exactly like "the tool is broken", and it was misdiagnosed as such. Use a bounded polling loop
in the shell, or a supervisor that owns the wait.

### `codex` at high reasoning effort returns NOTHING unless its code-mode host is on PATH

This one is nasty because it **degrades silently instead of failing**.

`~/.codex/models_cache.json` marks the `gpt-5.6-sol` model as `tool_mode: "code_mode_only"`, and the
Homebrew cask ships **no `codex-code-mode-host` binary** — so **every tool call fails**. At **low**
reasoning effort the model degrades gracefully and still answers, which is what made an early probe
look fine. At **xhigh** it gives up after three failed tool calls and **exits 0 with zero output** —
a silent no-op that reads exactly like "codex considered it and had nothing to say".

Sessions from this effort are known to have been **blocked, not empty-handed**. Some review legs
recorded as "no findings" were legs that never ran.

- **Fix:** put a sidecar directory on PATH symlinking **both** `codex` and `codex-code-mode-host`
  (the host binary ships inside `~/.codex/plugins/.plugin-appserver/`), and smoke-test at xhigh
  before relying on a leg.
- **Rule:** **a content-free codex leg is a BLOCKED leg, not a verdict.** Before you record any codex
  result, grep its log for `failed to spawn code-mode host` and check the output file size.

Note the correction of the correction, because the record matters: the spawn-error log line was at
one point dismissed as "harmless noise" and the real fault blamed on the flags. That dismissal was
**wrong**. The flags were fine; the host binary was missing.

### The Rust gate is `node scripts/gate.mjs` — a bare `cargo test` silently skips ~4,400 tests

`cargo test --workspace --tests` **silently drops the `verter_session` integration binaries** because
of feature unification, which is roughly 4,400 tests including most of the architecture guards. It
exits 0 and looks like a full run. `node scripts/gate.mjs` builds the test universe once and runs
**both** surfaces from the same artefacts. Use it. If you must run the surfaces by hand, run both:
`cargo nextest run --workspace` **and** `cargo test -p verter_session --tests`.

Related: default `cargo nextest run` is **fail-fast**. One pre-existing failure cancels the rest and
hides everything downstream of it — this masked four real guard failures once. Use
`--no-fail-fast` when you are assessing the state of a branch rather than gating a change.

## The reasoning traps

These are not tooling problems. They are the two ways people in this effort convinced themselves —
repeatedly, in writing, with citations — of things that were false.

### A static "safe because it roots on a hash" argument is worthless here. Settle reachability with a TEST.

This exact argument was made and refuted **three times** in the cache-admission work — once by a
read-only diagnostic, once by an adversarial reviewer who had been explicitly instructed to attack
that very claim and who concluded it was safe, and once in a rationale comment written into the
source. Every time, a discriminating test proved poison.

The underlying reason is explained in
[`cache-admission-closure-design.md`](cache-admission-closure-design.md) §2: rooting on a content
hash proves nothing when the reason a read was non-cacheable **does not move the content hash**.

**The working rule:** when two reviewers disagree about whether a path is reachable, **do not get a
third opinion — write the test.** Force the exact scenario through the real seam and watch it go
red. In this effort, that is the only method that ever settled the question, and it settled it every
time it was tried.

### A passing suite is not proof. Mutate the fix and watch the test go red.

A landed "fix" in this area had **zero coverage**: reverting its headline predicate left the entire
suite **green**. The test pinned a neighbouring loop, not the retention it claimed to pin — and its
doc comment explicitly claimed otherwise. It took a reviewer manually mutating the production
predicate to discover it.

**The working rule, before you claim a fix is covered:** revert your own change, run the test you
wrote for it, and confirm it **fails**. Then restore. If it still passes, you have written a stub
that advertises coverage it does not have — which this project's rules class as a gate-bypass, not a
pass.

This is also the standard the independent second solve of the cache bug was held to, and passed:
reverting its predicate turned its test red, restoring it turned it green.

## The status traps

### "Compile-restored" is not "green"

A long fix chain once tracked progress by counting compile errors (261 → 164 → 55 → 0) and reported
success. The first honest test run was **400 failures**. A compile-error count measures nothing about
behaviour. **The completion signal is a green run, never a compile-error count.**

### A worktree's git history can be unbisectable

The implementation branch contains commits that **do not compile**. Any argument of the form "this
was green at commit X" is unsafe there, and `git bisect` over it is unsound. It lands as a **squash**
for this reason among others.
