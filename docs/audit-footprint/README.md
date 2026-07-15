# Audit Footprint

Deterministic, request-scoped observability for every audited
`VerterHost` entry-point — `get_component_meta`, `resolve_type`,
`compile`, `analyze`, workspace ops, LSP handlers, MCP tools, and
bundler batches. Exposes exactly what a single request loaded,
instantiated, projected, and materialized — plus the per-context
cache counters that explain whether the request was served cold,
warm, or by joining an in-flight peer.

The audit record is the sole observability authority for the
audited surfaces. The audit substrate (`verter_audit`) holds the
DTOs and the producer-side `AuditObserver` trait; the session layer
(`verter_session`) owns the host runtime, records store,
registration lifecycle, and footprint miner.

## Concepts

### Request context

Every audited request installs a thread-local
[`RequestContext`](https://github.com/pikax/verter/blob/main/crates/verter_session/src/request_context.rs)
that carries:

- `request_id: u64` — monotonic; zero reserved for "not populated".
- `canonical_id: Arc<str>` — the component being resolved.
- `footprint_capture: bool` — whether the accumulator is attached.
- Per-context `AtomicU64` counters (`cold_builds`, `warm_hits`,
  `joined_waits`, `sentinels`, `inflight_aborted_retries`,
  `cold_aborts_swept`). These are **exact under concurrency** —
  distinct requests never overlap.

The context propagates through `verter_scheduler::OpaqueRequestContext`
into worker threads. Auto-ingested dependency Source jobs inherit
their parent request's context, so VFS reads for dependency loads
land in the audit record alongside the request that caused them.

### Hermetic vs `attach_to`

Two entry points — see `AuditedRequest::builder()`:

- `hermetic()` (default) — constructs a fresh `VerterHost` with
  `audit_enabled + footprint_capture` both on, runs the request,
  returns the triple `(analysis, resolution, record)`.
- `attach_to(host)` — audits against an existing host. Concurrent
  audits on distinct threads are isolated by
  per-context counters; same-thread nested audits are rejected with
  `NestedAuditNotSupported`.

### Loaded files vs declared-dependency files

Two distinct contracts on `RequestFootprintAudit`:

- `loaded_files()` — exactly the canonical IDs the scheduler
  actually read on behalf of this request. Union of `vfs_reads`
  and `shared_load_reuses`, sorted + deduplicated.
- `declared_dependency_files()` — broader set that also includes
  fresh `IndexedReady` builds. Useful for dependency-graph
  rendering; **not** an exact-read contract.

Assertion helpers mirror both:
`RequestAuditRecord::assert_loaded_files_exactly` and
`RequestAuditRecord::assert_declared_dependency_files_exactly`. Pick
the helper whose semantics match the fixture's intent.

### Walker — `why_loaded` / `why_instantiated`

Iterative backward walk over the derivation subgraph. Single
implementation in Rust (`RequestAuditRecord::why_loaded`); NAPI + WASM
bindings serialize `ProvenanceChain` JSON, TS helpers render the
chain as plain text. Depth cap is 256 hops; `SharedLoadReuse`
terminates only the affected branch (winner-context details
attached as terminals).

## Quick start

### Rust — test harness

```rust
use verter_session::audited_request::AuditedRequest;

let (analysis, resolution, record) = AuditedRequest::builder()
    .files([
        ("/Owner.vue", OWNER_SFC_SRC),
        ("/types.ts", "export type Props = { n: number };"),
    ])
    .resolve("/Owner.vue")?;

record.assert_loaded_files_exactly(["/Owner.vue", "/types.ts"])?;
let chain = record.why_loaded("/types.ts");
println!("{}", verter_session::component_meta_audit::render_chain_text(&chain));
```

### NAPI — consumer side

```ts
import { ComponentMetaHost, whyLoaded, decodeAuditBundle } from "@verter/native";

const host = new ComponentMetaHost({
  auditEnabled: true,
  footprintCapture: true,
});
host.upsertBase("/Widget.vue", SFC_SRC);
const session = host.openSession();

const buffer = session.getComponentMetaWithAudit("/Widget.vue");
const bundle = decodeAuditBundle(buffer);
if (bundle) {
  const chain = whyLoaded(session, bundle, "/Widget.vue");
  // `chain.steps` is BFS-ordered; `chain.terminated` explains why the walk stopped.
}
```

### WASM — playground

Structural counterpart:

```ts
import { whyLoaded } from "@verter/wasm";

const bundle = session.getComponentMetaWithAudit("/Widget.vue");
if (bundle) {
  const chain = whyLoaded(session, bundle, "/Widget.vue");
}
```

## Debug workflow — dumping the JSON

Set `VERTER_COMPONENT_META_AUDIT_JSON_OUT` to a filesystem path;
the host writes the record there after every audited request
completes (pretty-printed JSON):

```bash
VERTER_COMPONENT_META_AUDIT_JSON_OUT=/tmp/audit.json pnpm ...
```

Inspect with any JSON tool:

- **macOS / Linux**: `jq '.footprint.loaded_files' /tmp/audit.json`.
- **Windows PowerShell**: `Get-Content /tmp/audit.json | ConvertFrom-Json | Select-Object -ExpandProperty footprint`.
- **VS Code**: open the file; built-in JSON preview + outline.
- **Python cross-platform**: `python -m json.tool /tmp/audit.json`.

## u64 / i64 transport

Every audit integer field larger than 32 bits — signed or
unsigned — transports as a decimal string. `RequestAuditRecord`'s
`request_id`, `bytes_read`, `duration_ns`, and
`process_rss_delta_bytes` (i64) round-trip through
`JSON.parse` / `JSON.stringify` with zero precision loss.
Consumers that need arithmetic call `BigInt(s)`.

u32 and smaller remain JS `number`.

## Troubleshooting

- **`AuditNotEnabled` on `getComponentMetaWithAudit`** — construct
  the host with `auditEnabled: true, footprintCapture: true`.
- **`AuditRecordMissing { request_id }`** — the store is bounded
  to 256 entries; long-running processes with many audited
  requests can displace older records. Drain records with
  `take_audit_record` shortly after resolution.
- **Empty `vfs_reads` on a real request** — typically a capture-site
  TLS propagation gap. Auto-ingested dep Source jobs thread the
  parent's context onto the `QueueEntry`; other gaps may surface
  as new features land. File a bug with the specific request +
  expected read set.
- **`has_orphan_edges: true` in the footprint** — the miner
  truncated the derivation subgraph at
  `HostConfig::max_derivation_edges` (default 10 000). Raise the
  limit or investigate why the request pulls in that many edges.

## See also

- [API reference](./api-reference.md)
- [Structured events](./structured-events.md)
- Audit substrate crate:
  [`crates/verter_audit`](https://github.com/pikax/verter/blob/main/crates/verter_audit/src/lib.rs)
- Host runtime:
  [`crates/verter_session/src/host_audit_runtime.rs`](https://github.com/pikax/verter/blob/main/crates/verter_session/src/host_audit_runtime.rs)
- Architecture skill: `/audit-infrastructure`
- Component-meta consumer skill: `/component-meta`
