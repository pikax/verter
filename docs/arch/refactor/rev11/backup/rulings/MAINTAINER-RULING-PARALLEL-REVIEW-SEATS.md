---
ruling_id: "PARALLEL-REVIEW-SEATS"
type: "maintainer-directive"
date: "2026-08-18"
date_source: "stated"
binds: ["program-wide review-seat protocol"]
source_file: "MAINTAINER-RULING-PARALLEL-REVIEW-SEATS.md"
summary: "Review mandates run concurrently, not sequentially. Read-only seats (conformance, architecture) run in parallel on the shared candidate worktree (codex exec --sandbox read-only mutates nothing); the adversarial seat gets its own worktree cut from the exact candidate commit, since it plants and reverts mutations. Fix cycles fan out the same way: dispatch all seats for a round at once, fix once against the union of findings. Same-day amendment: the adversarial mandate is reassigned from an external CLI to a Claude Agent subagent in its own worktree, because a read-only codex seat structurally cannot perform the required plant/RED/revert/GREEN cycle."
supersedes: []
superseded_by: []
contradicts: []
notes: "Unchanged throughout: codex+grok only for conformance/architecture seats, never a Claude subagent there; prompts neutral/unprimed; grok keeps default-to-BLOCK; round cap 3; no seat grades its own work."
---

# Maintainer ruling — review seats run in PARALLEL (2026-08-18)

> also reviewers in review-fix cycle should be parallel

**Run the review mandates concurrently**, not one after another. Sequential seats were costing a full
review round of wall-clock per block for no correctness gain.

## The one real constraint, and how to satisfy it while still going parallel

Seats were serialized because the ADVERSARIAL mandate plants mutations into the working tree
(plant → prove RED → revert → prove GREEN). A concurrent seat reading that same tree sees a mutated
file and reports a phantom defect, or worse, a phantom pass. That contamination is real — it was
observed on this program.

It does NOT require serialization. It requires ISOLATION:

- **Read-only seats run in parallel on the same tree.** `codex exec --sandbox read-only` mutates
  nothing, so conformance and architecture can share the candidate worktree safely and start together.
- **The adversarial seat gets its OWN worktree**, cut from the exact candidate commit. It plants and
  reverts there, touching nothing another seat can see.
- **Fix cycles fan out the same way**: dispatch all seats for a round at once, collect, then fix once
  against the union of findings — rather than fix-review-fix-review serially.

## Unchanged

- Seats are EXTERNAL CLIs only — `codex` + `grok`, never a Claude subagent as a review seat.
- Prompts stay NEUTRAL and unprimed; grok keeps an explicit default-to-BLOCK posture.
- No seat grades its own work.
- Round cap 3, then targeted-delta review and explicit dispositions.
- Only ONE gate at a time on this machine — that is a memory constraint, not a review one, and it does
  not relax.

## Amendment — the adversarial seat is a SUBAGENT (2026-08-18, same day)

> the adversarial reviewer should be subagent

**The adversarial mandate runs as a Claude Agent subagent, in its own worktree.** Conformance and
architecture stay on external CLIs (`codex`, `grok`), read-only, in parallel.

**This resolves a real contradiction in the previous instruction.** The adversarial mandate REQUIRES
plant → prove RED → revert → prove GREEN, with the plant proven present, unique and new. A seat invoked
as `codex exec --sandbox read-only` cannot write, so it structurally CANNOT perform that check — it could
only read tests and opine, which is precisely the weak review this program has been burned by. A subagent
with full tool access in an isolated worktree can actually plant, run the suite, observe RED, revert, and
observe GREEN.

Resulting seat roster per review round, all dispatched concurrently:

| mandate | who | where |
|---|---|---|
| conformance | `codex exec --sandbox read-only` | candidate worktree (shared, read-only) |
| architecture | `codex exec --sandbox read-only` (or `grok`) | candidate worktree (shared, read-only) |
| **adversarial** | **Claude Agent subagent** | **its OWN worktree, cut from the exact candidate commit** |

Unchanged: prompts neutral and unprimed; grok keeps default-to-BLOCK; no seat grades its own work; round
cap 3; the adversarial seat still must prove every plant applied, and a green planted run means the plant
failed until proven otherwise.
