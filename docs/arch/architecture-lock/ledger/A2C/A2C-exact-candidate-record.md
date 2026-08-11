# A2C exact-candidate record

Bounded record per `contracts/agent-orchestration.md` §9.

```text
BLOCK: A2C
STATE: BLOCKED
       (latency gate failed; three-mandate recheck remains pending)
BASE: 70ea4c01bea870e9684a66f229230808aeb64235 / tree retained by Git
CANDIDATE: 04048a9471f1c13e81cda075fa27a6c35b59a842 /
           tree 5ca44c8e58d79cf040bbd066a25c44323cf0e10c
           (candidate-state transition remains external program-ledger authority)
ACCEPTED_TARGET: none
LANDING_EQUIVALENCE: none
CHARTER_DIGEST: 50bbb992eadcb9080de2f48ff9d21ce667f409dabdb1bea117d6bbf3899be895
CONTEXT_PACKET_DIGEST: 72bbd3412957d54fd9770465de0e2e63b91265483199af2737d2df455985684d
STACK: none / PRE-A6
CHANGES: committed candidate only; no tracked evidence-time change.
DELETIONS: none required or authorized by evidence execution.
EVIDENCE: A2C/latency-benchmark-record.md; A2C/LATENCY-GATE-STOP-FINDING.md;
          A2C/command-proofs/latency/. Forty measured interleaved pairs after five
          warmup pairs, five construction shapes, one stable control, and 100,000
          deterministic bootstrap resamples per cell. Digest manifest:
          A2C/command-proofs/digests.sha256.
REVIEWS: conformance, architecture, and adversarial/performance recheck pending;
         this record makes no acceptance recommendation.
DISCOVERIES: the candidate fails the frozen 3.000000% construction-latency gate.
             Median slowdowns: flat -1.413832%, nested 2.480523%, switch 3.363167%,
             64 live targets 72.024907%, 65 live targets 78.337124%. The bootstrap
             upper bounds for four cells exceed the gate.
NEXT_LEGAL_BLOCKS: none from this worker; A3 remains blocked on accepted A2C state.
MAINTAINER_DECISION_REQUIRED: yes — candidate disposition and any formal benchmark
                              recalibration/rescope are maintainer-only decisions.
```

