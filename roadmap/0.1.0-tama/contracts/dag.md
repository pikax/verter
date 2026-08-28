# Unified DAG contract

Root metadata, every file under `authority/dag/`, authoritative charters, and catalogs define the static work graph. `authority/state/implemented.toml` alone records implementation state. A dispatchable node is READY when every transitive DAG ancestor has an implementation-ledger row. Conflict, resource, and external-requirement fields are planning guidance, not runtime locks or machine-validated authorizations.
