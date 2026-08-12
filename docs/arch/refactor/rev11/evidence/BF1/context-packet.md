# BF1 context packet

BF1 ("Framework compiler contract and compatibility lock") is a gap-analysis-only
block per its charter (`docs/arch/refactor/rev11/charters/BF1.md`). Verbatim record
of the bounded context given to the implementer, per governance's L2 (block
context) scope.

## Charter (binding)

`docs/arch/refactor/rev11/charters/BF1.md` — owns exact Vue/Svelte compatibility
domains, official package pins/integrity, product-boundary glossary, capability
matrix, public/default route reachability, maturity classification, complete
Vue/Svelte option inventories, supported/unsupported/experimental/projection-required/
version-incompatible cells, conformance acceptance IDs, official-case manifests,
golden/normalizer contracts, performance cells locked before candidate implementation,
and affected DAG/charter/capability/gate amendments. Must not implement broad
compiler fixes.

## Scope given to implementer

Verify each of BF1's 7 numbered exit criteria and owned-scope bullets against the
already-landed AMD-005 (Framework Compiler Conformance Rescope) package. AMD-005 was
maintainer-ratified prior to BF1's dispatch and already delivers every artifact BF1's
charter requires (version-domain manifest, product-boundary glossary, option
inventories, capability matrix, official-core oracle contract, exclusion contracts,
conformance/golden contract, official case manifests, normalizer contract,
performance-impact lock). If genuinely satisfied, BF1 requires zero new content — do
not fabricate placeholder work to justify a landing.

Predecessor: B1, accepted `03b2fdbfc6d12452824768d9e389a5f6f3d680df`; AMD-005 ratified
and landed prior to dispatch.

## Outcome

Implementer confirmed all 7 exit criteria and owned-scope bullets already satisfied
by the landed AMD-005 package — zero new production or evidence content required, no
unique implementer commit. Three independent review mandates dispatched:
conformance PASS (capability-matrix `VERIFY` rows confirmed deliberate
execution-deferred disposition, not unresolved debt; one non-blocking stale-prose
discovery), architecture PASS (DAG sequencing `B1->BF1->BF2->BF3->{B2,B3}` confirmed
coherent, zero production diff confirmed, emitter-mapping dispositions spot-checked
against live source), adversarial found one BLOCKING governance finding (AMD-005
§15.1's ratification quotation cited the wrong reviewed-package SHA — the pre-fix
`BLOCKING_FINDINGS` commit `ce1d0e4688` instead of the PASS-reattested commit
`7442bb9060`; substance/bytes/attestations were always correct, citation only).
Escalated per STOP conditions (outside BF1's write set, touches a maintainer-signed
ratification record); architecture-authorized as non-discretionary record repair (no
new maintainer ratification required per governance §10), landed as one standalone
commit `f1b59d2dd`. Scoped reattestation confirmed the fix minimal, correct, no
regression. All three mandates closed PASS on the final candidate (BF1's own
identity is exactly `f1b59d2dd`, since it required no additional content beyond
that fix).
