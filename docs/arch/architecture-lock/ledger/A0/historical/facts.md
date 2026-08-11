# A0 — Verified Entry Facts (Revision 11) (HISTORICAL)

> **HISTORICAL.** This is the pre-candidate entry-facts capture, written BEFORE any
> candidate existed and BEFORE the maintainer rulings (R-1…R-8) were made. Statements
> such as "NOT YET DESIGNATED" (maintainer) and "the state file is UNVALIDATED"
> describe that moment only and are NOT current. The current exact-candidate record is
> `../A0-exact-candidate-record.md`; live state is `../../program-state.toml`.

**Artifact class:** A0 evidence record (non-mutating inspection).
**Block:** A0 — Adopt Revision 11 and freeze exact checkout. **Status:** IN PROGRESS — NOT accepted.
**Acceptance authority:** maintainer only (`governance.md` §1.1, §15). This artifact confers no acceptance.
**Repository under inspection:** `<REPO>` — left byte-clean; no mutation performed.
**Evidence root:** `<EVIDENCE>\` (external to the repository).
**Recorded by:** Claude Opus 5 (`claude-opus-5`) main session, Claude Code 2.1.222.

The following facts were independently collected and are recorded verbatim.

---

- repo root: <REPO> ; remote origin (fetch+push) https://github.com/pikax/verter ; branch `main` ; upstream origin/main ; ahead 0 / behind 0
- entry_checkout_sha = 9af553dd262f82ac2f66e4ebf0a0faa70bc7aec0 ; short 9af553dd2 ; tree = 3cf111cf5665586b7d8fdfd520f01cfee3bf8108
- working tree CLEAN (`git status --porcelain` empty); untracked_count = 0; submodules: none; worktrees: 1 (the primary checkout only)
- HEAD commit: "feat(typeinfo): support type narrowing (#94)", author Carlos Rodrigues <carlos@hypermob.co.uk>, signature status `E` (git %G? = E — signature could not be verified / no valid key), commit.gpgsign=false, tag.gpgsign unset, user.signingkey unset, gpg 2.4.9 present. => commit signing NOT in use.
- 53 git stashes present; 571 of 596 local branches unmerged into main; 34 remote branches unmerged into origin/main
- toolchain: rustc 1.97.1 (8bab26f4f 2026-07-14); cargo 1.97.1 (c980f4866 2026-06-30); cargo-nextest 0.9.137; node v26.5.0; pnpm 10.22.0; host x86_64-pc-windows-msvc; Windows 10.0.26200.8875; rustup active toolchain 1.97.1-x86_64-pc-windows-msvc overridden by repo rust-toolchain.toml
- Cargo.lock sha256 = b4fb9825718c60ca8439744953a82958380dfdb18daabc2ed686e4918e838b27
- pnpm-lock.yaml sha256 = 3f789a2ade9617b68dc75b2734b36ab331c5aa0518f44e0d04a33dec7cda1cfb
- orchestrator runtime: Claude Code 2.1.222 ; requested model claude-opus-5 ; actual model claude-opus-5 ; provider Anthropic ; fallback/substitution: NONE
- GitHub: gh CLI 2.93.0, authenticated as `pikax`, token scopes gist/read:org/repo/workflow; repo pikax/verter PUBLIC; permissions admin+maintain+push+triage+pull = true; default branch main; squash+merge+rebase all enabled; delete_branch_on_merge=true
- Branch protection on main: NONE (`/branches/main/protection` → 404). One repository ruleset "Copilot review for default branch" (active, ~DEFAULT_BRANCH) with rules: deletion, non_fast_forward, copilot_code_review. NO merge queue configured. NO required status checks.
- Stack tooling: gt / git-town / spr / ghstack / jj all ABSENT locally. GitHub native stacks not configured. Ordinary dependent PRs available. Signed rebases: not configured (no signing).
- Open PRs: exactly ONE — #98 DRAFT, main <- agent/rsvelte-runtime-engine, mergeStateStatus DIRTY, last updated 2026-07-30, "feat(svelte): delegate runtime compilation to rsvelte". Architecture-affecting: touches the framework/compiler boundary. Disposition: PENDING MAINTAINER DECISION.
- Open issues: 14 (incl. #97 undeclared-props, #96 LSP debounce delay, #95 rename, #93 LAPCE hang, #92 TS7 mappers).
- CI at the entry SHA (run 31325653744, workflow "CI", head sha 9af553dd2): FAILURE — 53 failed / 20 success jobs. Failing: Rust Clippy + Rust Build Configurations (6 clippy `-D warnings` lints in verter_session, NEW at 9af553dd2), Rust Test (2 failed + 2 timed out of 24089), and the whole VS Code E2E matrix (independent per-cell assertion failures, not a cascade). main has been persistently red since 2026-03-18 (316d6598f); last green CI on main was 2026-03-16 (94415737e).
- Python 3 is NOT installed (WindowsApps stub only). The Revision 11 ZIP `verter-architecture-v11.zip` and `verter-architecture-v11.sha256` are ABSENT from the machine, so `tools/validate_package.py`, `tools/selftest_orchestration.py`, `tools/validate_program_state.py`, `tools/validate_stack_window.py`, and `tools/validate_landing_equivalence.py` COULD NOT BE RUN. Package validation state = UNVERIFIED/BLOCKED.
- Verified artifact digests present locally (match the published Revision 11 validation report exactly): verter-architecture-lock-master-plan-v11.md = 3303834589df23cd04338801374857e685d9961df3d323c60c4b58db54ce62ce ; verter-opus-orchestrator-prompt-v11.md = d32b3f748230b3735469195ed62e6728242774ea0a575af1999b724164a750c3. Claimed canonical package digest af11392f5f9eeea75cbd82def85adadfee41b3c8032b5248c09e96aba13123a7 (85 files) is UNVERIFIED — no ZIP/manifest available.
- Designated maintainer: NOT YET DESIGNATED (pending explicit maintainer designation). Orchestrator identity: Claude Opus 5 (claude-opus-5) main session, Claude Code 2.1.222.

---

## Consequences recorded in `program-state.toml`

- `authority_package_digest`, `release_report_digest`, `program_dag_digest`, `maintainer` = `UNRESOLVED_BLOCKED_A0`
  (package validation could not be executed; no maintainer designated).
- `program-state.toml` `status = "ACTIVE"`, `current_block = "A0"`, block A0 `status = "IN_PROGRESS"`;
  every other block (A1 … L4, 49 rows) remains `LOCKED`. No block after A0 is exposed as legal work.
- All three A0 review mandates and `maintainer_decision` remain `PENDING`.
- `tools/validate_program_state.py --mode live` has NOT been run against this state file
  (the tool is unavailable). The state file is therefore UNVALIDATED by the package validator.
