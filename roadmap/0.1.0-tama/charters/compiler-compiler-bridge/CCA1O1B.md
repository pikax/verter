<!-- unified-charter-v2
id=CCA1O1B
name=Canonical typed host-request execution seam
phase=compiler
train=compiler.compiler-bridge
product=compiler_bridge
kind=implementation
semantic_role=delivery
class=compiler
predecessors=CCA1O1,CCA1O1A
owner=compiler.compiler-bridge:session entry executing a canonical compile request and its typed result envelope
conflict_domains=compiler_execution,host_service_graph,public_protocol
resource_class=rust-mixed
review_profile=public-3
gate_profile=targeted-domain
implementation_effort_min=high
implementation_effort_default=high
review_effort_min=high
review_effort_default=high
verification_effort_min=high
verification_effort_default=high
confirmation_effort_min=high
confirmation_effort_default=high
size=M
dispatchable=true
optional=false
release_gating=none
external_requirements=
charter=charters/compiler-compiler-bridge/CCA1O1B.md
max_production_loc=700
max_production_files=7
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# CCA1O1B — Canonical typed host-request execution seam

## Independently acceptable outcome and rollback boundary

The durable problem: the typed framework-discriminated compile request exists as a schema and a converter, and nothing executes one. Every session compile route still derives its framework demand from `CompileProfile` — `compile_request_build.rs` builds the framework host backend's demand from profile axes, and both compile lanes reach it that way — so there is no session API a caller can hand a canonical `CompileRequest` to. A binding that decoded a typed request would have to convert it back into a profile to run it, which reinstates the untyped vocabulary one layer down and silently drops any demand the profile cannot express. Every consumer migration behind this gap is therefore unimplementable as written.

Outcome: the session host exposes one entry that accepts a caller-supplied canonical `CompileRequest` for an already registered source, executes it through the existing bound framework host integration, and returns a typed result envelope. The canonical request is the demand document end to end: nothing on this route reconstructs a `CompileProfile` from it. Reverting removes the entry, the envelope, and the batch projection; the legacy profile routes are untouched throughout and remain the only production route until their own consumers migrate.

## Concrete surfaces and APIs

- `crates/verter_session/src/host_resolve/compile_request_build.rs`: accept a caller-supplied canonical request as the demand source for the bound backend, beside the existing profile-derived construction the legacy lanes keep using.
- `crates/verter_session/src/host_resolve/virtual_file_pipeline.rs`: the execution path the new entry drives, including the admission and artifact publication it shares with the existing lanes.
- `crates/verter_session/src/host_compile.rs`: the typed batch projection, preserving the existing one-registration-per-canonical batch invariant and its single-call batch route.
- Session request/response types and their focused tests.
- `verter_compiler`'s `FrameworkHostIntegrationBackend` and the Vue and Svelte host integrations, only as far as consuming the supplied request requires. Host integration remains the sole `CompileAdmission` issuer; no product backend mints one.
- The seam is one compile-and-return call: a canonical id plus a request, returning the response. For migrated typed consumers it replaces the ensure-then-read pair. The source is NOT part of the request: a caller registers the source once through the existing source-only upsert, and the entry reads that already-stored immutable snapshot by canonical id. Aliases resolve once, and a request whose framework arm contradicts the registered carrier is refused rather than compiled under the wrong carrier.
- The batch projection takes one entry per input, each carrying its existing source carrier exactly once beside its typed request; it does not register a source and then copy the same bytes into the request. It returns one entry per input in original input order, each holding a response or a typed failure.
- The response carries the canonical id, diagnostics, and a discriminated product list in request order. Each row is one-to-one with a requested product kind: runtime rows expose the existing stable virtual-file payloads, the IDE row exposes the existing IDE response, and the analysis row exposes the existing host analysis payload. The public-API and declaration kinds stay typed unsupported here, exactly as both host integrations already refuse them.
- The result is complete-only. A decode, construction, admission, or execution refusal fails the whole request; it never returns partial siblings, a null, an ensure boolean, or a silent fallback to profile-derived demand.
- This is a host compatibility envelope over the products the host already produces. It is not the later artifact-set schema, which stays owned elsewhere.
- Protocol and FFI schema changes, JavaScript bindings and their published declarations, consumer migrations, and every legacy deletion are excluded.

## Exact predecessor contract

- **CCA1O1:** implemented ledger row for “Typed FFI host compile-request schema”; the framework-discriminated request and its exact fail-closed conversion to the canonical compiler request exist, so this node executes an already-canonical request rather than defining one.
- **CCA1O1A:** implemented ledger row for “Canonical Svelte custom-element prop-type admission”; the Svelte custom-element prop-type slot has its final shape, so a request reaching this entry cannot carry a superseded closed vocabulary.

## Acceptance and evidence

- A caller registers a source once and then executes one canonical request for it, receiving the response in one call; no profile is constructed anywhere on that path, and structural evidence proves the route builds no `CompileProfile` from the supplied request.
- Every requested product kind the host can produce is returned in request order and is byte-equivalent to what the legacy route produces for the same demand, including output bytes, source maps, diagnostics, canonical ids, and span meaning.
- A request whose framework arm contradicts the registered carrier is refused with a typed failure; it is never compiled under the registered carrier instead.
- A request naming an unregistered canonical id fails; a request for the public-API or declaration kind returns the typed unsupported outcome rather than an empty success.
- An admission or execution refusal fails the whole request and publishes no sibling product, and no partial response, null, or ensure boolean can be observed on this route.
- The batch projection returns one entry per input in original input order, registers each source exactly once, and copies no source into a request; a per-input failure isolates to that entry.
- The legacy ensure and cached-read pair, their profile normalization, and their cached-slot behavior are unchanged and still pass their existing tests; the legacy read stays a pure cached read.
- Evidence is TDD at the session boundary for each acceptance above, plus an equivalence comparison against the legacy route for both frameworks over the shared product set.

## Deletions, budgets, and aborts

- Delete nothing. No legacy lane, profile route, converter, or cached-slot behavior is removed here, and no second execution authority may survive beside the bound host integration.
- Planning guidance: roughly 700 LOC across 7 files in 2 related crates. These figures are guidance, not a cap; rescope only under the program's mandatory thresholds or when a binding, protocol schema, or consumer enters.
- Abort on reconstructing a `CompileProfile` from the supplied request, on a second admission issuer, on duplicate parse or analysis for one admission, on adding source bytes to the request, on a partial or nullable result, or on the batch route registering a source more than once per canonical.

## Verification and review

Use TDD at the session boundary, run the session and compiler suites and `targeted-domain`. Apply `public-3`; add only CCA1O1B's ledger row.
