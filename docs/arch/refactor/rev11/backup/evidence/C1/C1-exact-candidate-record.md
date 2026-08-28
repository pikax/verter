# C1 exact-candidate record

- Base: `d1f3d50a948597f036868543b9bb21acacd730ff`, tree
  `2e7cf8637ec5c52b0fa04572d99672b052f1f85f`.
- Production: `2820cf2eb790caffdb69f59bc20402d7d0a6647b`, tree
  `ef8efbec06c8e87d1d6d72d9ea8e69fa624f515b`.
- Reviewed candidate: `c46c60c52f33784356a9f1d7fade31627486e874`, tree
  `031c84419aaa1bc851c24e31add987c9ad678ba8`.
- Authorized gate/freeze carrier: `a2de5e39070da1ba5718b736f39d46d6f04fc398`, tree
  `c1bf69e65346fe3febfd8ed9eccd27f7e5bf18fa`.
- Accepted C1 commit: `267cfd0079022dd278b2414e209f459f27d6a721`, tree
  `c1bf69e65346fe3febfd8ed9eccd27f7e5bf18fa`, on
  `program/architecture-lock`.
- Step-6 manifest: `evidence-manifest.tsv`, SHA-256
  `5a845946da8f9956d325172e66fe51754bc8b0e3cae5dbc83c760a65b8d6e630`.
- Review result: conformance PASS; architecture PASS; adversarial/performance-memory PASS; all
  receipts name the reviewed candidate exactly and report no findings.
- Performance: literal failures and exact dispositions remain as registered; no result or threshold
  is relabelled.
- Canonical gate: default `node scripts/gate.mjs` exited 0 on the exact carrier; Surface 1 passed
  `25,539/25,539`, with `598` skipped and `35` trybuild cases excluded under the canonical interim
  policy. The shipped-cfg guard remains temporarily skipped. Raw output and telemetry SHA-256 values
  are recorded in `landing-record.md`.
- Acceptance: **ACCEPTED / LANDED** by explicit maintainer direction. `landing-equivalence.md`
  proves the carrier and accepted commit have the same tree and canonical patch, and records the
  atomic true-fast-forward checks.

The three review receipts remain bound to the reviewed candidate and are not restamped. The
maintainer separately authorized the 13-path evidence/program-state-only carrier after the canonical
gate; the landing record discloses that bridge rather than rewriting review identity.
