# Rev11 legacy-architecture reconciliation and successor charter pack

## What this package contains

This is the implementation-grade amendment draft that was missing from the earlier DAG-only proposal.

It contains:

- **3 revised successor DAG modules** with **27 static nodes**;
- **27 detailed static charters**;
- **30 generated Native Checker feature-slice charters**;
- **1 generated Native Checker required-family convergence charter (`NCKF0`)**;
- a machine-readable Native Checker family manifest;
- existing-node amendment requirements;
- legacy `docs/arch` source reconciliation and disposition seed;
- end-state architecture, dependency rationale, charter quality gate, and generated charter index;
- external authorization and security review-profile additions for executable supply-chain work.

Total DAG charters: **58**.

## Revised topology

The earlier proposal had three oversized boundaries. This pack splits them:

### Native Checker

```text
NCK0 constitution
  -> NCK1 executable regions/contributions
  -> NCK2 query/results
  -> NCK3 shared-proof rule kernel
  -> NCK4 manifest/oracle/generator
  -> NCK5 framework ingress
  -> NCK6 authority arbitration/publication
  -> NCK7 shared consumer service
  -> NCK8 terminal

NCK4 + NCK6 -> 30 generated NCF feature slices -> NCKF0 -> NCK8
```

NCK6 no longer mixes authority arbitration with every CLI/LSP/public adapter. NCK7 owns consumer integration; NCK8 is proof/deletion/promotion only. `NCKF0` replaces an external “all required slices complete” assertion with a generated DAG convergence node.

### Language Service

```text
LSO0 constitution
  -> LSO1 recovery
  -> LSO2 target graph
     -> LSO3 navigation
     -> LSO4 occurrences/hierarchy
        -> LSO5 semantic rename planning
     -> LSO6 completion/resolve intents
     -> LSO7 presentation
  -> LSO8 authored edit transaction
  -> LSO9 generated conformance
  -> LSO10 terminal
```

References/hierarchy, semantic rename policy, and final edit application are independent blocks. This prevents “references + string replacement + WorkspaceEdit” from masquerading as one rename implementation.

### Engine Provisioning

```text
EPR0 policy
  -> EPR1 artifact/trust/install contract
     -> EPR2 optional managed acquisition
     -> EPR3 optional bundled shipping
  -> EPR4 authorized candidate resolution/selection
  -> EPR5 activation/handshake/health/ProviderEpoch
  -> EPR6 terminal
```

Acquisition, shipping, validation, selection, and activation remain separate security authorities. EPR2/EPR3 are optional and require explicit maintainer authorization. A no-download/no-bundle product remains valid.

## Primary files

### Proposed canonical DAG modules

- [`authority/dag/expansion-native-checker.toml`](authority/dag/expansion-native-checker.toml)
- [`authority/dag/expansion-native-checker-families.example.toml`](authority/dag/expansion-native-checker-families.example.toml)
- [`authority/dag/expansion-language-service.toml`](authority/dag/expansion-language-service.toml)
- [`authority/dag/expansion-engine-provisioning.toml`](authority/dag/expansion-engine-provisioning.toml)
- [`authority/root-module-registration.example.toml`](authority/root-module-registration.example.toml)

### Charters

- [`charters/expansion-native-checker/`](charters/expansion-native-checker/)
- [`charters/expansion-language-service/`](charters/expansion-language-service/)
- [`charters/expansion-engine-provisioning/`](charters/expansion-engine-provisioning/)
- [`generated/CHARTER-INDEX.md`](generated/CHARTER-INDEX.md)
- [`generated/REV11-SUCCESSOR-STATIC-CHARTERS.md`](generated/REV11-SUCCESSOR-STATIC-CHARTERS.md) — all 27 static charters in one review file
- [`generated/NATIVE-CHECKER-FAMILY-CHARTERS.md`](generated/NATIVE-CHECKER-FAMILY-CHARTERS.md) — all 30 generated feature slices plus NCKF0 in one review file

Representative detailed charters:

- [`NCK0`](charters/expansion-native-checker/NCK0.md) — diagnostic authority constitution
- [`NCK2`](charters/expansion-native-checker/NCK2.md) — query/result/cache architecture
- [`NCK6`](charters/expansion-native-checker/NCK6.md) — exact family authority arbitration
- [`NCK7`](charters/expansion-native-checker/NCK7.md) — shared consumer service
- [`NCF-CO-OVER`](charters/expansion-native-checker/generated-families/NCF-CO-OVER.md) — generated overload slice
- [`NCKF0`](charters/expansion-native-checker/generated-families/NCKF0.md) — generated required-family convergence
- [`LSO2`](charters/expansion-language-service/LSO2.md) — canonical target/provenance graph
- [`LSO5`](charters/expansion-language-service/LSO5.md) — semantic rename planning
- [`LSO8`](charters/expansion-language-service/LSO8.md) — sole authored edit transaction engine
- [`EPR2`](charters/expansion-engine-provisioning/EPR2.md) — optional managed acquisition
- [`EPR4`](charters/expansion-engine-provisioning/EPR4.md) — deterministic candidate resolution
- [`EPR5`](charters/expansion-engine-provisioning/EPR5.md) — atomic activation/ProviderEpoch

### Architecture and migration

- [`architecture/END-STATE.md`](architecture/END-STATE.md)
- [`architecture/DEPENDENCY-RATIONALE.md`](architecture/DEPENDENCY-RATIONALE.md)
- [`architecture/CHARTER-QUALITY-GATE.md`](architecture/CHARTER-QUALITY-GATE.md)
- [`amendments/existing-node-amendments.md`](amendments/existing-node-amendments.md)
- [`sources/legacy-arch-reconciliation.md`](sources/legacy-arch-reconciliation.md)
- [`catalogs/legacy-arch-disposition.example.toml`](catalogs/legacy-arch-disposition.example.toml)
- [`catalogs/native-checker-family-manifest.toml`](catalogs/native-checker-family-manifest.toml)

### Security and external authorization

- [`catalogs/external-requirements.additions.toml`](catalogs/external-requirements.additions.toml)
- [`catalogs/review-profile.security-3.example.toml`](catalogs/review-profile.security-3.example.toml)

## Charter design

Every static charter includes:

- independently acceptable outcome;
- current/final owner;
- expected production surfaces and concrete named APIs;
- exact predecessor contracts;
- binding architecture and identity/invalidation/publication laws;
- 6–7 internal subblocks with architecture, expected changes, and discriminating proof;
- migration/cutover and exact deletions;
- forbidden designs;
- acceptance IDs, performance evidence, rescope/abort conditions, verification, consumers, and source reconciliation.

Generated checker slice charters include the exact slice scope/facts/oracle examples, six implementation/certification/promotion subblocks, one-engine constraints, incremental/cache-admission evidence, and bounded rescope rules.

## Important amendment caveats

This package is a high-detail proposal, not a substitute for the canonical Rev11 amendment workflow. Before admission, the live authority generator must:

1. pin the current commit/tree rather than trusting the proposal pins;
2. generate exact source line atoms and SHA-256 text digests;
3. verify every predecessor ID and current accepted receipt;
4. register module files, charters, external requirements, conflict-domain leases, and generated indexes;
5. reconcile whether a `security-3` equivalent already exists;
6. generate the complete legacy disposition catalog from the current Git tree;
7. validate cycles, reachability, optional/conditional predecessors, product/release gates, charter metadata equality, and source coverage;
8. run RED/GREEN negative controls, exact gates, and configured independent reviews on the landing-frozen candidate.

The source blob SHAs in the disposition seed are branch observations used for reconciliation; the canonical deletion amendment must recompute them immediately before deletion.

## Recommended admission sequence

1. Admit source reconciliation, existing-node amendments, the three static DAG modules, family manifest schema, and charter quality guards.
2. Generate/validate the complete legacy path disposition catalog.
3. Delete/relocate the legacy documents in the same accepted authority amendment.
4. Execute successor nodes according to the graph; do not wait for implementation terminals to delete superseded planning prose once its authority has been transferred.
