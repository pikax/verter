# Kernel decision tier

This directory holds the successor decision tier that runs before L4: the
docs-only kernel, project-model, source-module, profile, workspace and
framework-constitution decisions. It was opened by operator ruling **R1 —
parallelisation (2026-10-02)**, which moved these decisions ahead of L4 as
zero-production work.

The plan itself (nodes, charters, readiness) is owned by the TAMA controller,
not by this repository. This page records only the rule every decision in this
tier obeys. It describes the repository at `test(engine): split owner-local
suites and architecture guards (#763)`, 2026-10-07.

## Code guard

**BR0** is the code guard. Every implementation node keeps BR0, and through it
L4-A, as an ancestor (operator ruling R5, 2026-10-02). Code that needs L4's
final evidence closure carries an explicit L4 edge instead. Nothing in this
tier makes code ready; a task behind this tier and not behind BR0 writes docs
only.

## Docs-only rule

Every task behind this tier, and not behind BR0, obeys these four rules:

1. **Own file, own path.** It writes contract text, inventories and case tables
   only, inside its own scoped docs path. Each decision owns its own file under
   `docs/arch/kernel/`, or the path its scope names. The directory listing is
   the index; there is no shared index file to maintain.
2. **No enforcement machinery.** It adds no CI gate, validator, xtask check,
   schema enforcement, compile-fail test, spec file or workflow. Where its
   charter specifies such a check, the check belongs to the receiving lock or to
   the first implementation node that the charter's 2026-10-02 amendment names.
3. **Cite or explain proofs.** For AC2–AC4-style proofs it cites existing
   evidence, or records why the proof does not apply.
4. **Record the head it describes.** It states the repository head its text
   describes, by landing title (first line) and ISO date, never by commit SHA
   or URL. Later L4 changes to the owners it inventories are reconciled by
   the family locks or by the named implementation consumer; the decision is
   not re-dispatched.

## Reconciliation owners

| Lock | Reconciles |
| ---- | ---------- |
| UAI0 | Identity, carrier, parser, and coordinate contracts |
| UAO0 | Activation, observation, TypeInfo, index, and performance contracts |
| UAP0 | Capability, coexistence, rule/action, formatter, and public contracts |
| UAM0 | Manifest, validator, and governance contracts |
