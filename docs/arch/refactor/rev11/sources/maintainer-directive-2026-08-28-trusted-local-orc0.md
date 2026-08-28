# Maintainer directive — trusted-local ORC0 activation

Date: 2026-08-28

This directive supersedes the preactivation operating model in the 2026-08-27 ORC0 directive without altering or deleting that historical source. The user/maintainer controls landing; agents execute the repository guide.

ORC0 assurance is **operator-attested local consistency and auditability**. The canonical tool coordinates all registered external runtime roots for one repository under one repo-global lifecycle lock and mutable local anchor. It validates inputs before publication, recomputes under the lock, journals exact intended bytes, and recovers an exact committed transaction. It does not claim cryptographic harness authenticity, independent anti-rollback, malicious-filesystem-owner resistance, or resistance to intentional toolchain replacement. Loss of the local anchor blocks normal mutation until an explicit trusted-operator reinitialization creates a visible new lineage with `continuity = "unknown/lost"`.

The architecture consult is the sole CLI model role: OpenAI Codex, model `gpt-5.6-sol`, effort `xhigh`, neutral and read-only. Implementation, fix, review, verification, and confirmation run as fresh harness tasks. Orchestration is provider-neutral. Each node states low/medium/high minima and defaults; admission deterministically escalates from the node's existing kind, risk, public, concurrency, semantic-authority, and release signals. An optional override may raise but never lower a computed tier. Admission emits ready role briefs and binds the exact policy, task, provider, model, effort, prompt, report, lease, round, node, and candidate identities downstream.

Landing requires the current round only and a clean three-of-three set of distinct review tasks, plus fresh verification and confirmation. `FIX_REQUIRED`, `ABORTED`, and `RELEASED` explicitly close a round; closed or superseded rounds are audit-only and can never regain acceptance. Two review/fix cycles are the soft maximum. A P0/P1 remaining after round two requires the neutral architecture consult to state whether work continues and set an explicit additional-round cap. P2/P3 follow the owning disposition policy.

The preactivation ORC0 R1 lease/finalization history associated with candidate `c3acb57730e13dbb6f2abc8de4c65856ae383956` remains append-only rejected/audit-only history. Historical r3/r4/r6 prompts, manifests, and reports remain exact-byte audit sources and cannot satisfy current acceptance. No file from that history is deleted or promoted by this directive.

J1 closes only as `LANDED_GRANDFATHERED`. The package lifecycle remains `DORMANT -> ORC0 -> ACTIVE`, and runtime evidence remains outside the committed authority package. Disposable agent/train worktrees are removed immediately after their result is safely committed or recorded and no longer needed; completion reports state cleanup status. Reports must be honest and terse.
