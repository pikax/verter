# Engine extraction prerequisite ratification — 2026-10-02

## Context

The captured SKR-ENGINE contract requires a moves-only extraction with no
production dependency from the engine onto the session or concrete scheduler.
It assigns the request-bound port redesign to SKR-ENGINE-PORTS. The operator's
2026-10-02 ruling requires that redesign to land before extraction, while the
captured plan and the ports charter require extraction first.

The source establishes a real prerequisite, independently of change size:

- `project_semantic_dispatch/build.rs::stitch_module_augmentations` reaches
  `host.workspace().reverse_deps_for` through
  `ResolverContext::host_for_fact_tracer_install`. Augmentation collection also
  reaches `ProjectTypeStore::indexed` and session-owned artifact population.
- `resolver_core/resolver_context.rs` returns `ProjectTypeStore`, `HostConfig`,
  `HostStoreView`, `VerterHost` and `ProjectSemanticDispatch`. Its default
  cancellation implementation also uses the concrete scheduler. Moving this
  trait unchanged does not remove those dependencies.
- `decl_body_memo.rs` imports session-owned fact lenses, resolver helpers and
  provenance. The source half needs a verified neutral boundary too; an engine
  import rename alone does not establish both crates' closure.

These paths are relative to `crates/verter_session/src/`. The six replacement
ports are not implemented in the inspected session and query-boundary sources.
There is no partial extraction to clean up.

## Intent Contract

**Verdict: RESCOPE. Implementation remains incomplete.**

SKR-ENGINE keeps its identity, both crate extractions, the neutral DTO moves,
owner unit tests, old-path deletion, and every acceptance obligation AC1–AC5.
It may start the extraction only after SKR-ENGINE-PORTS delivers the boundary
required to make those moves acyclic. The operator's ports-first ruling governs
the sequencing. Neither the missing prerequisite nor advisory size estimates
justify abandoning extraction or marking it accepted.

SKR-ENGINE-PORTS keeps the six ports: IndexedInputs, OwnedLowering, RouteLookup,
FactValidation, Cancellation and ExecutionSubmission. It must deliver its
existing behavior-preserving contract against the current session modules and
neutral query boundary, without requiring the extracted crates to exist first.
Host construction and scoped lifetimes stay with K3; residual source,
observation and audit narrowing stays with ARH4; production handler composition
stays with ARH6. No new task replaces SKR-ENGINE.

The complete replacement SKR-ENGINE charter is supplied in the recorded
`ratification-decision@2` response. This document records the requested ruling;
it does not create a repository-owned DAG or certify implementation.

## Changes

- The controller must remove the extraction-to-ports prerequisite before
  establishing ports-to-extraction ordering. Adding the reverse edge alone
  creates a cycle. Preserve V8 and H3 as prerequisites of extraction, and make
  their standing contracts prerequisites of the preparatory port work.
- Reconcile the existing ports charter's predecessor, source paths, comparison
  baseline and focused port-test command with its pre-extraction location.
  Preserve its four acceptance outcomes, including no ambient access, unchanged
  behavior, discriminating capability tests and no new per-node indirection.
- Existing extraction consumers, including SKR-ENGINE-TESTS, SKR-OBS-D, the
  representation optimizations and L4-A, must continue to wait for actual
  extraction acceptance. A documentary ruling cannot release those consumers.
- Re-derive the relocation map after the ports land. The inspected canonical
  algebra is `project_semantic_dispatch/canonical_algebra.rs`, signature code is
  the session sibling `signature_kernel/`, and continuation inputs include
  `semantic_execution.rs` and `project_semantic_dispatch/query_frames.rs`.
  These locations correct the map's assumptions; they do not authorize moving
  host ownership into the engine.
- The requested dated record is the only repository change. This checkout has
  no existing roadmap charter, ledger, authority files, neighbouring decisions
  or roadmap validator to amend. Controller state remains the authority; the
  response must retain the unresolved sequencing and implementation work.

## Legacy Deletions

No source is deleted by this ruling. At extraction, delete the old engine and
source implementations, duplicate DTO definitions and compatibility re-exports
as AC3 requires. The ports owner removes the displaced whole-host access path
in its own cutover. No temporary engine-to-session dependency, second resolver,
duplicate cache or incomplete crate facade is authorized.

## Verification

Before extraction, inspect the delivered ports and re-derive the complete
production dependency closure under all production features. Concrete host,
artifact, route and scheduler ownership must remain outside the engine; source
workers must not call the engine.

At extraction, retain the normalized before/after nextest population and ignore
set, canonical gate, dependency-closure guards with planted violations, old-path
deletion proof, workspace Clippy, protocol freshness, native/TypeScript and WASM
build lanes, and build-isolation proof. Timings remain advisory; paired
performance measurement remains SKR-PERF-0's. Apply V8's standing resource,
baseline and typed-reuse contracts and preserve H3's publication behavior.

Check this documentary change with whitespace validation and the controller's
response-schema validation. Commands, results, platform and operational
candidate identities belong in the response, not this record. Missing
implementation or unavailable required checks cannot be reported as passed.
Validate live behavior without historical Git-object, ancestry or commit-message
proof requirements. Independent review, CodeRabbit and required CI remain
separate landing gates.
