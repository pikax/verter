# Maintainer ruling of record — AMD-010

Reproduced verbatim below as issued, except that the amendment identifier it uses was
renumbered from its draft number to `AMD-010` by a separate maintainer ruling; the audit
amendment keeps the earlier identifier. That renumbering is clerical and changes no
substance. All four rulings below apply.

The "Sequencing" section below is labelled in its own text as orchestrator-owned and not
maintainer-decided, and the amendment's §7 says the same of itself. The program orchestrator
directed this amendment to land after BF3's unit landed rather than after BF3's acceptance; the
constraint's stated purpose — not advancing BF3's base while it is mid-flight — is satisfied,
since BF3's unit landed first and this lands as a separate squashed unit on top. BF3 is not
accepted, and nothing here accepts it.

---

# Maintainer ruling — AMD-010 (recorded 2026-08-16)

Ratifier: Carlos Rodrigues <carlos@hypermob.co.uk> (GitHub: pikax), designated maintainer.
Solicited by: program orchestrator, after two independent architecture consults (the second
run unprimed) and a third verifying drafting pass independently converged on the same
diagnosis. Both consults stated that ratification is maintainer-reserved, citing
`docs/arch/refactor/rev11/governance.md:12-19` and `:285-299`.

## Ruling 1 — AMD-010 breadth

RATIFIED AS DRAFTED (the NARROW form): 3 documents, 4 deltas.

- `charters/B3.md:3` — predecessor corrected to `BV0` and `BF3` (the charter currently
  contradicts ratified AMD-006 and the authoritative `program-dag.toml`).
- `charters/B3.md:16-18` — B3 assigned bounded ownership of option admission,
  unknown-option refusal, and mechanical conversion of every currently reachable
  production route into the canonical request. Later owners retain route replacement,
  publication, final carriers, and equivalence proof.
- `AMD-005:129-130` — superseded so K2 owns final typed carrier representation and
  `Any + Send + Sync` removal, NOT the initial conversion B3 acceptance requires.
  AMD-005 §6 (`:132-144`) is expressly PRESERVED.
- `charters/B2.md:15-17` — exit E1 corrected to require no blocked B2 PARSE FACET; an
  aggregate official-case row leaves `blocked` only once every applicable facet closes.

NOT amended (claims verified as unsupported by the drafting pass, against the first
consult's recommendation): `program.md:413-417` and
`evidence/framework-conformance/product-inventory.md:37-40`.

The emitter-mapping ledger ingress rows remain the recorded, explicitly-labelled broader
ALTERNATIVE, DEFERRED — not adopted by this amendment.

No DAG change. Moving K2 earlier is impossible: `B3 -> B4 -> BV1/BS1 -> B5 -> B6 -> K2`
is a cycle.

## Ruling 2 — B3 conversion boundary (drafter's Q-6)

B3's option-conversion obligation STOPS AT THE OUTERMOST RUST INGRESS (NAPI, WASM host,
FFI, session profile, compiler). B3 does NOT extend into the `packages/unplugin` or
`packages/wasm` public JS/TS surfaces.

Binding condition: every residual JS/TS silent-ignore must be assigned a NAMED LATER
OWNER in the amendment; none may be left unowned. This includes the verified defect that
`opts.template` is consumed nowhere in `packages/unplugin` (which also falsifies that
package's own documented forwarding claim).

## Sequencing (orchestrator, not maintainer-decided)

AMD-010 MUST NOT be committed until BF3 lands. Committing to
`program/architecture-lock` now would advance BF3's base and destroy its linear
fast-forward landing. The amendment lands as its own squashed unit AFTER BF3 acceptance.

## Related standing ruling (no amendment required)

B2 and B3 SERIALIZE: B2 lands first, then B3 rebases onto B2's accepted tree. Range-level
disjointness can satisfy AMD-005 §4 in principle, but the available proof fails the bar —
`crates/verter_compiler/src/framework_common/carrier_compiler.rs` is a third shared file
and its `parse` / `compile_bundle` are members of ONE trait declaration (`:684-761`), which
no range-level proof can split. This is execution serialization between siblings, NOT a new
semantic DAG edge. B4 remains gated on both.

## Ruling 3 — JS-1 disposition (recorded 2026-08-16, same maintainer)

Maintainer: "verter is strict by default, `[key: string]: unknown` is the default vue less
strict type. Verter needs to be strict, verter types are not too bad as they stand now,
just a few bugs here and there."

JS-1 is therefore RESOLVED and requires NO owner assignment, NO new program block, and NO
amendment delta. The open `[key: string]: unknown` index signature at
`packages/unplugin/src/core/types.ts:44-53` is not a legitimate public surface awaiting an
owner — it is a defect: a loose Vue-inherited shape that silently accepts and drops options
Verter never consumes.

Disposition: ADOPT-NOW as an ordinary defect fix — narrow the public option type to a
strict, closed, typed surface (the preparer's recommended option 3). Handled under the
standing project rule that a wrong/silently-ignored result is a bug to FIX test-first, never
a tracking or guard mechanism. It is NOT a program-block concern and does not gate B3
dispatch, AMD-010 ratification, or any ledger transition.

Sequencing: subject to the same commit embargo as everything else — lands as its own unit
after BF3 accepts.

Consequence for the amendment: §8.1's escalation of JS-1 as an unowned residual is CLOSED.
The binding condition from Ruling 2 ("every residual JS/TS silent-ignore must be assigned a
named later owner, or an explicitly escalated unowned record") is satisfied for JS-1 by this
maintainer disposition, which supersedes the escalation.

## Ruling 4 — JS-1 scope (recorded 2026-08-16, SUPERSEDES Ruling 3's disposition)

Maintainer: "verter vue public types will be handled by me after the program is done."

JS-1 is OUT OF PROGRAM SCOPE. Ruling 3's substance stands unchanged — Verter is strict by
default, and the open `[key: string]: unknown` at
`packages/unplugin/src/core/types.ts:44-53` is a defect rather than a legitimate public
surface — but its DISPOSITION changes from ADOPT-NOW to maintainer-owned post-program work.

- Owner: the maintainer, personally.
- Resolution gate: after the architecture program completes.
- The program performs NO fix, NO tightening, and NO further investigation of Verter's Vue
  public TypeScript surface. A broader public-TS looseness sweep was dispatched under
  Ruling 3 and was STOPPED on this ruling; it produced no adopted findings.
- Ruling 2's binding condition ("every residual JS/TS silent-ignore is assigned a named
  later owner, or an explicitly escalated unowned record") remains SATISFIED for JS-1: the
  named owner is the maintainer, post-program.
- Unchanged: JS-1 gates nothing — not AMD-010 ratification, not B3 dispatch, not any ledger
  transition. The ratified delta set stays exactly 3 documents / 4 deltas.

Standing consequence for the rest of the drive: do not open Verter Vue public-type work,
and do not fold public-TS type tightening into any program block's scope.
