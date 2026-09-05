# Authority changes

Edit the static DAG, charter, catalog, or ledger through an ordinary reviewed patch. Regenerate projections and run the docs gate. There is no amendment digest chain, authority lock, activation transition, or external ratification receipt.

Periodic Architect conformance and final train review use the current amended authority. They include every ordinary reviewed amendment effective for the train without introducing a separate amendment manifest or digest.

## Cross-train contract changes

A reviewed change that adds or removes a cross-train contract consumer updates `catalogs/contract-dependencies.toml` and its DAG path together. The catalog validates static producer ancestry only. Consult `contracts/successor-seams.md` for read-only reconciliation of known current/successor owners. Correct pending charter obligations through an ordinary amendment; preserve implemented charters as historical acceptance records.
