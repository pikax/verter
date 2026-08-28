---
ruling_id: "COMMENT-CLEANUP-PASS"
type: "maintainer-directive"
date: "2026-08-19"
date_source: "stated"
binds: ["program-wide, per-block landing process"]
source_file: "MAINTAINER-RULING-COMMENT-CLEANUP-PASS.md"
summary: "Every block gets a grok comment-cleanup pass, run after review mandates pass and before the squash, scoped to that block's diff only: remove AI watermark phrasing, restatement of the obvious, over-long explanations, and plan/phase archaeology; keep non-obvious invariant comments, safety/ordering/fail-closed rationale, rustdoc/JSDoc on public APIs, and any comment a test/guard asserts on. Comments only, zero code/behaviour change; re-run targeted tests + cargo fmt afterwards; the cleanup seat must not be the block's own reviewer."
supersedes: []
superseded_by: []
contradicts: []
notes: ""
---

# Maintainer ruling — comment cleanup pass per block (2026-08-19)

> I want to also add a pass at the end of each block to make grok update the comments added by the
> implementor, because opus does add watermark in the comments and comments are very long too and
> sometimes not necessary

## The pass

**Every block gets a grok comment-cleanup pass, run AFTER the review mandates pass and BEFORE the
squash** — so the landed commit is already clean and no follow-up landing is needed.

Scope: the comments in THAT BLOCK'S DIFF only. Not a repo-wide comment sweep.

### Remove
- **AI watermark phrasing** — narration of what the author did, self-congratulation, hedging, and
  "we/I" commentary. Comments describe the code, not the process that produced it.
- **Restatement of the obvious** — a comment that says what the next line plainly says.
- **Over-long explanations** where two lines carry the same information as ten.
- **Plan/phase archaeology** — already forbidden by `no_phase_archaeology_in_production_code`, but this
  pass is where it gets caught before landing rather than by the gate.

### KEEP — do not strip these
- Anything recording a NON-OBVIOUS invariant, or why the obvious alternative is wrong. That is the
  comment class worth its bytes.
- Safety/ordering conditions, fail-closed rationale, and links to a `(CRITICAL)` rule.
- Rustdoc on public API (`///`) and JSDoc on changed public signatures — required by the repo's
  documentation rules.
- Any comment a test or guard asserts on. Several guards match doc text; deleting one silently breaks
  a guard.

### Hard constraints
- **Comments only. Zero code changes, zero behaviour change.** If a comment is wrong, that is a finding
  to report, not a licence to edit code.
- Re-run the block's targeted tests plus `cargo fmt --all --check` afterwards — a comment edit can still
  break a doc-asserting guard or a rustdoc example.
- Grok does this as an IMPLEMENTER pass. It must NOT be the seat that reviewed the block, and it does
  not re-review its own cleanup.

Rationale: the landed commit is what everyone reads afterwards. Verbose machine-written commentary is
noise that survives long after the round that produced it, and the repo's own rules already demand that
code read as final-state.
