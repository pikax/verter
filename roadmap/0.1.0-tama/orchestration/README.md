# Orchestration commands

There is no external runtime or mutable orchestration database. `programctl frontier` derives readiness solely from transitive ancestor rows in `authority/state/implemented.toml`. `programctl packet ID` combines that status with the node charter and ledger instructions.

Agents and maintainers coordinate branches, reviews, resources, external requirements, and landing. The CLI does not lease work or verify commits.
