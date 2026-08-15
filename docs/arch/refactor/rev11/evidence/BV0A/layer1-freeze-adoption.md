# Layer-1 semantic specification — freeze adoption record

**Artifact:** `packages/framework-conformance-harness/spec/assembled-map-composition-layer1.md`
**Adopted blob:** `0ea47424acfbd4913e11f16156baa597216c84fb`
**Adopted commit:** `6317cadd5` on `work/bv0a-layer1-spec` (rebased onto
`program/architecture-lock` @ `a30c1a2b0`; blob hash verified identical before and
after the rebase — nothing lost in the rebase).
**Specified against tree:** `program/architecture-lock` @ `20b03aaf1` (the pre-rebase
tip at authoring time; §11.5 of the document states what happens when a cited
implementation later changes).

This is the frozen layer-1 semantic specification required by
[AMD-008 §2 item 1](../../amendments/AMD-008-bv0a-assembly-neutral-exit.md) and
[BV0A.md](../../charters/BV0A.md): the pre-assembly DTO schema, the exhaustive
`UncomposableInputMap` validation order/taxonomy, the chaining/transform algebra for
both authorized rewrites under real `CodeTransform` semantics, the exhaustive
assembler write-site manifest, and the canonical output-artifact schema. Per AMD-008,
neither the independent JavaScript reference nor the production Rust implementation
may be written against it before this freeze is recorded.

## Review process

Seven full review rounds, twenty-one independent blind dispatches (conformance:
Codex xhigh; architecture: fresh Claude session; adversarial/governance: grok-4.6
xhigh — no mandate ever resumed across rounds). Findings narrowed monotonically:
13 → 8 → 7 → 5 → 3 → 2 → 0. Final round: architecture PASS, adversarial PASS
("freeze this text, do not open another prose-precision round"), conformance PASS
(including a 54,558-case brute-force adversarial comparison against the core
emission-standard proof, zero discrepancies, plus 20+ independent citation
spot-checks against real source). Full per-round transcripts are not durably
committed (they were an implementer session's own scratchpad output); this record
and the document's own §12 revision history are the durable trace.

Independently re-verified by the program orchestrator before this record was
written: the adopted commit is correctly rebased onto the current
`program/architecture-lock` tip (`git merge-base --is-ancestor` confirmed); the
blob hash matches; the branch has zero ancestry from either superseded BV0A
attempt (`work/bv0a-implementation`, `work/bv0-relanding` — confirmed via
`git merge-base --is-ancestor`); zero occurrences of identifiers unique to the
superseded attribution-matching design (`violation_key`, `probe_fragments`,
`violation_multiset`); and one technical citation (the `__sfc__`/export-default
rewrite logic, `crates/verter_session/src/compile.rs`) spot-checked directly
against source and confirmed accurate.

## Disposition against the FC-VUE-003 resolution gate

This record closes gate check 1 (layer-1 completeness) from
[`debt-layer1-gate-authority.md`](debt-layer1-gate-authority.md) — the document's
own §10 maps every requirement of AMD-008's umbrella description and the debt
record's own text to the section that answers it, and this was independently
confirmed, not merely asserted by the document.

Checks 2 (non-retroactive chronology — no reuse of a pre-freeze prototype) and 3
(maintainer adoption record, distinct from this freeze) remain, as designed, for
BV0A's own acceptance review to close, using this record's contamination
attestations as evidence. This freeze record is not itself that acceptance;
it authorizes starting the independent JavaScript reference and production Rust
work, nothing more.

## Next

The independent JavaScript reference implementation and the production Rust
`assemble_vue_main_module` correction may now be written against this frozen
specification.
