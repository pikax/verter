# Dependency-evidence inventory

Classification follows [the observation policy](../../../docs/arch/semantic-observe.md).

## verter_session_query

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| `FactSet` (one canonical, deduplicated set of facts per sealed signature: a shared slice when narrow or freshly sealed no wider than `WIDE_SIGNATURE_WIDTH`, a persistent tree when sealed wider or when a wide set is extended from another) and its order-independent `digest` | Every fact-validated cache stores it as its rail; readers iterate, search and range over it in place; equality, ordering and hashing read its content | REQUIRED | The candidate / carrier holding it; tree storage shared with every set built from it | `src/facts/fact_set.rs` | always |
| Fact-tree nodes (`fact_tree::Node`: leaves of up to 16 inline facts, branches of up to 16 children) and each node's own `RetentionCharge` (exactly the node's allocation: `Pinned` from the seal of the edit that built it, exchanged for a `Retained` share when a cache admission claims it) | The retention account charges each node once for its whole life, released by the node's own drop; a retaining cache claims the unclaimed nodes a signature reaches into its refusable reservation (`reserve_retained_with_evidence`), so a wide candidate is refused for its whole new footprint | REQUIRED-lifetime | The node; released exactly once when its last holder drops | `src/facts/fact_tree.rs` | always |
| `fact_tree::Node::claimed` (set once a retaining admission claimed the node and its whole subtree) | A claim walk stops at a claimed node, so repeated admissions of sets sharing claimed storage do not rewalk it | REQUIRED | The node; only ever turns on | `src/facts/fact_tree.rs` | always |
| `FactSet::tree_footprint` / `tree_footprint_of` (distinct nodes, their blocks and the retained and pinned bytes their charges hold) | Retention evidence and diagnostics; production accessor available with capture off | REQUIRED-lifetime | Read on demand from the live charges | `src/facts/fact_set.rs` | always |
| `ResultEvidence::retention` (a consumed result's receipt's one `RetentionCharge`, sized by the storage it owns: `Pinned` from birth, exchanged for a `Retained` share when a cache admission claims it) and `ResultEvidence::evidence_claimed` | A retaining cache claims the unclaimed receipts a signature reaches together with its tree nodes; a claimed receipt graph is not rewalked | REQUIRED-lifetime | The receipt; released exactly once by the last holder's drop | `src/facts/receipt.rs` | always |
| `claim_retained_storage` (the no-own-bytes form of `reserve_retained_with_evidence`; reserves nothing for a signature that reaches nothing unclaimed) | Stores that retain a signature without charging bytes of their own (`cache_runtime` artifact and query-candidate publishes, compile slots, binder-identity facts, owner import surfaces, framework surface and resolved script-fact stores, the app-config proof seed) claim its storage before admitting; a refused claim stores nothing and delivers the value uncached | REQUIRED-lifetime | The claimed storage, until its last holder drops | `src/facts/receipt.rs` | always |
| `ResultReceipt::retained_charge_bytes`, `ResultReceipt::retained_charge_class` | Current receipt charge and its class for retention evidence (tests); production accessors available with capture off | REQUIRED-lifetime | Read on demand from the live charge | `src/facts/receipt.rs` | always |
| `RetentionCharge::split_off` | Hands one granted reservation to the owners of each allocation it covers, so each share releases with its own owner | REQUIRED-lifetime | The charge it is split from | `src/retention/mod.rs` | always |
| `ReadSetSignature { facts: FactSet }` | Every fact-validated cache stores the complete rail; non-admission is decided by `SignatureAdmission`, never by a flag on the signature | REQUIRED | The candidate / carrier holding it | `src/facts/fact_cache.rs` | always |

## verter_type_engine

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| Semantic-operand evidence unions (`union_operand_evidence`, `seal_substitution`), sealed whole through `seal_canonical_signature` | The force's operand evidence: every input's facts are carried and revalidated before a minted operand serves | REQUIRED | One forced operand | `src/project_semantic_dispatch/semantic_operand.rs` | always |
| Completed structural carriers (`bound_completed_structural_carrier`): sealed onto the traced set they complete, sharing its storage; a carrier wider than `WIDE_SIGNATURE_WIDTH` whose completion made it wide carries its strict self-roots as one `StrictSelfRootWorld` (dropping every precise root fact); a traced set already wider keeps its precise roots. The torn self-root check reads the traced set in place | Memo, shape and refusal-summary warm reads validate the strict-world witness or the precise roots and every fact; a disagreeing traced root refuses as `SelfRootConflict` | REQUIRED | The candidate | `src/fact_signature_helpers.rs`, `src/semantic_query_memo/mod.rs` | always |
| `ArtifactNode::retention_account`, `QueryNode::retention_account` (default: the process account) and the cold-winner storage claim in `cache_runtime::node` | Before a cacheable cold result is published its signature's unclaimed storage is claimed; a refused claim bubbles the signature into the outer tracer and returns the value `ReturnOnly` under `RetentionPressure` | REQUIRED-lifetime | The published entry or candidate holds its share of the claimed storage | `src/cache_runtime/node.rs` | always |
| `RefusalSummary::carrier` (a deterministic refusal's proving prefix, never refused for its width) | An exact isolated-root repeat revalidates the whole prefix before it answers; sealing claims the carrier's unclaimed storage into the summary's one `Retained` reservation | REQUIRED | Bounded FIFO and the process retention account | `src/semantic_query_memo/refusal_summary.rs` | always |

## verter_session

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| `ValidatedFactCache` candidate signatures: sealed and stored as the candidate's own copy, charged inline with the candidate (`Candidate::_evidence_retention` claims the receipts the copy reaches) | Route, imported-root and fallthrough warm reads validate every fact; a candidate whose footprint the account refuses is not admitted | REQUIRED | The candidate (FIFO slot of `CANDIDATE_CAP`) | `src/resolver_core/mod.rs` | always |
| Storage claims on non-participant retaining stores: `CompileOutputNodeFactValidatedSession::publish`, `BinderIdentityFactsStore::insert`, `OwnerImportSurfaceDb::insert_owned`, `FrameworkSurfaceStore::insert`, `FrameworkScriptFactStore::publish_if_cacheable`, `AppConfigNoOverrideProofDb::publish` | Each admission claims its signature's unclaimed storage through `claim_retained_storage` on the process account (the compile, binder and script-fact stores hold a `StoreAccount`); a refused claim stores nothing and the caller keeps the complete value | REQUIRED-lifetime | The stored entry holds its share of the claimed storage until its last holder drops | `src/compile_output_node.rs`, `src/binder_identity_facts.rs`, `src/owner_import_surface.rs`, `src/framework/surface_store.rs`, `src/framework/script_facts.rs`, `src/app_config_proof_db.rs` | always |
| `MetaProvenance::app_config_proof_non_cacheable_refusals` (and its snapshot field) | none; measurement only | OPTIONAL | Host-lifetime atomic; reset by the provenance reset | `src/meta_provenance.rs`, `src/app_config_proof_db.rs` | `cfg(feature = "semantic-observe")` |

Width is never a refusal and leaves no producer of an overflow signal: there
is no per-host signature-overflow counter, port hook or provenance counter,
and `StructuredAuditEvent::FactSignatureOverflow` /
`NonAdmissionReason::SignatureOverflow` are never emitted (both stay on the
audit wire as schema values). No optional counter or trace is added: sets,
tree nodes and their charges are validity and lifetime state.

The test-support forcing knobs (`TestKnobs::force_fact_tracer_non_cacheable_read`
and the named one-shot refusal claimed through `arm_fact_tracer_refusal_once`)
exist only under `test`/`test-support` and are absent from default builds.
