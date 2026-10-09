# Paged dependency-evidence inventory

Classification follows [the observation policy](../../../docs/arch/semantic-observe.md).

## verter_session_query

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| Evidence pages: `ResultEvidence` of `EvidenceKind::Page` minted by `ResultReceipt::page` (a fixed-width contiguous run of one signature's canonical entries, its digest, canonical set, aggregated domains and resolution-evidence summary), including the index levels a level of pages is paged into | Every warm read validates every leaf through the shared receipt walk (`StoreView::validate_fact_signature`, `CapturedResolutionWorld::validates_fact_version`, the request view's per-receipt validation memo); reverse-index registration and selective invalidation read the page's canonical set through `ReadSetSignature::canonical_ids`; `signature_entries` reads pages through for root reconstruction, self-root discrimination and admission-rail lookups | REQUIRED | Immutable; shared by `Arc` with every signature, candidate, refusal summary and enclosing signature that holds it; freed with the last holder | `src/facts/receipt.rs`, `src/facts/fact_read_set.rs` | always |
| `ResultEvidence::kind` (`EvidenceKind::{Result, Page}`) | Signature ordering and equality (a page and a result receipt over the same facts stay distinct evidence); `signature_entries` reads only pages through | REQUIRED | The evidence it belongs to | `src/facts/receipt.rs` | always |
| `ResultEvidence::retention` (a page's one `RetentionCharge`, sized by the storage the page owns: `Pinned` from birth, exchanged for a `Retained` share when a cache admission claims the page) | The retention account charges each page once for its whole life; a retaining cache claims a signature's unclaimed pages into its refusable reservation (`reserve_retained_with_evidence`), so a wide candidate is refused for its whole new footprint | REQUIRED-lifetime | The page; released exactly once by the last holder's drop | `src/facts/receipt.rs` | always |
| `ResultEvidence::reaches_pages` (whether the evidence is or holds a page at any depth, set at sealing) and `ResultEvidence::pages_claimed` (set once a retaining admission has claimed every page reachable from it) | `reserve_retained_with_evidence` walks every receipt kind but skips evidence that reaches no page or whose pages are already claimed, so a page behind a consumed result's receipt is claimed and repeated admissions do not rewalk a claimed graph | REQUIRED | The evidence it belongs to; `pages_claimed` only ever turns on, because a claimed page never returns to a pin | `src/facts/receipt.rs` | always |
| `claim_evidence_pages` (the no-own-bytes form of `reserve_retained_with_evidence`; reserves nothing for a signature that reaches no unclaimed page) | Stores that retain a signature without charging bytes of their own (`cache_runtime` artifact and query-candidate publishes, compile slots, binder-identity facts, owner import surfaces, framework surface and resolved script-fact stores, the app-config proof seed) claim its pages before admitting; a refused claim stores nothing and delivers the value uncached | REQUIRED-lifetime | The claimed pages, until their last holder drops | `src/facts/receipt.rs` | always |
| `ResultReceipt::retained_charge_bytes`, `ResultReceipt::retained_charge_class` | Current page charge and its class for retention evidence (tests); production accessors available with capture off | REQUIRED-lifetime | Read on demand from the live charge | `src/facts/receipt.rs` | always |
| `RetentionCharge::split_off` | Hands one granted reservation to the owners of each allocation it covers, so each share releases with its own owner | REQUIRED-lifetime | The charge it is split from | `src/retention/mod.rs` | always |
| `ReadSetSignature { facts }` (the `overflowed` bit removed) | Every fact-validated cache stores the complete, paged rail; non-admission is decided by `SignatureAdmission`, never by a flag on the signature | REQUIRED | The candidate / carrier holding it | `src/facts/fact_cache.rs` | always |

## verter_type_engine

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| Semantic-operand evidence unions (`union_operand_evidence`, `seal_substitution`), sealed whole through `seal_canonical_signature` | The force's operand evidence: every input's facts are carried, paged when wide, and revalidated before a minted operand serves | REQUIRED | One forced operand | `src/project_semantic_dispatch/semantic_operand.rs` | always |
| Completed structural carriers (`bound_completed_structural_carrier`): a carrier whose top level is wider than one page carries its strict self-roots as one `StrictSelfRootWorld` (dropping every precise root fact, paged ones included) and is sealed whole; one whose width is held by pages keeps its precise roots. The torn self-root check reads traced pages through | Memo, shape and refusal-summary warm reads validate the strict-world witness or the precise roots and every page; a disagreeing paged root refuses as `SelfRootConflict` | REQUIRED | The candidate | `src/fact_signature_helpers.rs`, `src/semantic_query_memo/mod.rs` | always |
| `ArtifactNode::retention_account`, `QueryNode::retention_account` (default: the process account) and the cold-winner page claim in `cache_runtime::node` | Before a cacheable cold result is published its signature's unclaimed pages are claimed; a refused claim bubbles the signature into the outer tracer and returns the value `ReturnOnly` under `RetentionPressure` | REQUIRED-lifetime | The published entry or candidate holds its share of the claimed pages | `src/cache_runtime/node.rs` | always |
| `RefusalSummary::carrier` (a deterministic refusal's proving prefix, now never refused for its width) | An exact isolated-root repeat revalidates the whole prefix before it answers; sealing claims the carrier's unclaimed pages into the summary's one `Retained` reservation, and the summary's own estimate counts only top-level entries | REQUIRED | Bounded FIFO and the process retention account, as before | `src/semantic_query_memo/refusal_summary.rs` | always |

## verter_session

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| `ValidatedFactCache` candidate signatures: a signature wider than one page is sealed into pages on admission, and the admission claims every unclaimed page the signature reaches (`Candidate::_evidence_retention`) | Route, imported-root and fallthrough warm reads validate every page; a candidate whose pages the account refuses is not admitted | REQUIRED | The candidate (FIFO slot of `CANDIDATE_CAP`) holds its share of the claimed pages until its last holder drops | `src/resolver_core/mod.rs` | always |
| Page claims on non-participant retaining stores: `CompileOutputNodeFactValidatedSession::publish`, `BinderIdentityFactsStore::insert`, `OwnerImportSurfaceDb::insert_owned`, `FrameworkSurfaceStore::insert`, `FrameworkScriptFactStore::publish_if_cacheable`, `AppConfigNoOverrideProofDb::publish` | Each admission claims its signature's unclaimed pages through `claim_evidence_pages` on the process account (the compile, binder and script-fact stores hold a `StoreAccount`); a refused claim stores nothing and the caller keeps the complete value | REQUIRED-lifetime | The stored entry holds its share of the claimed pages until its last holder drops | `src/compile_output_node.rs`, `src/binder_identity_facts.rs`, `src/owner_import_surface.rs`, `src/framework/surface_store.rs`, `src/framework/script_facts.rs`, `src/app_config_proof_db.rs` | always |
| `MetaProvenance::app_config_proof_non_cacheable_refusals` (and its snapshot field) | none; measurement only | OPTIONAL | Host-lifetime atomic; reset by the provenance reset | `src/meta_provenance.rs`, `src/app_config_proof_db.rs` | `cfg(feature = "semantic-observe")` |

Removed with the width refusal, leaving no producer: the per-host
`VerterHost::signature_overflow_at_install` counter and its port hook
(`FactValidation::record_signature_overflow`), `ValidatedFactCache`'s
signature-overflow counter, the `memo_entry_overflow_refusals` and
`owner_import_surface_overflow_refusals` provenance counters, and the emission
of `StructuredAuditEvent::FactSignatureOverflow` /
`NonAdmissionReason::SignatureOverflow` (both stay on the audit wire as schema
values). No optional counter or trace is added: pages, their summaries and
their charges are validity and lifetime state.

The test-support forcing knobs (`TestKnobs::force_fact_tracer_non_cacheable_read`
and the named one-shot refusal claimed through `arm_fact_tracer_refusal_once`)
exist only under `test`/`test-support` and are absent from default builds.
