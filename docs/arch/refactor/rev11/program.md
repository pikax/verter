# Unified Rev11 program

The program merges current Rev11, the compiler architecture, and the successor expansion into one logical DAG while preserving independent product terminals. BR0 is the source-canonical sole successor entry: its two externally custodied maintainer decisions bind the accepted Rev11/L4 state before any successor work can dispatch. Compiler, formatter, lint, CLI, HTML/WC, Custom Elements, architecture proofs, and benchmark work can then proceed in isolated worktrees whenever their real predecessors and conflict leases permit.

The graph deliberately has no all-products join. L4 closes Rev11; BR0 promotes the successor product; each other product owns its own terminal. Exact sizing, review, integration, receipt, lease, amendment and orchestration behavior is centralized in `contracts/`, keeping packets and charters compact.
