# Validation

Tama validation checks only the static plan:

```text
node roadmap/0.1.0-tama/tools/validate-program-dag.mjs --strict
node --test roadmap/0.1.0-tama/tools/implementation-ledger.test.mjs
```

The first command cheaply checks DAG structure, charter/header parity, catalog references, ledger node membership, and schemas. The focused tests exercise the small readiness and ledger model.

Pull requests that change Tama-managed paths run the same two commands in the `Tama Roadmap` CI job. This is deliberately a cheap shape and scope check for agents: it rejects unknown fields, malformed rows, broken DAG references, charter drift, and catalog drift without introducing Git-identity validation.

Neither command resolves ledger commit hints or validates them against Git or GitHub. No commit SHA, tree, ancestry, receipt, lease, runtime journal, content digest, or external authorization participates in readiness.
