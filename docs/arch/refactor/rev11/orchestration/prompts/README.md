# Runtime prompts

One per role. **These are the only files injected into an agent.** They carry role boundary, inputs,
actions, output contract and stop conditions. Reasons live in [../design-notes.md](../design-notes.md)
and are never injected.

Fill every `{{FIELD}}`, then check:

    ! grep -nE '\{\{[A-Z0-9_]+\}\}' <the filled prompt>

Prints any unfilled field and exits 1; silent and exits 0 when ready. Send everything below the `---`
verbatim. Reference documents by path — do not paste them in.

| Prompt | Seat | Access |
|---|---|---|
| [program-orchestrator.md](program-orchestrator.md) | team lead | no code |
| [block-orchestrator.md](block-orchestrator.md) | named teammate | no code |
| [manager.md](manager.md) | subagent, one per block | no code |
| [implementer.md](implementer.md) | nested subagent | write, one worktree |
| [reviewer.md](reviewer.md) | nested subagent or external tool | read-only |
| [architect.md](architect.md) | Codex | read-only |
| [test-adversary.md](test-adversary.md) | optional, high-risk tests | write, own worktree |
