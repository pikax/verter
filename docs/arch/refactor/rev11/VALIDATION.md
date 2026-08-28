# Rev11 validation

The live docs gate covers the promoted single authority root, backup exclusion and byte retention, pinned pre-cutover source objects, generated freshness, strict authority validation, the canonical trusted-local CLI lifecycle, independent cross-runtime admission, exact transaction recovery, immutable review-worktree refusal, automatic effort policy, current-cycle-only escalation, anchor-loss reinitialization, and audit-only preactivation history.

Run:

```text
node docs/arch/refactor/rev11/tools/run-docs-gate.mjs
```

The gate discovers only live `tools/*.mjs`, runs every builder in freshness mode, validates the 197-node authority and 197 materialized charters, runs the canonical negative controls, and executes every live Node test. `backup/` is not a discovery root. A PASS requires zero unexpected skips.

No heavy Rust gate is part of this documentation authority cutover. Candidate nodes still owe their declared Rust, TypeScript, product, performance, and confirmation evidence.
