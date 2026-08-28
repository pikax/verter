# AMD-005 independent governance/DAG challenge

## Verdict

**BLOCKING_FINDINGS — bound to candidate commit
`ce1d0e4688af1b5bd548b6b68286632cc0f7ede8`, repository tree
`1ff1f83d8e994b6f1169b0b209c9f557c23f4728`.**

The branch, commit, and tree matched the dispatch identity before review. The worktree
was clean at that binding point. The candidate's parent is
`b3249d13d07806a14a4307954dfcc459cf7301ac`, tree
`57e412549c24c903877b471000569c99591a49fc`, which is also the current clean
`program/architecture-lock` checkout inspected read-only at
`<repo-root>`.

## Blocking findings

- `docs/arch/refactor/rev11/README.md:50-55`,
  `packages/framework-conformance-harness/evidence/current-state.md:20-48`,
  `docs/arch/refactor/rev11/amendments/AMD-005-framework-compiler-conformance-rescope.md:263-274`,
  `packages/framework-conformance-harness/evidence/program-state-transition.md:33-43`,
  `docs/arch/refactor/rev11/amendments/AMD-005-framework-compiler-conformance-rescope.md:288-293`, and
  `docs/arch/architecture-lock/ledger/program-state.toml:236-255`
  — **violated principle: a ratification package and its singular transition action
  must describe the live integrated predecessor state exactly; an accepted block
  cannot be treated as an external in-flight candidate or scheduled for acceptance
  again.** The canonical checkout is now `b3249d13d...`, its 51-block ledger records
  B1 `ACCEPTED` with `maintainer_decision = "ACCEPTED"`, and the exact package commit
  is directly based on that checkout. The stated `<worktree>/verter-b1`
  checkout does not exist, `git worktree list` registers only the canonical and
  rescope worktrees, and `refs/heads/work/b1-neutral-contracts` is absent. Nevertheless,
  the package says B1 is `READY` and separately in flight, calls that worktree
  unaccepted, says B1 “continues and must be accepted normally,” and makes re-reviewing,
  landing, and accepting B1 step 2 after ratification. The quoted maintainer action
  still says to keep BF1 locked until B1 is accepted even though that condition is
  already satisfied. This is not a harmless observation drift: it makes the proposed
  exposure sequence and exact ratification action describe an impossible second B1
  acceptance rather than the live transition from accepted B1. The candidate does
  preserve B1's actual ledger row and sole predecessor `A6`, and its B1 charter is
  byte-identical to canonical (`sha256
  ac60d191221fc5e5938e0343091c6809648a482960ca7c1a49596e547d3e28e1`),
  but those facts expose rather than cure the package's stale governance narrative.

- `.agent-run/architect-report.yaml:4-7`, `.agent-run/architect-report.yaml:43-55`,
  `packages/framework-conformance-harness/evidence/reviews/README.md:10-13`,
  `packages/framework-conformance-harness/evidence/reviews/architecture-challenge.md:3-15`,
  `packages/framework-conformance-harness/evidence/reviews/architecture-challenge-reattestation.md:3-10`,
  `packages/framework-conformance-harness/evidence/reviews/conformance-challenge.md:14-27`, and
  `packages/framework-conformance-harness/evidence/reviews/governance-challenge-reattestation.md:3-8`
  — **violated principle: every package self-report and ratification mandate must bind
  the exact candidate commit/tree; attestations from an earlier tree cannot approve a
  later squashed/rebased tree.** The architect report claims candidate
  `8fbef4ba2ce30d93a636f769639519df7a773a92` / tree
  `eba511f865239ac27abf7da4fd3b4d292ed9ebec`, while the dispatched and independently
  resolved candidate is `ce1d0e468...` / tree `1ff1f83d...`. The candidate-tree
  versions of the architecture and conformance reports bind `8fbef4ba...`, and the
  two candidate-tree reattestations bind
  `6920ddc6...` / tree `7d38eb20...`; none binds `ce1d0e468...`. The README makes a
  changed identity invalidate all three reports. The content SHA-256 values sampled
  below still match the architect report, but matching inner-file digests cannot repair
  the wrong repository commit/tree envelope.

- `packages/framework-conformance-harness/evidence/validate-package.mjs:242-250`,
  `packages/framework-conformance-harness/evidence/package-checklist.md:28-32`, and
  `packages/framework-conformance-harness/evidence/validation.md:36-53` —
  **violated principle: the exact package must pass its own declared validator and
  preserve the preparer/challenger boundary.** The validator requires all three
  challenge report paths to be absent from the prepared candidate, the checklist says
  no independent report is present, and the validation record claims the absence check
  passed. Yet commit `ce1d0e468...` adds candidate-tree versions of all three reports
  plus two reattestations.
  Running the declared command on the exact candidate fails at the first absence gate:
  `Error: architecture-challenge.md must be authored independently, not by package
  preparation`. `git diff --check` also reports trailing blank lines in the committed
  architecture and conformance reports. Distinct ignored run logs show separate
  architect/challenger session IDs, so there is no positive evidence that the original
  reviewers were the architect; the blocker is that their post-candidate artifacts
  were folded into a new candidate without regenerating the identity envelope, while
  the package still claims and enforces that they are absent.

## Independent checks and non-blocking discoveries

### Candidate and digest binding

The exact Git identity was independently resolved:

| object | recomputed identity |
|---|---|
| candidate commit | `ce1d0e4688af1b5bd548b6b68286632cc0f7ede8` |
| candidate tree | `1ff1f83d8e994b6f1169b0b209c9f557c23f4728` |
| candidate parent | `b3249d13d07806a14a4307954dfcc459cf7301ac` |
| AMD-005 | `1d4a354b72c9ec2458e47ac570b0bc6cc893576e7d364c6033b5f98f85302d81` |
| amended DAG | `335e0863ba1f21473a24befc0093dc01bad4f065ff03e6716c113448be054489` |
| Vue package lock | `0dd2290c0b7d01f4727953b838610727b18bcb999b634eeb8ab726508a34b951` |
| Vue exact closure | `d5caba234d8545b8b7bc7cc4cca8b8cf63f8ed594140d7cae80f3c7ae64606b2` |
| Vue case manifest | `30123a6d88e1e7382afdcc752b5438c3486dd462e59ce831742ad0a3a3dd95bd` |
| Svelte package lock | `0c27c9fc7bed24be3fd7a546b55b6ee5858b244a57613390a213fdb454b92ce2` |
| Svelte exact closure | `3dc4209c2911700de92858e350ddda2e6f5f333874a2eb330125ee808910dbce` |
| Svelte case manifest | `c251be5b8b1de3e58c526700c426e2502e8bd1eb1dd622e22119b667adee7a8e` |

The sampled content digests agree with the architect report. Its candidate commit/tree
and base/lineage description do not.

### Program-state integrity and ratification status

Both required commands passed independently:

```text
OK: program-state.toml ... validated 56 blocks ... in mode live
OK: program-state.template.toml ... validated 56 blocks ... in mode template
```

Raw block-section comparison between the current canonical 51-block ledger and the
candidate 56-block ledger found 51 common sections with zero byte differences, zero
removed sections, and exactly `BF1`, `BF2`, `BF3`, `BV1`, and `BS1` added. Outside the
block sections, normalizing `program_dag_digest` makes the file headers identical.
Thus no accepted/terminal digest, state, identity, evidence, review, decision, or note
was rewritten. The dispatch's statement that canonical B1 is `READY` is itself stale;
the candidate correctly preserves the canonical `ACCEPTED` row byte-for-byte.

AMD-005 remains explicitly `PROPOSED — NOT RATIFIED`. All five new charters say
`PROPOSED / LOCKED`; both state shapes give the five new rows `LOCKED`, empty identity
and evidence fields, `PENDING` reviews, and `maintainer_decision = "PENDING"`. No field
claims any new block is accepted or that AMD-005 has been ratified.

### Exact DAG and retained edges

Independent structural parsing found 51 base blocks/110 ordinary edges and 56 candidate
blocks/116 ordinary edges. The only replaced predecessor edges are:

- removed: `B1→B2`, `B1→B3`, `B4→B5`, `B5→C4`;
- added: `B1→BF1`, `BF1→BF2`, `BF2→BF3`, `BF3→B2`, `BF3→B3`,
  `B4→BV1`, `B4→BS1`, `BV1→B5`, `BS1→B5`, and `B6→C4`.

All other 106 ordinary edges are retained. The affected predecessor lists are exactly:

| block | predecessors |
|---|---|
| B1 | A6 |
| BF1 | B1 |
| BF2 | BF1 |
| BF3 | BF2 |
| B2 | BF3 |
| B3 | BF3 |
| B4 | B2, B3 |
| BV1 | B4 |
| BS1 | B4 |
| B5 | BV1, BS1 |
| B6 | B5 |
| C1 | A6, B1, B2 |
| C2 | B3, B5, C1 |
| C3 | C2 |
| C4 | B6, C3 |

This matches the maintainer's required semantic shape, including both `B6` and `C3`
as C4 predecessors.

### Concurrency, scope, independence, and checklist coverage

The package does not presently prove disjoint ownership and therefore does not
authorize either fork to run concurrently. AMD-005 lines 95-99 makes absence of an
exact proof serialize work. The disposition evidence identifies actual shared owners:
`vue_bridge.rs` spans B2/B3/BV1/B5, `strip_types/*` spans BV1/BS1, and the Svelte
carrier spans B2/B3/BS1/B5 (`emitter-mapping-dispositions.tsv:8,20-21`). For a future
BV1/BS1 overlap, `performance-impact.md:33-43` enumerates writable code, fixtures,
manifests, golden roots, package stores, target directories, ports, and explicit
heavy-machine leases, and makes shared locks/core files/one lease force serialization.
This is fail-closed and does not mistake the DAG fork for a concurrency receipt.

The direct parent-to-candidate diff changes only `docs/arch/architecture-lock/ledger`
and `docs/arch/refactor/rev11`; it changes no `crates/`, `packages/`, root manifest,
lockfile, production `scripts/`, or CI path. The evidence generators are under the
documentation evidence package and are not wired into compiler production. No
production compiler code changed in the actual package commit. A diff from the stale
claimed base `e6035b433...` includes the now-accepted B1 implementation and gate work,
which is another reason the package must not describe that SHA as its current
integration envelope.

All paths named by the maintainer's Required package list exist, and the package
validator reaches its final independent-report absence gate only after its preceding
option, capability, closure, manifest, DAG, and state assertions pass. Package presence
and most structural content are therefore complete. The stale B1 transition/ratification
content and the exact-candidate report topology above prevent governance completeness.

Ignored `.agent-run` logs record distinct session IDs for the architect, the original
three challengers, and both reattesters, and AMD-005 lines 295-297 expressly prohibit
self-ratification and self-review. No new block claims maintainer acceptance. That
supports reviewer independence at the session level, but it cannot validate reports
against a different commit/tree or make a validator-failing candidate ratifiable.
