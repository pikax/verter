# C1 AC5-authority registration rebase proof

## Identities

- Old base: `b9a1b5b2f5e6d689de89447ebc00cc37f9f6453b`
- Old C1 tip: `6d5c362e72a8b48fc8d3bc004008f0390b7744ef`
- Old tree: `c57f3cf774d3a5e29387e92b007f4ee58a700a3c`
- Registered AC5 authority commit/new base: `570c8f34660df8c354a0325e524f0d46e402e1fe`
- Registered trunk tree: `3a7e2b7c245e9896407f70776450365bc65c21e1`
- Conflict-free rebased tip before this evidence update: `4fa5c6c6c28a25128a079981712fc56495d4098c`
- Rebased tree before this evidence update: `e60ae680622b133b8ab71ea453edabcd9eba3fb1`
- Replayed commits: `187`

`git rebase 570c8f34660df8c354a0325e524f0d46e402e1fe` completed successfully without stopping. No conflict resolution was performed.

## Ancestry and content preservation

- `git merge-base --is-ancestor 570c8f34660df8c354a0325e524f0d46e402e1fe HEAD` returned success.
- Old delta `git diff --binary --full-index b9a1b5b2f..6d5c362e7` SHA-256: `024556062d5fbaf20438c09d123668f2ef3d409ce235902bbfd1000d617c723a`.
- Rebased delta `git diff --binary --full-index 570c8f346..4fa5c6c6c` SHA-256: `024556062d5fbaf20438c09d123668f2ef3d409ce235902bbfd1000d617c723a`.
- `cmp` over those binary patches: `PATCH_IDENTICAL`.
- Per-file C1 blobs: `391` checked, `0` mismatches; deleted paths were compared as absent on both tips.
- The AC5 authority registration changed `6` paths and the old C1 delta changed none of them. The sorted path intersection was empty, proving no file-level or field-level collision required interpretation.

## Registered authority inherited exactly

The rebased branch and `570c8f346...` have identical Git blobs for the registry, ledger, and durable AC5 ruling:

| path | Git blob | SHA-256 |
|---|---|---|
| `docs/arch/architecture-lock/ledger/authority-registry.toml` | `b1847b952956eac8b70906f6e319a58f04c83ced` | `94ad23f242518df4d30bb1adc51cd849d053fca870dc6b435150701747e7c5ed` |
| `docs/arch/architecture-lock/ledger/program-state.toml` | `8087b7cc526f4477e96dc08baad0cb30fbb12d4b` | `e20a6d6db30612a7eee5619171897a24e465bb1b3eee61a6b182f2fac0ca6ce7` |
| `docs/arch/refactor/rev11/rulings/ARCHITECT-RULING-2026-08-26-C1-AC5-GAP3-AUTHORITY.md` | `495f52fd7f016cbdd00e6868ba4f4673e94a705f` | `7b26645f49a62a7c5de06c6630a87f0d934ae06898c98aedf6a7e46c600ee2de` |

The candidate-owned consultation artifacts also retain the orchestrator-provided bytes:

- `ac5-authority-prompt.md`: `5d383b3388bc90eae8fd20df7cb3c066201809567f4de861e5c3042bb597ff9a`
- `ac5-authority-output.md`: `d0b450f23f6a2c81c923d466195f27c54177733c6588b6f2139d826c94cef396`

The registered performance authority remains independent and byte-preserved. `C1-A6-WALL-REL-001` and successor `C2-AC-C1-A6-CONTINUATION-001` are not reused as GAP3 authority; AC5 uses the distinct successor `C2-AC-C1-GAP3-TYPEINFO-GATEWAY-001`.

## Live-validator overlay

Candidate-branch edits to `authority-registry.toml` or `program-state.toml` are forbidden. A temporary external overlay at `/tmp/c1-program-state-overlay-4fa5c6c6.toml` changed only the current C1 identity fields:

- `implementation_candidate_sha`: `6d5c362e7...` -> `4fa5c6c6c...`
- `candidate_sha`: `6d5c362e7...` -> `4fa5c6c6c...`
- `candidate_tree`: `c57f3cf77...` -> `e60ae6806...`

Every dispatched, measured, evidence, review, and accepted identity remained byte-identical. The overlay SHA-256 is `6ec7e1527d02d7a7eca2f0f928fe60be762a3bc05aec520c64a9ec4b5876208f`; the inherited ledger SHA-256 is `e20a6d6db30612a7eee5619171897a24e465bb1b3eee61a6b182f2fac0ca6ce7`.

The exact live validator passed against the overlay:

```text
node scripts/validate-program-state.mjs \
  --dag docs/arch/refactor/rev11/program-dag.toml \
  --state /tmp/c1-program-state-overlay-4fa5c6c6.toml \
  --authority docs/arch/architecture-lock/ledger/authority-registry.toml \
  --mode live
```

Result: `OK`, `69` blocks, non-zero work asserted. Trunk will record the accepted final identity during landing, avoiding a self-referential candidate rebase loop.

## AC5 disposition now operative

- `C1-AC-5A-MODULE-RESOLVER`: accepted for both production module-resolution attempts, immutable observation snapshots, the exhaustive workspace retry driver, and C1-AC-9 path-probe/realpath/package-manifest I/O conversion.
- `C2-AC-C1-GAP3-TYPEINFO-GATEWAY-001`: `CODEX-DEFER` to C2 under the registered AC5 ruling. The absent `TypeInfoCore::attempt(NonFlowOperation)` gateway supplies no C1 evidence and is not called complete.
- `C2-AC-C1-A6-CONTINUATION-001`: remains a separate performance-continuation obligation with its existing bytes, owner, and tests unchanged.

RESULT: REBASE PASS; AC5 AUTHORITY OPERATIVE; STEP 6 MAY CONTINUE

## Source-policy authority rebase (final Step-6 rebase)

- Previous tip: `f8191bac45436d6618d397866d206c1898dab376`, tree
  `7096e96256e7a6e5e9103ff6bfafacf68da12f33`.
- Registered source-policy authority/new base:
  `23f60303f28a0a53b4fe6aa750887bb5f2a46b14`, tree
  `95e87e54db705ca446f7cc604b84d4dd0f5e7d6c`.
- Rebased production/evidence subject before final evidence-only commits:
  `6fd3356e3d1ec7d21e4f03850a224283ef43371e`, tree
  `e94f502da626c9062fff54c442d51d90d6e097e2`.
- Replayed commits: `191`.
- The one historical add/add stop was
  `a6/wall-diagnostic.md`: the replayed historical commit carried the earlier
  `ca1701b43bbd40c5a15d797647d8b4fe1427156297cd4966e7acd72b2347453`
  diagnostic, while trunk already carried the later authoritative
  `63c632006b5f5df404876389f48c7b1e7858919388f736f52c3fa149ab44ebb9`
  bytes. Replaying that historical side and the later branch update produced the exact trunk/final
  bytes. No semantic merge or conflict interpretation occurred.
- `git merge-base --is-ancestor 23f60303f... 6fd3356e3...` returned success.

The nine originally rejected files are byte-identical at the pre-rebase tip, rebased subject, and
registered trunk base:

| path | SHA-256 |
|---|---|
| `evidence/C1/a6/performance-authority-output.md` | `458da29abb693cd6336e8da9efdf46edf6438cc2f6ba5b245bf03a1f749caed3` |
| `evidence/C1/a6/performance-authority-prompt.md` | `c032d04269b625dda393124c4f5720cdcf87a4ee4bb4d923503edc0eae8d0ca5` |
| `evidence/C1/a6/residual-244-diagnostic.md` | `3e28f8b2bd15c954c2342015732d92edc0ace214f60e2a6b743a8a01bb7e90ea` |
| `evidence/C1/a6/unblock-architecture-consult.md` | `7531f5811957eb5cc0fb0a71f0a43502c24e22ed37a3d1f99e1fe382110df7a8` |
| `evidence/C1/a6/wall-diagnostic.md` | `63c632006b5f5df404876389f48c7b1e7858919388f736f52c3fa149ab44ebb9` |
| `evidence/C1/ac5-authority-output.md` | `d0b450f23f6a2c81c923d466195f27c54177733c6588b6f2139d826c94cef396` |
| `evidence/C1/ac5-authority-prompt.md` | `5d383b3388bc90eae8fd20df7cb3c066201809567f4de861e5c3042bb597ff9a` |
| `rulings/ARCHITECT-RULING-2026-08-26-J1-RESTRUCTURE.md` | `a83195ea292b39c25715ef42d7dcab17357d4c4bea0a52e88196bb7b65fc73e4` |
| `rulings/ARCHITECT-RULING-2026-08-26-TCM0-REMEDIATION.md` | `d5aca4b4b5c42a82bfb77f1cc9a91074004c6876a7532850306622b703ff66c7` |

The orchestrator-owned untracked prompt was moved aside before rebase, compared byte-for-byte with
the trunk add, and inherited at SHA-256
`2bbd80e1f6efde434c5f5d477818d6a0fbfd6e814fa890052f915e8ca094a937`.
The exact machine-marker selector then passed (run `3b38b5b3-f990-47da-89fb-ebbfd0ceab10`), and the
complete package including all exception mutations passed 187/187 (run
`4de9f87e-438d-4488-bd24-2ef6bc64312e`).

The candidate-owned live overlay changed only C1's three active identity fields from
`f8191bac4...` / `7096e962...` to `6fd3356e3...` / `e94f502d...`; dispatched, measured, evidence,
review, and accepted identities were untouched. Inherited state SHA-256:
`cd11d0b7d6d455ec025a59fde658e403c5a80a91c35145b9e85479f078d0544c`; overlay SHA-256:
`56a49d64ece58bdf5641ed634c408095166d64f8e4f8fa2ccc2c6908ad0c9a02`.
The exact live validator passed 69 blocks against
`/tmp/c1-program-state-overlay-6fd3356e.toml`. Candidate-branch registry and program-state bytes were
not edited.

RESULT: FINAL AUTHORITY REBASE PASS; EXACT EVIDENCE ADMISSION INHERITED; LIVE VALIDATOR PASS
