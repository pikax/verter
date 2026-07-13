# Component-meta performance work — where the authority actually lives

This memory used to route every agent to a machine-local plan file
(`D:\tmp\verter-component-meta-performance-plan.md`) and called it "the source for current
orchestration state", to be inspected before changing component-meta / performance /
architecture-guard code.

**That file no longer exists** — not on another machine, and not even on the one that wrote the
note. An agent following this memory today would read nothing, or improvise.

That is the entire lesson, and it is why memory is not an authority: a remembered path (or
command, or model slug, or branch name) outlives the thing it points at, is copied forward into
briefs, and is followed by agents that never saw it reviewed. Memory may LINK to an authority;
it may not BE one. See `/mom-cto-orchestration` → Memory Is Not Authority.

## What to read instead

- An IN-FLIGHT plan is session state. It comes from the brief and from git — never from memory.
- Durable component-meta / performance architecture is in-tree:
  - `CLAUDE.md` → Component-Meta Shallow-By-Default Rule, Cache Architecture, Build Philosophy.
  - Skills: `/component-meta`, `/type-resolution`, `/type-cache-architecture`, `/rust-performance`.
  - `docs/arch/` for binding designs, plans, and the debt ledgers.

Before component-meta / performance / architecture-guard changes, load those, and take the
current branch and plan state from git and the brief.
