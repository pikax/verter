# Scheduler generation-floor retirement inventory

Classification follows [the observation policy](../../../docs/arch/semantic-observe.md).

## verter_scheduler

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| Generation floors (per-canonical removed-generation history) | None; external publication is fenced by `SourceWitness` and host identities by `SourceVersion` | REQUIRED-lifetime, retired | No population or backing allocation; removal, reset, unknown removal and cancellation record nothing | `src/scheduler.rs`, `src/scheduler/lifecycle.rs` | absent |
| DAG retirement floor kept for external publishers | None; removal sweeps the removed node's generations in one transition and later stale admissions are refused by the live object witness | REQUIRED-lifetime, retired | No retained floor; the sweep runs inside one lifecycle transition | `src/dag.rs`, `src/scheduler/lifecycle.rs` | absent |
| `SourceWitness` (canonical, node incarnation, generation) | `commit_artifact`, `remove_artifact_not_newer_than`, `try_get_source_for_witness` | REQUIRED | Held by the external publisher for one publication or cleanup; no scheduler-side storage | `src/node.rs`, `src/scheduler.rs` | always |
| `SourceSnapshot.incarnation`, `AnalysisSnapshot.incarnation` and `SourceVersion` | Host base revision token, raw-template version rail, upsert commit fence | REQUIRED | One scalar per committed snapshot, stamped by the driver at commit; retires with the snapshot | `src/node.rs`, `src/scheduler/driver.rs` | always |
| Successor node generation start | Node identity: a removed or reset file's successor starts at generation 0; a language re-home continues its predecessor's generation | REQUIRED-lifetime | One node object | `src/scheduler/lifecycle.rs`, `src/scheduler.rs`, `src/scheduler/completion.rs` | always |

## verter_session

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| `HostSourceRevisionToken.file_incarnation` for scheduler-registered sources | LSP host-revision staleness checks and external block request owner revisions | REQUIRED | Carried by `HostSourceData` for the committed snapshot's lifetime | `src/host_executor.rs` | always |
| `RawTemplateAnalysisEntry.source_version` and `RawTemplateSlotAdmission.source_version` | Raw-template slot install ordering and reader validation | REQUIRED | One entry per canonical in `DerivedRawState`; dropped on removal and cleared by invalidation | `src/types.rs`, `src/host_manage/analysis_io.rs` | always |
| Upsert commit fence on `SourceVersion` | `finish_upsert_post_commit` and committed carrier route publication refuse a read-back from another node object or generation | REQUIRED | One upsert transaction | `src/host_upsert.rs`, `src/host_manage/prepared_decl.rs` | always |

Removal, reset, unknown removals and cancellation leave no generation history:
the scheduler's node table, DAG, auto-ingest tracking, deferred blockers and
source-root versions drain, and the node table's backing capacity stops
growing with the length of the removal history. No event history or
production per-operation feature check is introduced.
