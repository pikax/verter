# A0 Command Proofs — Index (`contracts/baseline-lock.md` §4)

All commands were run against the A0 candidate
`b7ea2dc88bda86473de81de3438b7f88ef30adc7` / tree
`47645406a9246e600af995c62608b709347e13a4` on branch `docs/rev11-architecture-plan`
in the dedicated worktree (referred to below as `<WORKTREE>`; the primary checkout
was never touched). Shared environment for every row: Windows 11 Pro 10.0.26200,
Git Bash (POSIX sh), no extra environment variables set, no cargo features passed
(default features). Toolchain (resolved inside the worktree by the repo's
exact-pinned `rust-toolchain.toml`): rustc 1.97.1 (8bab26f4f 2026-07-14), cargo
1.97.1, cargo-nextest 0.9.137 (75ddba7e9 2026-05-26), node v26.5.0. Raw output
digests are SHA-256 over the raw captured bytes (stdout+stderr interleaved).

| # | exact command | working dir | env/features | exit | executed | skipped/ignored | binaries/packages/fixtures | raw file | raw-output SHA-256 |
|---|---|---|---|---|---|---|---|---|---|
| 01 | `cargo nextest run -p verter_session -E 'test(tracked_paths_no_machine_roots)'` | `<WORKTREE>` | default features | 0 | 5 tests run, 5 passed | 8674 skipped (filtered out by `-E`) | `verter_session::main` test binary (dev profile) over `git ls-files` of the candidate tree | `01-tracked_paths_no_machine_roots.txt` | `53a0149388b72eba5db1b0076e40a2caa85f801a9dacbaf460814ec145490c9e` |
| 02 | `cargo nextest run -p verter_session -E 'test(tracked_paths_are_portable)'` | `<WORKTREE>` | default features | 0 | 12 tests run, 12 passed | 8667 skipped (filtered out by `-E`) | `verter_session::main` test binary (dev profile) over `git ls-files` of the candidate tree | `02-tracked_paths_are_portable.txt` | `e9010268163de006867e606d10bb8f8f29e5633cdf59e97f69000252de92c2b1` |
| 03 | `cargo nextest run -p verter_analysis_inputs -E 'test(analysis_config_paths_never_committed)'` | `<WORKTREE>` | default features | 0 | 6 tests run, 6 passed | 28 skipped (filtered out by `-E`) | `verter_analysis_inputs` test binary (dev profile) over `git ls-files` of the candidate tree | `03-analysis_config_paths_never_committed.txt` | `0e0f37045fddc8871a9786160f4288e6c3ea98b7c2551c487b59cf6da0367661` |
| 04 | `node --test scripts/validate-program-state.test.mjs` | `<WORKTREE>` | none | 0 | 26 tests, 26 pass | 0 skipped | node v26.5.0; `scripts/validate-program-state.mjs`; self-written temp-dir TOML fixtures (no external fixtures) | `04-node-test-validate-program-state.txt` | `f637c76c322bc5d0b80566422eb29951b312768c1e410eabc369e0295a13d230` |
| 05 | `node scripts/validate-program-state.mjs --dag docs/arch/refactor/rev11/program-dag.toml --state docs/arch/refactor/rev11/templates/program-state.template.toml --mode template` | `<WORKTREE>` | none | 0 | 50 blocks validated | 0 | node v26.5.0; the candidate tree's DAG + state template | `05-validator-template-mode.txt` | `e043a503108220665972333b5a9ee38fd8d15437cb7cd25c599fad4f8db0cef9` |
| 06 | `node scripts/validate-program-state.mjs --dag docs/arch/refactor/rev11/program-dag.toml --state <EVIDENCE_ROOT>/program-state.toml --mode live` | `<WORKTREE>` | none | 0 | 50 blocks validated | 0 | node v26.5.0; the candidate tree's DAG + the live external ledger (post-round updates: candidate identity, context-packet digest, evidence digest) | `06-validator-live-mode.txt` | `75f4ddc1bbc103c100dfb918eb8352f14a34464122e8b9e71f6ce6b84b0e8ff9` |
| 07 | stub experiment: copy `validate-program-state.test.mjs` to a scratch dir beside a validator stub containing only `process.exit(0);`, then `node --test <scratch>/validate-program-state.test.mjs` | scratch dir (outside the repo) | none | 1 | 26 tests, 0 pass, **26 fail — the required outcome** | 0 skipped | node v26.5.0; the candidate's test suite unmodified; one-line stub validator | `07-stub-experiment.txt` | `e2bb40f71fe83f830d592a3444b85c290afa75402843c6c8f38bc3b1dd0c6d17` |
| 08 | falsification battery: script mutates copies of the live ledger (stackless READY successor; foundational NOT_REQUIRED acceptance; mismatched/ABORTED/LOCKED/equal-layer stacked predecessor; blank `program_dag_digest`; non-ACTIVE live status; diverged accepted identity without landing equivalence; round-5 blocker reproductions — proven D1 PRIVATE_CHECKPOINT over LOCKED predecessors, unproven D1 checkpoint, wrong-class L4 checkpoint; round-6 cases — accepted root with `entry_lock_digest` emptied, accepted root with the `entry_lock_digest` line deleted, D2 REVIEW with otherwise-perfect stack fields over a proven D1 PRIVATE_CHECKPOINT) and runs the validator `--mode live` on each | scratch dir (mutated ledger copies; validator + DAG from `<WORKTREE>`) | none | 0 (script asserts every case exits non-zero with its targeted message) | 15 mutated ledgers validated, **15/15 REJECT — the required outcome** | 0 | node v26.5.0; `scripts/validate-program-state.mjs`; mutated copies of `<EVIDENCE_ROOT>/program-state.toml` | `08-falsification-rejections.txt` | `a04ebefa21f15744b7fbce5c47a7ddb289039f9e47fb7bccd054b403e43fcc19` |
| 09 | `node scripts/validate-program-state.mjs --dag docs/arch/refactor/rev11/program-dag.toml --state <EVIDENCE_ROOT>/program-state.toml --mode live` (final proof run AFTER A0 acceptance: A0 ACCEPTED with all three mandates PASS, maintainer_decision ACCEPTED, accepted identity = candidate identity; A1 READY; current_block A1; all other blocks LOCKED) | `<WORKTREE>` | none | 0 | 50 blocks validated | 0 | node v26.5.0; the candidate tree's DAG + the live external ledger (post-acceptance state) | `09-validator-live-mode-post-acceptance.txt` | `75f4ddc1bbc103c100dfb918eb8352f14a34464122e8b9e71f6ce6b84b0e8ff9` |

Notes:

- Rows 01–03: nextest "skipped" counts are the package's other tests excluded by the
  `-E` filter expression, not ignored tests within the selected set; each selected
  set executed non-zero work and passed completely.
- Row 07 is a negative control: exit 1 with 26/26 failures is the PROOF (the suite
  cannot pass against an always-green validator). A green run here would be the
  failure.
- Row 08 is the round-6 falsification battery (the round-4 cases, the
  equal-layer case, the three round-5 PRIVATE_CHECKPOINT blocker reproductions —
  which ALL exited 0 before the round-5 fix — plus the two round-6 entry-lock
  cases, which BOTH exited 0 before the round-6 fix, and the round-6
  perfect-stack checkpoint-predecessor case): every mutation was verified to have
  APPLIED (the driver script throws if a substitution target is absent or the
  mutated bytes equal the original), so a rejection is evidence about the
  validator, not about a plant that failed to land. Rejection lines are quoted in
  `../A0-exact-candidate-record.md`.
- Row 09 is the final post-acceptance proof run (A0 ACCEPTED, A1 READY). Its
  raw-output digest EQUALS row 06's — not a copy-paste error: the validator's
  success output is a single deterministic OK line naming the state path, block
  count, DAG path, and mode, all identical across the two runs; the differing
  ledger CONTENT is attested by the ledger file itself, not by this output line.
- The historical round-3 bypass re-tests (bare-stack_id stacked exception; ACCEPTED
  with PENDING mandates; ACCEPTANCE_RECOMMENDED with a BLOCKING mandate — ONE
  violation, see the corrected count in `../A0-exact-candidate-record.md`; the
  `""#REQUIRED_..."` quote-comment parse bypass) and the `program_dag_digest`
  64-`a` substitution probe remain rejecting.
