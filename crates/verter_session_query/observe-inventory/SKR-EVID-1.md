# Paged dependency-evidence inventory

Classification follows [the observation policy](../../../docs/arch/semantic-observe.md).

## verter_session_query

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| Evidence pages: `ResultEvidence` of `EvidenceKind::Page` minted by `ResultReceipt::page` (a fixed-width contiguous run of one signature's canonical entries, its digest, canonical set, aggregated domains and resolution-evidence summary), including the index levels a level of pages is paged into | Every warm read validates every leaf through the shared receipt walk (`StoreView::validate_fact_signature`, `CapturedResolutionWorld::validates_fact_version`, the request view's per-receipt validation memo); reverse-index registration and selective invalidation read the page's canonical set through `ReadSetSignature::canonical_ids`; `signature_entries` reads pages through for root reconstruction, self-root discrimination and admission-rail lookups | REQUIRED | Immutable; shared by `Arc` with every signature, candidate, refusal summary and enclosing signature that holds it; freed with the last holder | `src/facts/receipt.rs`, `src/facts/fact_read_set.rs` | always |
| `ResultEvidence::kind` (`EvidenceKind::{Result, Page}`) | Signature ordering and equality (a page and a result receipt over the same facts stay distinct evidence); `signature_entries` reads only pages through | REQUIRED | The evidence it belongs to | `src/facts/receipt.rs` | always |
| `ResultEvidence::retention` (a page's `Pinned` `RetentionCharge`) | The process retention account charges each page once for its whole life, so the pinned total lowers the refusable headroom every `Retained`/`Active` admission sees | REQUIRED-lifetime | The page; released exactly once by the last holder's drop | `src/facts/receipt.rs` | always |
| `ResultReceipt::retained_charge_bytes` | Current page charge for retention evidence (tests); a production count accessor available with capture off | REQUIRED-lifetime | Read on demand from the live charge | `src/facts/receipt.rs` | always |
| `ReadSetSignature { facts }` (the `overflowed` bit removed) | Every fact-validated cache stores the complete, paged rail; non-admission is decided by `SignatureAdmission`, never by a flag on the signature | REQUIRED | The candidate / carrier holding it | `src/facts/fact_cache.rs` | always |

## verter_type_engine

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| Semantic-operand evidence unions (`union_operand_evidence`, `seal_substitution`), sealed whole through `seal_canonical_signature` | The force's operand evidence: every input's facts are carried, paged when wide, and revalidated before a minted operand serves | REQUIRED | One forced operand | `src/project_semantic_dispatch/semantic_operand.rs` | always |
| Completed structural carriers (`bound_completed_structural_carrier`): a carrier wider than one page carries its strict self-roots as one `StrictSelfRootWorld` and is sealed whole | Memo, shape and refusal-summary warm reads validate the strict-world witness and every page | REQUIRED | The candidate | `src/fact_signature_helpers.rs` | always |
| `RefusalSummary::carrier` (a deterministic refusal's proving prefix, now never refused for its width) | An exact isolated-root repeat revalidates the whole prefix before it answers; the summary's retention charge counts only top-level entries because its pages are charged by themselves | REQUIRED | Bounded FIFO and the process retention account, as before | `src/semantic_query_memo/refusal_summary.rs` | always |

## verter_session

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| `ValidatedFactCache` candidate signatures: a signature wider than one page is sealed into pages on admission | Route, imported-root and fallthrough warm reads validate every page | REQUIRED | The candidate (FIFO slot of `CANDIDATE_CAP`) | `src/resolver_core/mod.rs` | always |
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
