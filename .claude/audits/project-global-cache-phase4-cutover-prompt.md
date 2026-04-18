# Single-agent kickoff prompt — Phase 4/5 cutover

Copy the block below into a fresh Claude Code session on the
`refactor/semantic-db-overhaul` branch at repo
`D:\dev\personal\verter`. The agent is expected to execute the full
cutover; it may take many commits but must land every slice and end
with one authoritative project-global cache path, zero request-view
hot path, and `host_request_view.rs` deleted.

---

```
Continue the project-global cache overhaul on branch
`refactor/semantic-db-overhaul` (repo: D:\dev\personal\verter).

READ FIRST, in this order:
1. .claude/audits/project-global-cache-phase4-cutover-handoff.md
   — the authoritative execution playbook. Follow its §3 slice
   sequence and §5 verification gates literally.
2. .claude/audits/project-global-type-rewrite-correctness-audit.md
3. C:\Users\david\.claude\plans\component-meta-project-global-cache-overhaul.md
4. D:\dev\personal\verter\CLAUDE.md
5. Skills: /type-resolution, /component-meta, /host-session
6. crates/verter_session/src/host_request_view.rs (the module being deleted)

STATE AT HANDOFF (commit ab48b7e1, branch ahead of origin by 21):
- Phases 0 → 3 landed
- Phase 4 memo retirement slice landed (external_inputs_memo,
  eval_state_memo, EvalStateMemoEntry retired)
- Workspace clippy baseline clean (-D warnings passes on --lib --tests)
- `phase4_in_view_surface_ratchet` test in project_global_cache_tests.rs
  locks the current ceilings; lower the ceilings in every commit that
  lowers the counts, or delete the whole test when the cut completes.
- 9734 workspace tests pass.

YOUR MISSION: land the full signature cut and delete the request-view
architecture. By the end of your run:
- crates/verter_session/src/host_request_view.rs must not exist
- `grep -rn 'RequestStoreView\|CURRENT_REQUEST_VIEW\|_in_view' crates/verter_session/src`
  returns zero hits outside tests that assert absence
- `grep -rn 'ModuleFacts\b' crates/verter_session/src | grep -v module_facts_db.rs`
  returns zero hits; module_facts_db.rs deleted
- ProjectSemanticDispatch wires every SemanticQueryKey variant, not just
  ResolveDecl
- Every ValidatedFactCache write publishes a dep-signature fragment
- Old-vs-new corpus audit (plan §J) runs before the final deletion commit
- CLAUDE.md + 3 skill docs reflect the final architecture
- .claude/audits/project-global-type-rewrite-correctness-audit.md marked
  COMPLETE
- .claude/audits/project-global-cache-phase4-cutover-handoff.md and
  .claude/audits/project-global-cache-phase4-cutover-prompt.md deleted
  in the final slice

EXECUTION RULES:
- Work through §3 slices 2 → 7 in that order (bottom-up mechanical cut).
  Then slice 1 (meta.rs), then 8 (tests), then 9–16.
- Each phase-final commit must pass the §5 gates:
  `cargo test --workspace --tests --verbose` and
  `cargo clippy --workspace --lib --tests -- -D warnings`
- Intermediate commits inside a large slice may temporarily break tests,
  but every phase-final commit is green.
- Conventional commits (`refactor(session): ...`).
- Delete legacy paths in the same change that replaces them. No feature
  flags, dormant helpers, fallback branches, shims, or TODO-for-later.
- Do NOT skip hooks (--no-verify). Fix the underlying issue.
- Never add Co-Authored-By lines to commits.
- Never run `git push`.

WHEN COMPLETE, produce a final summary that lists:
- commits made
- tests run (tight + broad gates + integration + corpus)
- intentional behavioral deltas (if any) observed in the corpus diff
- updated docs

If you run out of runway, stop at the nearest green commit, update the
ratchet ceilings to match current counts, and update the handoff file
with progress + what's left. Do NOT leave the tree in a red state.

Start by reading the handoff playbook at
.claude/audits/project-global-cache-phase4-cutover-handoff.md
— it has the exact migration pattern, the view-probe → live-host-probe
table, the per-slice ordering, the test-disposition list, and the
sharp-edges inventory you will need.
```
