# ADR-004 — TypeScript Compatibility Uses Semantic Profiles, Not Build Versions

**Status:** Accepted  
**Decision owner:** native TypeScript-compatible semantic interpretation  
**Reopen only if:** a supported operation can be proven independent from every profile dimension currently classified as semantic.

## Context

TypeScript compatibility varies by supported TypeScript family and semantics-affecting project options. Rendering, serialization, worker count, cache policy, and internal implementation versions do not change TypeScript meaning and should not over-invalidate semantic results.

## Decision

Every TypeScript-compatible native semantic operation is evaluated under `TypeScriptSemanticProfileId`, containing only dimensions that can change observable interpretation, including:

- supported TypeScript compatibility family/version;
- semantics-affecting compiler options;
- module/module-resolution mode and conditions;
- JSX semantics;
- target/lib basis and exact custom/versioned library fingerprints where relevant;
- package exports/imports, paths, type roots, package-boundary, case, and symlink policy;
- declared supported behavior of the semantic kernel.

The following are separate:

- generated program semantics → `OutputProfileId`;
- diagnostic/type/path rendering → `PresentationProfileId`;
- wire/container layout → `SerializationProfileId`;
- execution placement/worker/cache/deadline/budget → execution policy;
- persistent interpretation safety → compatibility domain/build fingerprint.

An internal refactor does not change semantic profile identity unless observable semantics change. Unsupported profiles fail closed. Verter-specific stricter analysis is separately labeled enrichment.

## Consequences

- caches are semantically complete without progress-version over-keying;
- multiple supported TypeScript compatibility profiles can coexist in one kernel;
- presentation and serialization changes do not invalidate semantic computation unnecessarily.

## Rejected alternatives

- **Global pinned checker with no profile:** cannot represent multi-project compatibility.
- **One giant profile containing every option/version:** over-invalidates and turns implementation history into semantics.
