# Review reports for the exhaustion-closing delta

Earlier rounds of this block have no committed reports — that gap is recorded in the landing record.
These three are committed in full so the same gap does not repeat.

All three seats are EXTERNAL CLIs. No seat authored any of the text or code it reviewed, and the
seats ran SEQUENTIALLY in one worktree, because concurrent seats planting mutations in a shared tree
produce cross-contaminated results.

| mandate | seat | verdict | report |
|---|---|---|---|
| conformance | Codex `gpt-5.6-sol`, reasoning effort `high` | BLOCKING | [`exhaustion-conformance.md`](exhaustion-conformance.md) |
| architecture | Codex `gpt-5.6-sol`, reasoning effort `high` (separate session) | BLOCKING | [`exhaustion-architecture.md`](exhaustion-architecture.md) |
| adversarial | Grok 4.6, extra-high reasoning, explicit default-to-BLOCK, required to author its own plants | BLOCKING | [`exhaustion-adversarial.md`](exhaustion-adversarial.md) |

Every prompt required, besides a per-claim verdict, an enumeration of EVERY numbered item of the
charter's "Owned scope and required procedure" and EVERY sentence of its "Required exits", each
marked `SATISFIED-BY-DELTA` / `UNCHANGED-BY-DELTA` / `NOT-EVIDENCED` with a citation — an uncited
criterion being `NOT-EVIDENCED` and BLOCKING by default. The prompts stated that `NOT-EVIDENCED` is a
legitimate and expected verdict for some items and must not be softened.

The reports are reproduced as the seats wrote them, with one mechanical change: absolute checkout
paths in citations were rewritten as repository-relative, because a tracked file carrying a developer
home path fails a repository guard. Nothing else was altered.

## What they found, and what happened to it

Each seat returned BLOCKING. Every finding was fixed in the same session:

- **The recompile-lane probe fixture was shared and racy** (conformance F1, architecture ARCH-001).
  Reproduced by both seats: the non-serialized suite failed with `ENOTEMPTY`, and a probe could exit 0
  with `"fresh":true` while its own record carried an errored lane. Each invocation now takes a
  private `mkdtemp` fixture and removes only its own, the record publishes an errored-case list, the
  process exits non-zero when that list is non-empty, and the consuming test asserts it is empty.
- **The recompile-lane test named a stronger property than it measured** (adversarial F1). It
  concluded that the bundler's cross-file block "iterated over a non-empty list" from a measurement
  taken on a DIFFERENT, in-process host; the seat proved it by disabling that block in the built
  plugin and watching the test stay green. The claim was removed rather than reworded: the test now
  asserts only that `buildStart` completed over a real fixture and that both published modules are
  byte-identical to the host's products, and the evidence states the named closure condition for the
  part that remains unattributable.
- **Two new tracked evidence files carried a developer home path** (architecture ARCH-002) and failed
  an existing repository guard — a hard gate failure, caught before the gate ran. Rewritten as
  repository-relative, with an editorial line on the verbatim record saying exactly what was
  normalised.
- **Four documentation statements no longer matched the code** (architecture ARCH-003, adversarial
  F2/F3/F4): an overstated diagnostic count, a stale status paragraph, comments naming deleted
  constants, and a two-versus-three mismatch in the unreachable-class list. Each was re-measured and
  corrected.
- **A tracker identifier had leaked into an `#[ignore]` reason in crate source** (adversarial,
  out-of-delta). Pre-existing, but it is the only such occurrence in the tree and the repository
  forbids it, so it was adopted and fixed rather than recorded and left.

Two `NOT-EVIDENCED` verdicts survive the fixes and are NOT closed by them. They are the reason this
block is still not acceptance-recommended, and they are stated in the landing record's Status
section: the recompile WRITE remains unattributable out of process, and the escalated findings row's
per-failure clause cannot be satisfied at track level.

## The two targeted confirm rounds

The round cap applies, so the fix delta received TARGETED confirms — each asking two questions about
that delta only (did each fix do what it claims, does any of them introduce a defect), never a fresh
hunt across the branch.

| round | scope | seat | verdict | report |
|---|---|---|---|---|
| confirm 1 | the fix delta for the three reports above | Codex `gpt-5.6-sol`, effort `high` | BLOCKING | [`exhaustion-confirm-1.md`](exhaustion-confirm-1.md) |
| confirm 2 | the fix delta confirm 1 produced | Codex `gpt-5.6-sol`, effort `high` | PASS | [`exhaustion-confirm-2.md`](exhaustion-confirm-2.md) |

Confirm 1 found two more real defects in the fixes themselves, both proven by measurement rather than
argued: the probe's new errored-lane list scanned only the four named entry cases, so the per-export
drive family could error while the process still exited 0; and a module comment added by this work
cited a tracking row using a phrase the repository's archaeology guard forbids in crate source, which
made that guard FAIL — a hard gate failure, caught before the gate ran. Both were fixed and both were
re-measured two-sidedly in confirm 2 against the pre-fix revision.
