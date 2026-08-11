# A1 Exact-Candidate Record — non-vacuous commands and capability truth (round 2)

`contracts/agent-orchestration.md` §9 bounded record for block A1. Lives
EXTERNALLY (not in the candidate tree) so recording review verdicts does not mint
a new candidate. This round supersedes the round-1 record (retained at
`historical/round1/` for audit only); every identity-bearing evidence file in
this bundle carries the FINAL candidate identity below.

```text
BLOCK: A1
STATE: BLOCKED
       (pending the three-mandate recheck — conformance, architecture,
        adversarial/performance — against the exact candidate identity below.
        No self-approval: this record makes no PASS claim.)
BASE: b7ea2dc88bda86473de81de3438b7f88ef30adc7 / tree 47645406a9246e600af995c62608b709347e13a4
CANDIDATE: 13cedd6fc1315bfb6fec0c4cacb0eacdb02c6c83 / tree a992bb87382e58d6ec846c7be37cbb941ee0b1b2
           (single squashed commit on branch block/a1-command-truth, parented on
           BASE; documentation-only capability-matrix completion. ROUND-2
           ORDERING: the commit was created 2026-08-10T07:17:47Z, BEFORE the
           first evidence run at 07:19:20Z; EVERY command and sentinel in this
           bundle ran against exactly this SHA/tree with a clean worktree, and
           NO tracked change followed the runs — the charter's one unchanged
           candidate/evidence SHA requirement holds by construction.)
ACCEPTED_TARGET: none
LANDING_EQUIVALENCE: none
CHARTER_DIGEST: b92ef37570b804d170aac6877cd41299e236a7dcb237a6c1d50e76a7748f6d4c
                (SHA-256 of docs/arch/refactor/rev11/charters/A1.md — identical
                bytes at BASE and CANDIDATE)
CONTEXT_PACKET_DIGEST: recorded in the ledger as block.A1.context_packet_digest
                       (SHA-256 of A1/context-packet.md in this evidence root:
                       244d9d14b6123b2ef7c084cd7ecda33ef8182ef57e1c04ed005b14f9a1c21f67)
STACK: none
CHANGES: one tracked file — docs/arch/refactor/rev11/contracts/capability-matrix.md:
         every seed-table cell RESTORED to its A0-accepted base value (the
         Status column is exactly VERIFY on all 8 rows — the contract's
         unapproved marker is intact); execution facts moved to a NEW
         references-only §2.1 subsection that points at this external bundle by
         file/row NAME and inlines no count, date, verdict, or SHA, so the
         tracked file is byte-stable across evidence re-runs; non-VERIFY seed
         cells cite the ratified plan seed at the A0-accepted base as their
         authority; the Vue golden corpus is described as three trees
         (vdom, vapor, vdom-inline); the LSP receipt statement is the precise,
         now-true binding (receipt sourceSha == this CANDIDATE sha). No source,
         build, test, or config change; no failure was fixed.
DELETIONS: evidence-only scaffolding created by the block — the isolated
           sentinel clone — DELETED after the battery and verified gone
           (charter in-scope requirement); the candidate tree contains no A1
           scaffolding.
EVIDENCE: A1/command-proofs/ (raw outputs + index.md, uniform
          discovered/executed/passed/failed/skipped convention, baseline-lock §4
          fields, SHA-256 digest table over every raw + doc file);
          A1/sentinel-verification.md (sentinels A/B/C/D/E + fmt control, every
          plant proven present/unique/new, JS canonical selector honestly
          UNMET-with-cause); A1/environment.md; A1/context-packet.md.
          Headline counts (this round, candidate-bound): gate VERDICT PASS —
          S1 24626 discovered / 24044 executed / 24044 passed / 0 failed /
          582 skipped; S2 3 suites clean; S3 9040 / 8477 / 8477 / 0 / 563;
          ignored-test inventories enumerated from the gate's own archives EQUAL
          the skip counts (582 and 563) — no silently-not-running surface;
          targeted receipts: typeinfo_proto_ts_freshness 5/5, verter_css_syntax
          94/94; doctests 32 discovered / 4 executed / 4 passed / 28 ignored
          (35 suites); JS no-bail 4448 / 4416 / 4411 / 5 / 32 (typeinfo 3 +
          unplugin 1 pre-existing; playground 1 nondeterministic — see
          DISCOVERIES); scripts 52/52; wasm spec 43 / 41 / 41 / 0 / 2; provider
          matrix attempted 274/274, per-route tsserver 83/8, tsgo 78/13,
          shared-tsgo 77/15, setupFailures [], receipt sourceSha == CANDIDATE;
          conformance 286 Vue goldens (3 trees) + 1066 + 1218 Svelte goldens +
          80 name-parity rows + 11 macro-oracle cases in sync, svelte-oracle
          6453 / 6441 / 6441 / 0 / 12; corpus gate ARMED: 290 .vue discovered /
          40 sampled / 333 requests over all three real provider routes,
          FAILED (exit 1) on REAL pre-existing acceptance bars (all routes
          wedged, latency bars breached, unexpected-empty results) — recorded,
          not fixed; corpus handled as CLASSIFIED: anonymous label "Corpus A",
          content fingerprint
          7e9a65dd26b4cd1f17158aa26dc658e8a10768668a44a7aae74067e171f6dec5
          (sha256 over the sorted sha256(content)+relpath manifest of its 290
          .vue files), FILE_DETAIL off, and a zero-hit grep for the corpus
          directory name over the ENTIRE evidence bundle and the tracked diff
          (the only "nuxt" tokens anywhere are the repository's own
          packages/nuxt workspace package — a tree fact predating A1).
          Recorded failures (none fixed): clippy 7 lints (workspace) / 5 (wasm
          target) in verter_session; provider matrix 36; JS 5; corpus-gate
          acceptance bars.
REVIEWS: conformance PENDING; architecture PENDING; adversarial/performance
         PENDING — all three must run against the unchanged CANDIDATE above.
DISCOVERIES: (1) NONDETERMINISM RESTATEMENT (supersedes round 1's refuted
             "checkout-environment-sensitive / linked worktree vs fresh clone"
             claim — that causal claim is WITHDRAWN and appears nowhere in this
             bundle): the verter_language trybuild compile_fail failure is
             nondeterministic and NOT reproduced under isolation — reviewer:
             6/6 green in the same linked worktree at the round-1 candidate
             (3 warm nextest + 2 cold-scratch CARGO_TARGET_DIR; the one red ran
             23.2s vs 0.9s warm); this round: green inside the full gate and
             4/4 green direct re-runs (command-proof row 21). The
             verter_tsc fallthrough_attrs case, the playground wasmInContextLs
             case (failed once under the 24-package parallel no-bail run, green
             in its dedicated config), and the shared-tsgo 2-case delta receive
             the SAME classification — the round-1 asymmetry (flake vs
             topology-defect on same-shaped evidence) is corrected.
             (2) root `pnpm test` is parallel+bail-fast and cannot reliably
             deliver a planted suite's verdict on a red baseline — reproduced
             on a second independent planted run; its sentinel is recorded
             UNMET-with-cause and the discriminating credit belongs to the
             --no-bail variant (candidate for a CI-selector decision at A6, NOT
             changed by A1).
             (3) @verter/types is typecheck-only (runtime throws invisible by
             design) — round-1 finding, still true; the round-2 plant is
             type-level accordingly.
             (4) pnpm 10.22 ignores package.json#pnpm.onlyBuiltDependencies;
             buf/oxfmt shims still functional — gate preflight attested them
             present and ran the byte-pin with tolerance OFF (round-1 finding,
             re-observed).
             (5) the armed corpus gate FAILS on real acceptance bars (every
             route wedges under corpus load) — the known pre-existing
             product-issue class, now count-attested under A1; recorded for the
             product backlog, not fixed here.
NEXT_LEGAL_BLOCKS: per the validated ledger after A1 acceptance (A2 is the
                   sole successor gated only on A0+A1).
MAINTAINER_DECISION_REQUIRED: yes — A1 acceptance on the exact CANDIDATE
                              identity after the three mandates return.
```
