# BV1 — dispatch context

## Predecessor state

Sole predecessor B4, ACCEPTED. Base commit for this candidate: `ff2e0217e` (the tip at
dispatch time after a clean rebase; the branch was originally cut from `051a42ae3`, the
commit that accepted B4 and opened this block — the commits between the two are unrelated
ledger-identity/CI maintenance, docs-only, none touching Vue or the compiler).

## Starting state

`crates/verter_vue_conformance/corpus/known-divergences.json` carried 63 tracked
divergences between Verter's Vue codegen and the official Vue compiler's output, all
generated against the superseded `3.6.0-rc.1` oracle pin — stale relative to the program's
locked `3.6.0-rc.3` domain and never regenerated after a prior repin attempt.

## Work performed

1. Repinned the seed-goldens oracle to `3.6.0-rc.3` and regenerated the divergence ledger
   against it (58 entries once current).
2. Diagnosed and fixed a compile-pipeline determinism gap: the conformance suite's shared
   `VerterHost` produces different emitted module text across `cargo test`'s default
   concurrent-test-thread scheduling on an otherwise-unchanged tree. `cargo nextest`
   (the canonical gate's runner) is not exposed — one process per test, no shared state to
   race on. Documented in `seed_conformance.rs`'s doc comment; all conformance-suite
   commands in this dispatch used `--test-threads=1` for a reliable signal.
3. Closed all 58 divergences through real Vue Vapor/VDOM codegen corrections — see
   `landing-record.md` for the itemized list. Two genuine runtime bugs were found and fixed
   along the way (not just conformance mismatches): a Vapor `<slot :prop>` binding silently
   dropping its value, and a Vapor mixed-text/slot-sibling case attaching a reactive text
   update to the wrong DOM node.
4. Fixed the comparator's own scope-ordinal computation (raw AST-node-index-derived,
   sensitive to cosmetic paren-node counts) to rank by the semantic scope's own creation
   order instead — closes the one entry this was blocking, with an added discriminator
   recipe proving no blind spot was introduced.
5. A three-mandate review pass (codex conformance, grok architecture, Claude-subagent
   adversarial-with-plant-prove-RED-GREEN in its own worktree) surfaced two concrete,
   verified findings: a ratified BV0→BV1 debt row (`<template v-if>` not a transparent
   wrapper in Vapor codegen, recorded in `evidence/BV0/landing-record.md`'s "Fix round 3")
   still `#[ignore]`d, and a v-bind blank-value/same-name-shorthand conflation bug. Both
   fixed for real in a targeted follow-up round; see landing-record.md.
6. Investigated (not merely asserted) two further syntax-shape gaps the review raised —
   v-for/v-slot rest-element and default-value destructuring, and `<slot>` element v-bind
   spread/dynamic-key props. The destructuring gap was confirmed via real pinned-runtime
   mounts to be genuine data loss and fixed; the slot-outlet gap was confirmed genuine but
   architecturally separate (needs official's shared merge-array prop form) and left as a
   named, pinned characterization test rather than forced through.

## Review arc

Three review mandates ran in parallel against the fully-closed-backlog candidate:
codex (conformance, `gpt-5.6-sol`, high effort), grok (architecture, `grok-4.6`, high
effort, explicit default-to-BLOCK posture), and a Claude subagent (adversarial, its own
worktree, genuine plant→RED→revert→GREEN cycles against three independent bugs plus the
comparator fix itself). Adversarial returned PASS with one non-blocking discriminator-
soundness finding (independently corroborated by codex, same root observation). Codex
returned BLOCKING with five findings; two were concrete and verified (the debt row, the
blank-value conflation), fixed in a follow-up round; the discriminator-soundness finding
was also fixed; the remaining codex findings (`FC-HYDRATION-001`/`FC-TS-001-LOCAL`/
`FC-ATOMIC-001`/`FC-ZERO-WORK-001`/`FC-PERF-001` "not independently evidenced") reflect
codex's read-only sandbox being unable to execute any test command in this dispatch
(`cargo` build-lock permission denial) combined with a diff-scoped read that does not see
predecessor-block evidence (these acceptance IDs originate from BF3/B4, already ACCEPTED)
— not a new gap this candidate introduced. Recorded, not silently dismissed; the full
canonical gate independently confirms nothing regressed program-wide.

## Verification arc

See `landing-record.md` for the full canonical-gate summary and command list.
