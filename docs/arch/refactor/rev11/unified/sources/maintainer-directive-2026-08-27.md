# Maintainer directive — unified DAG activation after the J1 landing milestone

- Provenance: exact user direction in the active Codex task/thread.
- Recorded: 2026-08-27 (Europe/Lisbon).
- Authority class: maintainer direction for the unified-v2 activation transition only.
- Historical limit: this directive does not change the live legacy ledger's `J1 = IN_PROGRESS` row and does not call the J1 landing milestone legacy or v2 acceptance.

## Exact directive

> finish/land unified DAG, then start the first block that unlocks initial trains, then prepare two prompts for separate compiler and D-train branches.

## Operative interpretation

The unified DAG may import the exact J1 fast-forward landing milestone at commit `6a6c3c1a83709f7a58918e5b4e3d1eedcbd3ddac`, tree `f227feb7f302b26ce896ccb9b41f9e308937ae64`, parent `f12c2c287a92fd713d83dadb1ff635601218a7cf` as `LANDED_GRANDFATHERED`. That distinct state satisfies the J1 side of ORC0 activation without asserting that J1's legacy acceptance surface is closed. The directive authorizes the exact candidate-bound `maintainer_unified_v2_activation` slot for ORC0; it grants no BR0, successor-genesis, TCM0 rescope, finding-disposition, or general amendment authority.

After the unified DAG lands, orchestration may dispatch the first READY block that unlocks the initial trains. Compiler and D-train execution remain separate branch/worktree prompts and must still satisfy the unified DAG's exact predecessor, lease, gate, review, finalization, and acceptance controls.
