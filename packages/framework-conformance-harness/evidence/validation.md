# Package validation record

Validated in the package worktree on 2026-08-12. These are documentation/state and
evidence checks; no production compiler build/test, Cargo command, benchmark, broad
Node test, or `scripts/gate.mjs` run was performed.

## Program state

```text
OK: program-state.template.toml (docs/arch/refactor/rev11/templates/program-state.template.toml) — validated 56 blocks (non-zero work asserted) against docs/arch/refactor/rev11/program-dag.toml in mode template
OK: program-state.toml (docs/arch/architecture-lock/ledger/program-state.toml) — validated 56 blocks (non-zero work asserted) against docs/arch/refactor/rev11/program-dag.toml in mode live
```

Commands:

```sh
node scripts/validate-program-state.mjs \
  --dag docs/arch/refactor/rev11/program-dag.toml \
  --state docs/arch/refactor/rev11/templates/program-state.template.toml \
  --mode template
node scripts/validate-program-state.mjs \
  --dag docs/arch/refactor/rev11/program-dag.toml \
  --state docs/arch/architecture-lock/ledger/program-state.toml \
  --mode live
```

An independent predecessor-list comparison against the start commit reported:

```text
OK: amended DAG exact; 51 original blocks retained, 5 added, unaffected predecessor lists unchanged
```

Amended DAG SHA-256:
`335e0863ba1f21473a24befc0093dc01bad4f065ff03e6716c113448be054489`.

## Evidence package

```text
OK: AMD-005 package evidence validated in post-review mode (22718 non-zero assertions)
```

The package validator checks the closed option/capability/emitter/case enums, unique
IDs, exact row counts, official source-object shapes, 25-package Vue and 20-package
Svelte closures, exact resolved dependency-edge manifests, direct
versions/integrities, lock and manifest digests, exact amended predecessor lists, and
the 56-row DAG/state universes. Its explicit review phase then checks either absence
of all three primary report files (`--pre-review`) or their presence, exact reviewed
commit/tree binding, and closed verdict (`--post-review`). Post-review validation is
not a PASS-verdict oracle.

HISTORICAL: this command validated the primary review reports' binding to the
original candidate before the bounded fix round. It does not examine the fix-round
reattestations, so it does not bind to the final `PASS` identity
(`7442bb9060b7faa0720e528d3f96ee1df1abff95`, tree
`69502487b55f87eb7c0c009876865b64397da660`) — see AMD-005 §15.1 for that identity.
Retained here for reproducibility of the original review round only.

```sh
node --check packages/framework-conformance-harness/evidence/generate-official-case-manifests.mjs
node --check packages/framework-conformance-harness/evidence/generate-oracle-closures.mjs
node --check packages/framework-conformance-harness/evidence/validate-package.mjs
node packages/framework-conformance-harness/evidence/validate-package.mjs \
  --post-review \
  --reviewed-commit ce1d0e4688af1b5bd548b6b68286632cc0f7ede8 \
  --reviewed-tree 1ff1f83d8e994b6f1169b0b209c9f557c23f4728
```

Before the three primary reports exist, the corresponding preparation command is
`node packages/framework-conformance-harness/evidence/validate-package.mjs
--pre-review`. It intentionally rejects this attached-report state.

The case manifests contain 2,003 Vue official compiler test declarations and 3,475
Svelte official sample/suite rows (regenerated against svelte@5.56.10 — see
`svelte-case-identity-ledger.md`). Their SHA-256 values are:

- Vue: `76cbe75f5dbee5b6014ab44ec4b5e58ff77a65839fafdc40d7328dda30f456ba`
- Svelte: `0ba28efe7aafde6463d0a0977d8297561525d1c6d4161ffec33d0b8369eaaa3c`

These are blocked seed manifests, not conformance acceptance.

## Source-backed option coverage

The earlier Svelte `25/25` compiler-interface audit was incomplete because it omitted
the separate parse overloads and source-authored custom-element descriptor. The
expanded Svelte TSV has 35 exactly counted rows: the prior compiler/module/optimize
scope plus `svelte/compiler.parse`'s `filename`, `modern`, and `loose`, and
`customElement`'s `tag`, `shadow`, per-prop `attribute`/`reflect`/`type`, and `extend`.
Their sources and boundary treatment are recorded in `option-inventories.md`.

The validator proves the closed classification set, exact row count, and unique
surface/key pairs. BF1 and a fresh independent conformance challenge still own the
semantic correctness judgment; the attached blocking report is not acceptance.
