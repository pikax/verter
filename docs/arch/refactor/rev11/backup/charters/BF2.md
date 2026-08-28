# BF2 — Official-core compiler conformance harness

**Status:** PROPOSED / LOCKED. **Class:** Foundational test infrastructure.
**Predecessor:** BF1.

## Objective

Build hermetic test-only infrastructure that can falsify framework output against the
exact official domains without supplying production behavior.

## Owned scope

- offline official compiler invocation and immutable golden generation;
- generated fragment and assembled JavaScript parsing;
- import/export and exact-package linking;
- Vue script/template assembly validation;
- parser-backed cosmetic normalization and structural/topology comparison;
- deterministic client and server execution against official runtimes;
- hydration controls and meaningful cross-pairings;
- diagnostics, source-map, and TypeScript-observable product validation;
- official-case extraction, disposition, coverage accounting, and provenance; and
- normalizer negative/mutation tests with proven mutation application.

BF2 cannot change production compiler behavior, implement a runtime, patch generated
output, inject helpers, mock missing exports, use a forbidden corpus, or let candidate
output update expectations.

## Required exits

`FC-HARNESS-001`, `FC-MANIFEST-001`, and `FC-NORMALIZER-001` pass. Harness self-tests
prove source/package drift refusal, offline execution, non-vacuous official and
candidate arms, expected-golden immutability, parse/link/runtime failure detection,
atomic result accounting, diagnostic/mapping discrimination, and every forbidden
normalizer mutation. Every seed manifest declaration is runner-enumerated or has a
reviewed allowed disposition. Performance cells locked by BF1 pass.

## Abort/rescope

Stop if an official runner cannot be made hermetic, official dynamic cases cannot be
enumerated, expected provenance is incomplete, the runtime requires output patching,
or a normalizer rule would erase semantic structure.
