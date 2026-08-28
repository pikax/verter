# AMD-015 — B1 covers aligning a normative contract with the identity vocabulary it specifies

**Status:** RATIFIED WITH CORRECTIONS 2026-08-25 by the codex architect, to whom the
maintainer delegated amendment ratification. See §7.

**Prepared against:** local `program/architecture-lock` commit
`fdf754c882c1c8a4ff4cac7395326eeef6fe46a3`.

**Amends on ratification:** three files —
[`../charters/B1.md`](../charters/B1.md), the B1 `charter_digest` in
[`../../../architecture-lock/ledger/program-state.toml`](../../../architecture-lock/ledger/program-state.toml),
and the B1/AMD-015 document records and B1 authorization record in
[`../../../architecture-lock/ledger/authority-registry.toml`](../../../architecture-lock/ledger/authority-registry.toml).
It adds one acceptance row and rebinds the exact authority bytes. **It changes no
DAG edge, no block class, no predecessor, no capability-matrix cell, and no
performance gate. It does not alter B1's accepted status or re-open any accepted
candidate.**

## 1. Why this exists

B1's *Required final state* opens with:

> distinct identity/profile/mapping/result-contract types from `architecture.md`;

That row names one authority — `architecture.md`. The identity vocabulary it
covers is, in several cases, ALSO specified by a normative contract under
[`../contracts/`](../contracts/), and the two are not automatically identical.
When they disagree, implementing the row requires choosing one and correcting the
other. B1 has no acceptance row covering that correction, so the work is real,
required, and uncovered.

This is not hypothetical. `contracts/parse-ownership.md` §2 and `architecture.md`
§6.1 declare the same `ParseInstanceId` with different names for its generation
type — `ParseGeneration` and `ParseInstanceGeneration` respectively. A
declaration can carry only one of them. Whichever it carries, one normative
document is then wrong, and nothing in the program would report it.

## 2. Why the row is written as a class

The immediate instance is a one-token nominal-type rename. The repeatable class is
a lexical mismatch between `architecture.md` and a normative contract for a type
B1 declares. It applies only when the documents already agree on every non-lexical
part of the declaration and contract.

This amendment does not treat fields, variants, or carried semantic types as mere
spelling. Adding, removing, renaming, or reordering a field or variant, changing a
carried semantic type, or changing an invariant, owner, lifetime, compatibility,
behavior, or acceptance obligation is contract substance and remains an
abort/rescope condition. For the lexical class only, `architecture.md` supplies the
identifier spelling because B1's existing Required final state row already names
`architecture.md` as the type-vocabulary source.

## 3. The proposed change

`docs/arch/refactor/rev11/charters/B1.md`, appended to the *Required final state*
list:

```diff
 - no current semantic/cache/parser owner duplicated merely to host new types.
+
+Added by AMD-015 — the list above stays as carried from the template:
+
+- where a normative contract under `contracts/` and `architecture.md` use
+  different nominal identifiers for the same identity, profile, mapping, or
+  result-contract type B1 declares, while agreeing on the declaration's
+  structure, carried semantic types, invariants, ownership, lifetime,
+  compatibility, and behavior, the identifier spelling in `architecture.md`
+  controls and only the divergent type-identifier token or tokens are corrected
+  in the same candidate. This row authorizes no field or variant addition,
+  removal, rename, reordering, or carried-semantic-type change, and no change to
+  a contract invariant, ownership, lifetime, compatibility, behavior, or
+  acceptance obligation. Any such disagreement remains an abort/rescope
+  condition.
```

## 4. Consequences

- Lexical nominal-type-identifier alignment inside B1's declared type surface is
  covered work rather than uncovered work.
- The conformance mandate must compare the nominal type identifiers used for each
  B1-declared type by `architecture.md` and normative contracts and must correct a
  lexical divergence in the same candidate.
- The row confers no authority over a contract's fields, variants, carried semantic
  types, invariants, ownership, lifetime, compatibility, behavior, or acceptance
  obligations; those remain substantive.
- A substantive disagreement, or a disagreement involving a type B1 does not
  declare, remains an abort/rescope condition.

## 5. What this does NOT do

- It does not accept, re-open, unlock, or dispatch B1 or any other block.
- It decides only the lexical class: for a B1-declared type whose non-lexical
  contract already agrees, the identifier spelling in `architecture.md` controls.
  It creates no general precedence rule for substantive disagreement or for another
  block.
- It does not itself edit `contracts/parse-ownership.md`; it binds the candidate's
  correction of only the divergent type-identifier token.
- It changes no ADR, DAG row, ledger block status, capability-matrix cell, or
  performance gate.
- It does not authorize editing the generated consolidated master plan, which is
  convenience output rather than normative source.

## 6. Verification on ratification

1. Apply the §3 diff to `docs/arch/refactor/rev11/charters/B1.md`.
2. Rebind both exact references to the edited charter bytes:
   - `docs/arch/architecture-lock/ledger/program-state.toml`, B1
     `charter_digest`;
   - `docs/arch/architecture-lock/ledger/authority-registry.toml`,
     `B1-CHARTER.sha256`.
   With the §3 bytes exactly as printed, both values are
   `d7ee747f3adc1c045ccdc66f48c73f859d5d11d011886cb574c9e9a009967049`.
3. Add AMD-015 as a digest-bound `AMENDMENT` document in
   `authority-registry.toml`, using the SHA-256 of this amendment after the
   ratification text and all corrections are present; add that document id to B1's
   existing authorization record and update that record's scope to name the narrow
   lexical-alignment authority. Do not create a second B1 authorization record.
4. `program_dag_digest` is unchanged because `program-dag.toml` is unchanged.
5. Run:
   ```sh
   node scripts/validate-program-state.mjs \
     --dag docs/arch/refactor/rev11/program-dag.toml \
     --state docs/arch/architecture-lock/ledger/program-state.toml \
     --mode live \
     --authority docs/arch/architecture-lock/ledger/authority-registry.toml
   ```
   Expected result: 69 blocks validated, zero violations. The explicit authority
   argument proves the edited charter and this amendment are both read and
   digest-bound; an unchanged-result assertion that omits those reads is not
   verification.

## 7. Ratification

**RATIFIED WITH CORRECTIONS**, 2026-08-25, by the codex architect, acting under
the maintainer's delegated amendment-ratification authority. Ratification is bound
to the lexical-only §3 row and the §6 digest/authorization updates. It does not
accept or re-open B1 or accept candidate
`4d7009464ee181f0d6ff9b51e1a16671cba6e883`.
