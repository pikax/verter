# Start Verter Revision 11 with Claude Opus 5

Revision 11 is safe to hand to an Opus orchestrator **together with an actual Verter checkout and command access**. The architecture ZIP alone cannot inspect, modify, test, or review the repository.

**Normative entry:** `ORCHESTRATOR.md`. This guide, the bootstrap, and the role files are adapters; they cannot override the package contracts.

# 1. Verify the release

From the directory containing the release artifacts:

GNU/Linux:

```bash
sha256sum -c verter-architecture-v11.sha256
```

macOS:

```bash
shasum -a 256 -c verter-architecture-v11.sha256
```

Then on either platform:

```bash
unzip -q verter-architecture-v11.zip
python3 verter-architecture-v11/tools/validate_package.py verter-architecture-v11
python3 verter-architecture-v11/tools/selftest_orchestration.py
```

Do not continue on any checksum, manifest, live-self-test, validation, or extraction mismatch.

# 2. Launch the orchestrator from the Verter checkout

First verify the Claude Code runtime:

```bash
claude --version
```

The `claude-opus-5` model ID requires Claude Code **2.1.219 or later**. Upgrade before continuing if the installed runtime is older, and record the exact runtime version in A0 evidence.

Place the extracted package beside the repository, then launch Claude Code from the repository root:

```bash
cd /path/to/verter
claude --model claude-opus-5 --add-dir ../verter-architecture-v11
```

The adapter requests the fixed model ID `claude-opus-5`, not the floating `opus` alias. At startup, record the actual model/provider shown by Claude Code. A fallback or substitution is reported and causes the Opus-specific handoff to stop unless the designated maintainer explicitly accepts that runtime.

# 3. Paste the bootstrap prompt

Paste the complete contents of either checksum-verified copy:

```text
../verter-opus-orchestrator-prompt-v11.md
../verter-architecture-v11/agents/opus-bootstrap.md
```

They are byte-identical in a valid release.

The first run executes **A0 only**. It does not begin broad implementation, choose post-result performance gates, create a program-wide PR stack, or claim that the unimplemented architecture has already passed its final proof.

# 4. Optional role adapters

Optional Claude Code subagent definitions live under `agents/claude-code/`. Review them before copying them into the repository's `.claude/agents/` directory. They are convenience adapters only. `governance.md`, `contracts/agent-orchestration.md`, and each block's immutable context packet remain authoritative.

Do not treat several identically prompted model instances as automatically independent. Foundational review requires distinct mandates, clean contexts, direct evidence, and exact-candidate binding; a different model or human reviewer is valuable where available.

# 5. First-run success condition

A successful first run returns an A0 evidence package with:

- validated Revision 11 package digest and release provenance;
- requested and actual model, provider, and orchestrator runtime/version identity;
- exact repository entry checkout SHA/tree and dirty/worktree/submodule state;
- architecture-affecting open-change disposition;
- GitHub, CI, merge-queue, stack-tool, and permission facts;
- initialized and validated `program-state.toml`;
- no unauthorized post-A0 implementation.

Only the designated maintainer can accept A0 and authorize the next legal block.
