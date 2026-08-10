# ADR-018 — Agent Orchestration Uses One Model-Independent Program Ledger and a Pinned Opus Adapter

**Status:** Accepted  
**Decision owner:** autonomous or assisted program execution.  
**Reopen only if:** an alternative preserves exact state, role independence, resumability, and evidence discipline with less machinery.

## Context

The governance defines roles, but a long-running agent still needs a deterministic entry sequence, actual-runtime identity, durable block state, and bounded context packets. A model-specific prompt must not become architecture authority, and two competing execution ledgers would be worse than conversational memory.

## Decision

- `contracts/agent-orchestration.md` is the model-independent execution contract.
- `program-state.toml`, validated by `tools/validate_program_state.py`, is the sole durable program ledger.
- `ORCHESTRATOR.md` is the sole normative entry point and authorizes `A0` only.
- `OPUS-START-HERE.md` and `agents/opus-bootstrap.md` are convenience adapters for fixed model ID `claude-opus-5`; they record the actual runtime/provider and any fallback.
- Every worker receives a digest-addressed bounded context packet and one writable worktree/branch.
- Optional subagents are used only for substantial independent work or required review independence and cannot self-accept.
- Push, merge, destructive-history, secret, and repository-policy permissions are not granted by the bootstrap prompt.

## Consequences

The package can be handed directly to an Opus orchestrator without asking it to invent sequencing, state, permissions, or review independence. Governance remains portable to another model or a human orchestrator because the Opus adapter is non-normative.

## Rejected alternatives

- paste only the master plan and rely on conversational memory;
- a separate JSON orchestrator ledger beside the program ledger;
- floating model identity without actual-runtime recording;
- one agent context scopes, implements, and solely approves;
- model-specific prompt text as durable product architecture.
