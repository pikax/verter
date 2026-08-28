# Program-state transition for AMD-005

## Candidate shape

The amendment adds BF1, BF2, BF3, BV1, and BS1, changing the block universe from 51
to 56. The DAG, template, and tracked live ledger contain the same IDs in the same
order. New ledger rows are initialized as follows:

The amended `program-dag.toml` SHA-256 is
`335e0863ba1f21473a24befc0093dc01bad4f065ff03e6716c113448be054489`.

```toml
status = "LOCKED"
charter_digest = ""
context_packet_digest = ""
base_sha = ""
candidate_sha = ""
candidate_tree = ""
accepted_sha = ""
accepted_tree = ""
landing_equivalence_digest = ""
evidence_digest = ""
stack_id = ""
stack_snapshot_digest = ""
stack_layer = 0
conformance_review = "PENDING"
architecture_review = "PENDING"
adversarial_review = "PENDING"
maintainer_decision = "PENDING"
notes = "Introduced by proposed AMD-005; not dispatchable before ratification and predecessor acceptance."
```

No accepted row is changed. B1 is already accepted at
`03b2fdbfc6d12452824768d9e389a5f6f3d680df`, tree
`7f8230066735db17650b5d594a95d597540b3729`, and that accepted ledger fact is
preserved byte-for-byte by the amendment. The tracked live ledger's DAG digest is
updated solely so the amended shape can be validated; that mechanical binding is not
ratification or a state transition.

## Exposure sequence after ratification

1. Land the exact challenged package on `program/architecture-lock` and record the
   maintainer decision/digests.
2. Set BF1 `READY`; all other new blocks remain locked.
3. Accept BF1 before BF2; BF2 before BF3; BF3 before either B2 or B3.
4. B2/B3 concurrency requires exact disjoint ownership. B4 waits for both.
5. BV1/BS1 concurrency requires exact code/data/lease disjointness. B5 waits for both.
6. C4 waits for B6 and C3.

No amendment proposal, package commit, or challenge report independently changes a
block status. Every normal candidate/review/maintainer transition remains governed by
Revision 11.
