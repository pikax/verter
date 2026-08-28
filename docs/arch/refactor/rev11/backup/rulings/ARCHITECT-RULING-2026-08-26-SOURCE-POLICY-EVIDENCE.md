# Architecture ruling — source-policy authority evidence

Status: **RATIFIED**

## Dispatch record

- Input: `docs/arch/refactor/rev11/evidence/C1/source-policy-authority-prompt.md`
- Reviewed candidate: `f8191bac45436d6618d397866d206c1898dab376`
- Prompt SHA-256: `2bbd80e1f6efde434c5f5d477818d6a0fbfd6e814fa890052f915e8ca094a937`
- External raw-output SHA-256: `7c68c99de6bd967e349b0702892f7c76df1e8807e5894f23952315e7459dad2e`
- Exception manifest SHA-256: `d9f16e414d317b4426192994352e64abcd3ddf0fe27367c0a3fa54a0fa31cd78`

The raw architect output remains external because future machine-bound raw logs default to external,
digest-bound bundles. This durable instrument is the portable operative rendering. It preserves the
architect's decision, exact inventory, required proof, ownership, and final receipt without copying
machine-local absolute links into portable repository documentation.

## Ruling

The candidate is blocked, but the blocker is not a C1 production-portability defect. It is a
guard-scope and evidence-governance conflict plus two inherited trunk defects.

### Invariant

Machine-specific paths must not influence build, test, runtime, generated output, or portable
documentation. Exact authority evidence may record the environment where it was produced; once
digest-bound, those bytes belong to an integrity rail and must not be normalized silently.

Retain whole-tree scanning. Do not exempt `docs/`, `evidence/`, rulings generally, file suffixes, or
"non-production" content. A marker-bearing file is admitted only through the narrow authority-evidence
rail below.

### Exact admission rail

`docs/arch/portability-machine-marker-evidence-exceptions.tsv` contains exactly nine rows. Every
tracked file remains readable and scanned. A marker hit is admitted only when all of these checks pass:

1. The repository-relative path equals one manifest path exactly. Globs, suffix matches, directory
   exemptions, backslash aliases, parent traversal, and duplicate rows are invalid.
2. The worktree bytes hash to the row's lowercase SHA-256 exactly.
3. The row's existing pin document exists, is tracked, and contains that exact digest.
4. The row class confines the target to either the Revision 11 authority-evidence root or the Revision
   11 rulings root.
5. This ruling pins the exception manifest's own SHA-256 exactly.
6. Every row is live: its exact tracked file still contains a machine marker. Removing the marker without
   retiring the row fails; removing the row without removing the marker also fails.

The nine admitted identities are:

| Class | Exact repository path | SHA-256 | Existing pin document |
|---|---|---|---|
| Authority evidence | `docs/arch/refactor/rev11/evidence/C1/a6/performance-authority-output.md` | `458da29abb693cd6336e8da9efdf46edf6438cc2f6ba5b245bf03a1f749caed3` | C1 performance-authority ruling |
| Authority evidence | `docs/arch/refactor/rev11/evidence/C1/a6/performance-authority-prompt.md` | `c032d04269b625dda393124c4f5720cdcf87a4ee4bb4d923503edc0eae8d0ca5` | C1 performance-authority ruling |
| Authority evidence | `docs/arch/refactor/rev11/evidence/C1/a6/residual-244-diagnostic.md` | `3e28f8b2bd15c954c2342015732d92edc0ace214f60e2a6b743a8a01bb7e90ea` | C1 performance-authority ruling and ledger |
| Authority evidence | `docs/arch/refactor/rev11/evidence/C1/a6/unblock-architecture-consult.md` | `7531f5811957eb5cc0fb0a71f0a43502c24e22ed37a3d1f99e1fe382110df7a8` | C1 performance-authority ruling and ledger |
| Authority evidence | `docs/arch/refactor/rev11/evidence/C1/a6/wall-diagnostic.md` | `63c632006b5f5df404876389f48c7b1e7858919388f736f52c3fa149ab44ebb9` | C1 performance-authority ruling and ledger |
| Authority evidence | `docs/arch/refactor/rev11/evidence/C1/ac5-authority-output.md` | `d0b450f23f6a2c81c923d466195f27c54177733c6588b6f2139d826c94cef396` | C1 AC5/GAP3 ruling and rebase proof |
| Authority evidence | `docs/arch/refactor/rev11/evidence/C1/ac5-authority-prompt.md` | `5d383b3388bc90eae8fd20df7cb3c066201809567f4de861e5c3042bb597ff9a` | C1 AC5/GAP3 ruling and rebase proof |
| Inherited ruling | `docs/arch/refactor/rev11/rulings/ARCHITECT-RULING-2026-08-26-J1-RESTRUCTURE.md` | `a83195ea292b39c25715ef42d7dcab17357d4c4bea0a52e88196bb7b65fc73e4` | Ratified J1 program-state pin |
| Inherited ruling | `docs/arch/refactor/rev11/rulings/ARCHITECT-RULING-2026-08-26-TCM0-REMEDIATION.md` | `d5aca4b4b5c42a82bfb77f1cc9a91074004c6876a7532850306622b703ff66c7` | Stored, explicitly unratified TCM0 program-state pin |

No byte in those nine files is changed. Their existing rulings and pins require no re-registration or
re-ratification.

### Ownership and retirement

The seven C1 artifacts remain exact authority evidence. They may retire only through a separately
ratified external bundle or successor that preserves their authority; C1 does not rewrite them.

The two inherited ruling links are genuinely nonportable and do not become C1 cleanup:

- J1/program authority must issue a portable successor or formally re-ratified rendering before J1
  acceptance, update the J1 program-state digest, and retire the J1 exception row.
- TCM0/program authority must issue portable input before the remediation ruling is ratified or TCM0 is
  dispatched. The stored unratified instrument remains the received historical input; it is not edited
  and presented as the same receipt. Registration of the portable input retires the TCM0 exception row.

The row retirement is each owner's acceptance test. C1 is neither authorized nor required to repair
either inherited ruling.

### Proof and effect

Executable proof must reject altered admitted bytes, a manifest-only digest change, unlisted
marker-bearing evidence, targets outside the permitted roots, wildcards, duplicate paths, malformed
digests, missing pin documents, stale rows, row deletion with a live marker, and marker deletion with a
retained row. The exact selector, complete `tracked_paths_no_machine_roots` family, all
`verter_source_policy_gate` tests, and live program-state validator remain required; the canonical gate
runs at landing.

After this registered trunk act is inherited, all nine digests are rechecked unchanged, and the exact
selector passes, C1 may proceed to final freeze and reviews. This resolves only the source-policy blocker;
all other C1 landing and performance gates remain binding.

```text
LANE: c1-source-policy-evidence-authority
REVIEWED_SHA: f8191bac45436d6618d397866d206c1898dab376
VERDICT: FAIL
BLOCKERS: exact selector is red; registered content-addressed authority-evidence rail and owner-bound J1/TCM0 dispositions are absent
OPERATIVE_ACTS: 1) ratify and land exact path+SHA-256 evidence admission on trunk 2) register its ruling and inherited cleanup ownership 3) rebase C1 without changing the nine bytes 4) run mutation proofs, exact selector, source-policy suite, validator, and refresh frozen evidence
RATIONALE: portability governs operational and portable artifacts; immutable machine-bound authority evidence requires exact integrity admission, while inherited nonportable rulings remain their owners' cleanup
```
