<!-- unified-charter-v2
id=CCA1O2J
name=NAPI typed host-request callable route
phase=compiler
train=compiler.compiler-bridge
product=compiler_bridge
kind=implementation
semantic_role=delivery
class=compiler
predecessors=CCA1O1B,CCA1O2,CCA1O2H,CCA1O2I
owner=compiler.compiler-bridge:native callable typed host compile-request and batch routes
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
size=S
dispatchable=true
optional=false
release_gating=none
external_requirements=
charter=charters/compiler-compiler-bridge/CCA1O2J.md
max_production_loc=500
max_production_files=5
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# CCA1O2J — NAPI typed host-request callable route

## Independently acceptable outcome and rollback boundary

The durable problem: the native binding's typed host request and its decoder exist as Rust types that the addon only re-exports; no callable host method accepts one. Every callable native compile method still takes a compile profile, and the published declaration says so. A JavaScript caller therefore cannot reach the typed request at all, and the tightened closed-shape decode and the generated declarations that describe it have no route that executes them. Every native consumer told to move onto the typed request is blocked on an entry point that does not exist.

Outcome: the native host object exposes one callable typed compile method and one typed batch route. A caller registers a source once and executes one typed request against it in a single call, and the batch route takes one entry per input carrying its source carrier beside its request. Reverting removes the two routes and their declarations; the decode boundary, the generated declarations, the legacy profile methods, and every consumer stay as they are.

## Concrete surfaces and APIs

- `crates/verter_napi/src/lib.rs`: `compileRequest`, taking the canonical id and the typed request, and `compileRequests`, taking entries and options. Both are bound addon methods on the exported host object, and both public names are fixed here rather than chosen at implementation time.
- Each route decodes the JavaScript request exactly once through the existing native decode boundary, converts it once to the canonical request, and hands it to the session's typed execution entry. Neither may convert the request back into a compile profile.
- `crates/verter_napi/src/host_compile_request.rs`: only the decode-to-canonical handoff this route needs. The own-property materialization and closed-shape rules are unchanged.
- Conversion of the session's typed envelope into the addon's returned value, preserving canonical ids, diagnostics, output bytes, source maps, and public span and offset encoding.
- `packages/native/index.ts`: the declared signatures for both routes. The compile-request types themselves come from the generated declarations; only the non-generated result and route declarations are hand-written here.
- Evidence that the generated addon declaration carries both routes, so the published surface and the addon cannot disagree.
- Focused addon execution tests that call both routes from JavaScript and observe real compiled output, registered in the existing native test harness.
- The batch route preserves the existing one-registration-per-canonical invariant: each entry carries its source carrier exactly once beside its typed request and never registers a source and then copies the same bytes into the request. It returns one entry per input in original input order, each holding a response or a typed failure.
- A refusal throws or fails that entry; it never returns a partial result, a null, an ensure boolean, or a silent fallback to the profile route.
- Consumer migrations and every legacy-profile deletion are excluded. The legacy profile methods remain fully intact here.

## Exact predecessor contract

- **CCA1O1B:** implemented ledger row for “Canonical typed host-request execution seam”; the session executes a caller-supplied canonical request and returns the typed envelope, so this binding has something to call and never has to rebuild a profile.
- **CCA1O2:** implemented ledger row for “NAPI typed host-request adapter”; the tagged native request, its decode function, and its conversion to the shared schema exist at this boundary.
- **CCA1O2H:** implemented ledger row for “NAPI own-property closedness repair”; an own unknown or cross-framework key is refused whatever its value, so the route this node makes callable is closed as declared.
- **CCA1O2I:** implemented ledger row for “Generated native host-request TypeScript mirror”; the request declarations these signatures are written against are generated from the Rust schema and byte-pinned, so the published route cannot describe a shape the decoder refuses.

## Acceptance and evidence

- The published addon exposes both routes: they appear in the generated addon declaration and are callable from JavaScript, and tests invoke them over real object graphs rather than Rust fixtures.
- A caller performs one source-only registration and then one typed call, and receives the requested products for both Vue and Svelte carriers; the run makes no additional native call and copies no source into the request.
- The batch route registers each entry's source exactly once, returns one entry per input in original input order, and isolates a per-entry failure to that entry.
- Output bytes, source maps, diagnostics, canonical ids, and public span and offset encoding are equivalent to the legacy route for the same demand.
- An unknown or cross-framework property is refused on these routes whatever its value, and the refusal names the offending property where the schema names it.
- A refusal throws or fails that entry rather than returning a partial result, a null, or a boolean, and no path on these routes constructs a compile profile from the typed request.
- The legacy profile-bearing methods keep their existing declarations and behavior, and their tests still pass unchanged.

## Deletions, budgets, and aborts

- Delete nothing. The legacy profile methods and their declarations belong to a later deletion node and must survive this change intact.
- Planning guidance: roughly 500 LOC across 5 files in 2 related crates/packages. These figures are guidance, not a cap; rescope only under the program's mandatory thresholds or when a consumer migration or the browser binding enters.
- Abort on converting the typed request back into a profile, on a second decode path, on a hand-written duplicate of a generated request declaration, on a batch entry registering its source more than once, or on any legacy deletion.

## Verification and review

Add the addon execution tests first, inspect the generated addon declaration, run the native binding and package suites and `targeted-domain`. Apply `public-3`; add only CCA1O2J's ledger row.
