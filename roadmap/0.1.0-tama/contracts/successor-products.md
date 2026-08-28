# Rev11 successor end-state architecture

## Executive decision

The legacy `docs/arch` capability residue should not be represented by copied documents or one monolithic “post-Rev11” train. It resolves into three independently owned successor products:

1. **Native Checker** — evaluates diagnostics over the one semantic resolver and cuts diagnostic families from external ownership to certified native ownership.
2. **Language Service Operations** — provides one authored-coordinate target, occurrence, presentation, and edit-transaction architecture for editor and tooling operations.
3. **Engine Provisioning** — makes acquisition, validation, selection, activation, and capability publication explicit, policy-controlled, and supply-chain safe.

These products share Rev11 identities, semantic facts, mapping, provider lifecycle, public outcomes, profiles, manifests, and performance evidence. They do not share mutation authority.

## Full dependency shape

```text
                                   Rev11 L4
                                      |
                                      v
                                     BR0
                                      |
                                      v
                                  UAK0 -> UAK1
                                      |
            +-------------------------+-------------------------+
            |                         |                         |
            v                         v                         v
     Native Checker            Language Service          Engine Provisioning
      NCK0 ... NCK8             LSO0 ... LSO10             EPR0 ... EPR6
            |                         |                         |
            +----------- typed public/capability seams --------+
```

The three trains may run concurrently after their exact predecessors exist. Their convergence is product-scoped; no successor terminal waits on unrelated compiler, formatter, lint, or framework proof products unless a concrete capability requires them.

## 1. Native Checker

### Sole semantic architecture

```text
Authored source / framework contributions
                 |
                 v
     canonical semantic facts and proofs
  (symbols, types, Relate, ResolveCall, flow,
   contextual typing, modules, project facts)
                 |
                 v
          NCK3 DiagnosticRule kernel
                 |
                 v
   NCK2 DiagnosticBatch / proof & fix refs
                 |
        +--------+--------+
        |                 |
        v                 v
 NCK4 certification   NCK6 authority state
  oracle + overlay     External/ObserveNative/
  generated slices    CertifiedNative/Disabled
        |                 |
        +--------+--------+
                 v
       NCK7 shared DiagnosticService
                 |
         thin consumer adapters
```

The checker is **not** another type system. It does not own symbol lookup, type construction, relation, overload selection, control-flow analysis, module resolution, or project membership. A diagnostic rule consumes existing typed facts and proof references. Where a required fact is missing, the work belongs at the existing semantic owner; a checker-private fallback is forbidden.

### Query and result model

The core operations are scoped:

```rust
CheckRegion(ExecutableRegionId)
CheckFile(SourceUnitId)
CheckProjectRule(ProjectRuleKey)
CheckExpression(ExpressionSubject) // demand-only, not a whole-file prerequisite
```

`CheckProgram` is a coordinator/stream over scoped results, not a monolithic cache key.

```rust
struct DiagnosticBatch {
    basis: DiagnosticBasis,
    completeness: DiagnosticCompleteness,
    authority_snapshot: DiagnosticAuthoritySnapshotId,
    diagnostics: Arc<[AuthoredDiagnostic]>,
}

struct AuthoredDiagnostic {
    id: DiagnosticId,
    origin: DiagnosticOrigin,
    family: DiagnosticFamilyId,
    feature_slice: DiagnosticFeatureSliceId,
    rule: DiagnosticRuleId,
    subject: SemanticSubjectId,
    primary: AuthoredAnchor,
    related: Arc<[AuthoredRelatedLocation]>,
    proof: Option<DiagnosticProofRef>,
    fixes: Arc<[DiagnosticFixIntentRef]>,
}
```

A result distinguishes empty complete success from `NeedInputs`, unsupported, cancelled, stale, superseded, budget-exceeded, or partial. Only complete results admit to warm caches.

### Certification and cutover

Every semantic slice is a generated `NCF-*` node. Each node owns one bounded feature slice, its rules, exact fact reads, hermetic fixtures, pinned external oracle, optional reviewed correction overlay, incremental/admission proof, performance evidence, and NCK6 promotion.

The external engine is:

- the oracle and fallback owner for uncertified slices;
- a non-publishing comparison source in `ObserveNative`;
- never invoked by a native checker query.

The runtime resolver has one correctness behavior. TypeScript bug compatibility is recorded as review-gated test data, not a runtime mode or cache-key dimension.

`NCKF0` is generated from all required family rows and provides an internal DAG convergence node. NCK8 therefore does not rely on an external assertion that “all families are done.”

## 2. Language Service Operations

### One authored-coordinate operation substrate

```text
Authored position / semantic subject
                 |
                 v
       LSO2 canonical TargetGraph
       /         |          \
      v          v           v
 navigation   occurrences   presentation
 LSO3         LSO4          LSO7
                 |
                 v
          LSO5 RenamePlan
                 |
 completion ---> LSO8 AuthoredEditTransaction <--- fixes/actions
   LSO6
```

Core language-service APIs do not expose LSP positions, generated TSX paths, provider JSON, or raw workspace edits. Those belong to edge adapters.

### Target and provenance law

A target is identified by semantic declaration/symbol identity plus exact source/profile ownership. URI and range are renderings, not identity.

```rust
enum TargetProvenance {
    LiveSemantic { source_revision: SourceRevision },
    HostSource { source_hash: ContentHash },
    GeneratedMapping { compile_snapshot: CompileSnapshotId },
    ExternalDeclaration { source_hash: ContentHash },
    FrameworkContribution { contribution: ContributionId },
}
```

Generated mappings are valid only when the provider result and mapper use the same generated snapshot. Real/external source spans validate against their own source revision/hash. The current-file mapper is never reused for a foreign target.

### Recovery law

Broken carriers use two rails:

1. native parser/recovery diagnostics in authored coordinates;
2. semantic/provider operations only for regions and mappings that remain stable.

Recovery inserts minimal capability-tagged synthetic structure and preserves authored identifiers. It does not rewrite user tokens into semantically different expressions, weaken strict mapping, or invent source anchors.

### Edit law

Rename, completion resolve, diagnostics, and code actions emit typed **intent**, not final edits. LSO8 alone validates:

- source revision/hash and old text;
- semantic target and authority epochs;
- exact mapping basis;
- syntax-owned insertion anchors;
- overlap, file/path conflict, safety, and atomicity.

A final transaction is all-or-nothing. `0:0`, nearest-position, current-file foreign mapping, regex import placement, and silent overlap resolution are forbidden.

## 3. Engine Provisioning

### Explicit four-stage authority

```text
EPR0 policy
   |
   v
EPR2/EPR3 optional acquisition/shipping
   |
   v
EPR1 artifact validation & immutable receipt
   |
   v
EPR4 candidate resolution & selection plan
   |
   v
EPR5 spawn/handshake/health/ProviderEpoch activation
```

The stages remain separate because they have different correctness and security boundaries:

- **Acquisition** may perform network or release mutation, but only under explicit authorization.
- **Validation** proves artifact identity, compatibility, origin, integrity, trust, and safe installation.
- **Resolution** chooses among authorized validated candidates and performs no network or spawn.
- **Activation** revalidates the handoff, spawns/attaches, handshakes, health-checks, and atomically publishes a `ProviderEpoch`.

### Valid no-download/no-bundle end state

EPR0 may permanently forbid EPR2 and EPR3. In that case the product still converges deterministic explicit/project/editor/system discovery and truthful `NeedInputs`/unavailable behavior. Public policy must remove promises for unopened channels.

### Supply-chain invariants

No executable runs before validation. Managed installs use private no-follow temp roots, bounded safe extraction, atomic rename, and `READY` written last. Integrity/trust/revocation failures remain loud and cannot become “not found; try another source.”

Capabilities are published only after an exact version/protocol/capability handshake and atomic ProviderHub binding. A configured or discovered engine is not an active engine.

## Cross-product interaction laws

- Native Checker may reduce external diagnostic demand by certified family, but cannot disable provider completion/navigation capabilities.
- Language Service uses provider observations through typed adapters and exact `ProviderEpoch`; it never owns provisioning.
- Engine Provisioning publishes truthful active capabilities but does not decide semantic authority between native and external diagnostic families.
- All products use PUB0 outcomes, COX0 participation, VIM-generated conformance, and PER0 equivalent-work evidence.
- Product terminals delete only displaced authority they own. Full TypeScript engine retirement requires a separate future cutover proving no residual diagnostic, language-service, project, or oracle dependency.
