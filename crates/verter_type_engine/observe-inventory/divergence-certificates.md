# Divergence-certificate inventory

Classification follows [the observation policy](../../../docs/arch/semantic-observe.md).

## verter_type_engine

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| `ExactSteps` (the dispatcher's witness, minted by `ProjectSemanticDispatch::exact_steps` from the top build-local taint frame, or by `ProjectSemanticDispatch::exact_steps_since` from every frame between the one producing a claimed instantiation and the top, and the request's cancellation: no partial, nothing the memo would refuse to keep, a live request) | `CertifiedDivergence::exact_recurrence` and `CertifiedDivergence::uniform_growth`, the only certificate constructors: a conditional tail run whose arguments come round again or grow uniformly, and the lib `Awaited<T>` conditional re-entering an application on its path (in its own walk, or an enclosing build's through every frame since); without the witness executable tail/Awaited transitions continue under their budgets; an unfinished cross-query `Awaited` producer remains a same-path partial | REQUIRED | One certification; consumed by value, never stored | `src/project_semantic_dispatch/build.rs` | always |
| `UniformGrowth` (the dispatcher's witness, minted by `ProjectSemanticDispatch::uniform_growth`: one argument changed, and it is the declaration's growth wrapper over the current one) | `CertifiedDivergence::uniform_growth`: a conditional tail run that can never take its other branch | REQUIRED | One certification; consumed by value, never stored | `src/project_semantic_dispatch/build.rs` | always |
| `BuildLocalTaint::produces` (field: the identity of the `Instantiate` key a build frame or continuation frame builds) | `ProjectSemanticDispatch::exact_steps_since`, which finds the producer of a same-path claim on the build stack | REQUIRED | One build frame | `src/project_semantic_dispatch/mod.rs` | always |
| `InstantiateBuild::tail_current` and `InstantiateBuild::growth` (fields: the tail run's current arguments, and the declaration's `GrowthShape` read once per build) | `ProjectSemanticDispatch::uniform_growth` | REQUIRED | One instantiation build | `src/project_semantic_dispatch/build.rs` | always |
| `LibAwaitedWalk::depth`, `path` and `on_path` (existing fields: nested instantiation depth and active membership) | Local recurrence without an exact witness continues through the same evaluator; repeated synchronous transitions charge work and enter the existing depth rail, while tail transitions retain their tail allowance | REQUIRED | One library Awaited evaluation | `src/project_semantic_dispatch/build.rs` | always |
| `ProjectSemanticDispatch::lib_awaited_for_tests` (direct library-walk test seam) | Session tests exercise incomplete caller observations without a new cold-query frame hiding them | OPTIONAL | One test call; no stored state | `src/project_semantic_dispatch/build.rs` | `cfg(any(test, feature = "test-support"))` |

The test seam exposes the existing evaluator only under `test`/`test-support`; it adds no observation capture or production call. No store, counter or trace is added. A certificate keeps nothing beside its recovery node (`QueryError::CheckerRecovery` with `RecoveryBasis::Certified`), which the memo keeps under the holding computation's read set, so a helper edit retires it with that read set.

## Superseding notes

These rows of [`crates/verter_session/observe-inventory/SKR-FACTS-2.md`](../../verter_session/observe-inventory/SKR-FACTS-2.md) no longer describe the code:

- `ComponentMetaEvidence` no longer carries a divergence certificate, and `FinalComponentAdmissionProof` no longer has `divergence_certificate`: a certified divergence is a complete answer and is admitted as one, so no partial is ever retained on a certificate's account.
- The component-meta span no longer counts certificates (work, bytes, trip and schedule cuts remain).
- The divergence-certificate note (`ConnectedDemandLedger::note_divergence_certificate`) is removed: a same-path recursion delivery proved nothing and is no longer recorded as a certificate.
- The final-component refusal query replays operation-profile and isolated-root refusals only; the certified-divergence refusal class is removed.
