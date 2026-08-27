# C1 exact-candidate record

- Base: `d1f3d50a948597f036868543b9bb21acacd730ff`, tree
  `2e7cf8637ec5c52b0fa04572d99672b052f1f85f`.
- Production: `2820cf2eb790caffdb69f59bc20402d7d0a6647b`, tree
  `ef8efbec06c8e87d1d6d72d9ea8e69fa624f515b`.
- Reviewed candidate: `c46c60c52f33784356a9f1d7fade31627486e874`, tree
  `031c84419aaa1bc851c24e31add987c9ad678ba8`.
- Step-6 manifest: `evidence-manifest.tsv`, SHA-256
  `5a845946da8f9956d325172e66fe51754bc8b0e3cae5dbc83c760a65b8d6e630`.
- Review result: conformance PASS; architecture PASS; adversarial/performance-memory PASS; all
  receipts name the reviewed candidate exactly and report no findings.
- Performance: literal failures and exact dispositions remain as registered; no result or threshold
  is relabelled.
- Acceptance: unset. Independent verification, canonical gate, landing equivalence, landing, and
  maintainer acceptance remain pending.

This record binds the reviewed candidate, not its containing evidence commit. Rev11 requires an
exact delta reattestation if the containing commit becomes part of C1's landing delta.
