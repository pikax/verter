# Integration contract

Each block may be built in an isolated branch/worktree. The main orchestrator owns the integration branch. Integrate/rebase first, freeze candidate, run the gate, obtain the required independent reviews, then land byte-identically. Candidate and integration identities are distinct fields. Any conflict resolution or content change invalidates review/gate evidence. Resource leases serialize only overlapping conflict domains and the landing window; they never add DAG edges.
