# ARH4 evidence cases

The sole owning interface is `node tests/architecture-health/ARH4/verify.mjs` (verify) plus `node --test tests/architecture-health/ARH4/arh4.test.mjs` (dirty twins), wired into `test:scripts` and the CI `architecture-health` lane. The verifier joins the shipped ARH1 and ARH2 predecessors (imported `validate()`, never re-implemented) and the live host, audit runtime and scheduler sources. No DAG copy is read from this tree.

The host audit runtime mints and solely owns the host's audit records store; `VerterHost` keeps no second handle and no audit-record methods of its own. Every audit record is keyed by an id from the host's one request-id counter. Construction, source management, observation and resolver access carry no residual beyond the landed construction root and engine ports. The two ARH1 register rows bound to ARH4 are executed: `Scheduler.deferred_blocker_ids` is `pub(crate)`, and every scheduler hook the ARH1 contract narrows to test configuration carries the test-support gate.

## ARH4-ratification (accept)

Clean products validate. Manifest cases, products and verify/test commands match the verifier; `test:scripts` and the CI lane run both commands. Tests exercise the verifier without commit metadata.

## ARH4-cutover (reject) — AC1

- `inventory-*`: the six concerns are exactly construction, source management, observation, resolver access, audit access and the scheduler source-staging surface; each names a surviving owner type its path declares; a `no-residual` concern carries evidence, and a `narrowed` concern claims cutover rows that exist, each claimed exactly once.
- `owner-*`: CUT-1 survives on `HostAuditRuntime::{new, take_record, publish_record}` with `new(config)` minting the store; CUT-2 on `VerterHost::next_request_id`.
- `consumer-*`: every recorded consumer reaches its owner through the recorded call form, and the production population reaching the audit runtime's drain/publish forms equals the recorded set both ways.
- `register-row-*`: CUT-3/CUT-4 execute ARH1-CUT-2/ARH1-CUT-3, whose ARH1 owner is ARH4 and whose route is the scheduler; the field is declared `pub(crate)` and every ARH1-narrowed hook carries a `cfg` test gate.

The deleted host routes are held by the compiler: no consumer can call a method that no longer exists, so no check spells retired names.

## ARH4-authority (reject) — AC2

An independent audit records store, or a second request-id authority keying records into the host's store, is rejected by `audit_records_per_host_isolated_two_hosts_dedicated_record_stores` and `unregistered_and_audited_records_share_one_host_minted_key_space`. The second fails on the tree before this change: an unregistered read minted id 1 from a process-wide counter, the audited read minted id 1 from the host, and the later record replaced the earlier one. Dirty twins drop the key-space test, unbind a test name or empty the defect.

## ARH4-work (reject) — AC3

Cancellation binds the dropped-registration tests (the runtime's publish route keeps registration-first finalize). Deterministic ordering binds the key-space test: the same work on two fresh hosts mints the same ids whatever other hosts ran. Fresh/incremental, edit/revert and stale/partial are not applicable (no cache, query, map or fence state is touched). Dirty twins empty evidence, unbind a test, drop a rationale or invent a state.

## ARH4-delivery (reject) — AC4

No VIM/DX capability or wire surface changes. Each Rust API migration note names a target documented in `docs/audit-footprint/api-reference.md`.

## ARH4-cost (reject) — AC5

No latency, RSS or allocation budget is bound and work counts are unchanged, so `arh4-perf` is not applicable and has no manifest; the verifier fails if the disposition and the manifest's presence disagree, or if a wall-clock dimension is committed.
