# A1 Exact-Candidate Record — non-vacuous commands and capability truth

`contracts/agent-orchestration.md` §9 bounded record for block A1. Lives
EXTERNALLY (not in the candidate tree) so recording review verdicts does not mint
a new candidate. Candidate identity is recorded in the external ledger
(`../program-state.toml`) by the orchestrator, not in this file.

```text
BLOCK: A1
STATE: BLOCKED
       (pending the three-mandate recheck — conformance, architecture,
        adversarial/performance — against the exact candidate identity recorded
        in the ledger. No self-approval: this record makes no PASS claim.)
BASE: b7ea2dc88bda86473de81de3438b7f88ef30adc7 / tree 47645406a9246e600af995c62608b709347e13a4
CANDIDATE: recorded in the ledger (single squashed commit on branch
           block/a1-command-truth, parented on BASE; the branch's sole commit —
           a documentation-only capability-matrix completion; every candidate
           command ran against the BASE tree content with a clean worktree)
ACCEPTED_TARGET: none
LANDING_EQUIVALENCE: none
CHARTER_DIGEST: b92ef37570b804d170aac6877cd41299e236a7dcb237a6c1d50e76a7748f6d4c
                (SHA-256 of docs/arch/refactor/rev11/charters/A1.md at BASE)
CONTEXT_PACKET_DIGEST: recorded in the ledger as block.A1.context_packet_digest
                       (SHA-256 of A1/context-packet.md in this evidence root)
STACK: none
CHANGES: one tracked file — docs/arch/refactor/rev11/contracts/capability-matrix.md:
         the 8 seed rows completed with executed-evidence facts only (exact
         oracle pins, verified Svelte pin, provider-route execution truth,
         recorded pre-existing failures); every cell A1 did not prove remains
         VERIFY, stated explicitly. No source, build, test, or config change;
         no failure was fixed.
DELETIONS: none required (evidence-only block). Evidence-only scaffolding
           outside the tree (the isolated sentinel clone) is disposable; the
           candidate tree contains no A1 scaffolding.
EVIDENCE: A1/command-proofs/ (raw outputs + index.md with per-command
          exit/discovered/executed/pass/fail/skip/provenance and SHA-256
          digests over every raw file — digest table at the end of index.md);
          A1/sentinel-verification.md (three mandated sentinels + one
          supplementary control, with plant-applied proofs);
          A1/environment.md (toolchain/platform/provenance bundle);
          A1/context-packet.md.
          Non-vacuous headline counts: gate S1 24044 run (24043 pass / 1 fail /
          582 skip), S2 3 suites clean, S3 8477 run all pass; doctests 4
          executed / 28 ignored across 35 suites; JS no-bail 4448 tests (4412
          pass / 4 fail / 32 skip); provider matrix 274/274 attempted
          (tsserver 83/8, tsgo 78/13, shared-tsgo 79/13, receipt bound to the
          BASE SHA); conformance: 286 Vue goldens, 1066 + 1218 Svelte goldens,
          80 name-parity rows, 11 macro-oracle cases, 507 live-oracle tests,
          all in sync/green; corpus gate = honest EXPLICIT skip (no corpus in
          this environment), captured with its skip reason.
          Recorded failures (none fixed): gate 1 (trybuild
          registered_authority_capabilities…, worktree-environment-sensitive —
          does not reproduce in a fresh clone of the same commit); clippy 7
          lints (workspace) / 5 (wasm target) in verter_session; provider
          matrix 34 (8+13+13); JS 4 (typeinfo 3, unplugin 1); one flake
          observed once (verter_tsc fallthrough_attrs…).
          Selector findings: root `pnpm test` is parallel+bail-fast and kills
          most of the JS surface on any early package failure; the
          @verter/types suite is typecheck-only (runtime throws invisible by
          design).
REVIEWS: conformance PENDING; architecture PENDING; adversarial/performance
         PENDING — all three must run against the unchanged candidate identity
         recorded in the ledger.
DISCOVERIES: (1) trybuild suite failure is checkout-environment-sensitive
             (linked worktree red / fresh clone green at the same commit) —
             disposition needed by the orchestrator (affects what "the
             candidate's gate result" means for later baselines);
             (2) root JS selector bail-fast vacuity risk — candidate for a
             CI-selector decision at A6, NOT changed by A1;
             (3) @verter/types typecheck-only semantics — capability-matrix
             relevant when TypeInfo rows are ratified;
             (4) pnpm 10.22 ignores package.json#pnpm.onlyBuiltDependencies
             (build scripts for @bufbuild/buf, @parcel/watcher, core-js
             skipped; buf/oxfmt shims still functional — gate preflight
             attested them present and ran the byte-pin with tolerance OFF).
NEXT_LEGAL_BLOCKS: per the validated ledger after A1 acceptance (A2 is the
                   sole successor gated only on A0+A1).
MAINTAINER_DECISION_REQUIRED: yes — A1 acceptance on the exact candidate
                              identity after the three mandates return.
```
