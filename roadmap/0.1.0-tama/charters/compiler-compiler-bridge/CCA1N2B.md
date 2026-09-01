<!-- unified-charter-v2
id=CCA1N2B
name=Request-scoped host binding cutover
phase=compiler
train=compiler.compiler-bridge
product=compiler_bridge
kind=migration
semantic_role=delivery
class=compiler
predecessors=CCA1N2A
owner=compiler.compiler-bridge:one production framework binding per immutable host request
conflict_domains=compiler_execution,host_service_graph
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
charter=charters/compiler-compiler-bridge/CCA1N2B.md
max_production_loc=700
max_production_files=7
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# CCA1N2B — Request-scoped host binding cutover

## Independently acceptable outcome and owners

Every host compile attempt creates exactly one `BoundNativeHostRequest` from its immutable request snapshot at one common production binding point and threads the consumed binding into the existing host-backed and runtime-render compatibility routes. Both outer `compile_bundle` calls remain. Current ownership is per-route classifier branches; final ownership is the single binding point. Reverting restores only the displaced classifier branches and the unthreaded routes.

## Concrete surfaces and boundary

- Surfaces: the common binding point and threading in `crates/verter_session/src/host_resolve/virtual_file_pipeline.rs`, `crates/verter_session/src/host_resolve/compile_request_build.rs`, `crates/verter_session/src/host_executor.rs`, and `crates/verter_session/src/host_compile.rs`, plus focused tests.
- Owns: one binding per compile attempt; source-generation, framework-identity, host-epoch, and audit-identity coherence carried by the binding; threading into both existing compatibility routes; and deletion of the compile-lane classifier branches displaced by binding — the `is_vue` classification and threading in `virtual_file_pipeline.rs` and the `is_vue` parameter and shared framework fork in `compile_request_build.rs`.
- The Vue-else-Svelte grammar fallthrough in `host_executor.rs` is NOT binding-displaced — it runs in the shared source-ingestion stage for every parse, including parses no compile request triggered, where no request-scoped binding can exist. Its replacement by immutable-catalog registered grammar is a separate deletion population owned exclusively by the registered-grammar-selection node; this node must not delete, replace, or work around it, and must not forge, store, or widen a binding to reach that stage.
- After the shared framework fork is deleted, the fixed-Vue runtime-render compatibility route keeps a render-lane-only Vue request constructor (the characterized transitional request shape pinned by the fixed-Vue route's characterization evidence); the binding still supplies its identity/audit coherence. That constructor is retained residue whose deletion the bound runtime-render execution cutover exclusively owns; this node performs no execution cutover.
- Excluded: product-backend semantic changes, host-backed outer-call deletion, runtime-render outer-call deletion, public transport schema changes, cache behavior changes, and assembly relocation.

## Exact predecessor contract

- **CCA1N2A:** the sealed binding substrate exists with guarded construction, typed unavailability, and non-Clone consume-once discipline.

## Invariants and acceptance

- No production site re-derives framework identity from language classification; the binding point is the sole framework-identity derivation site for host compile requests. Two carve-outs remain until their owning cutovers: the render lane's characterized fixed-Vue request-shape constructor (deleted by the bound runtime-render cutover) and both retained outer calls' registry dispatch by artifact identity (deleted with their outer calls).
- Exactly one binding is created per compile attempt over one immutable snapshot; a supersession-driven re-snapshot binds anew for its own attempt, and a warm hit that performs no compile creates no binding.
- Both old outer `compile_bundle` calls still exist.
- Existing output, refusal, diagnostic, and cache behavior remains characterized and equivalent for both lanes and both frameworks.
- A stale source generation, mismatched artifact identity, or mismatched framework/host epoch fails closed with a typed outcome and publishes nothing; duplicate consumption of one binding is structurally impossible or a typed failure.
- Binding creates no parse, semantic, runtime, assembly, or copy work; equivalent-work counters are unchanged.

## Deletions, budget, and verification

Delete only the displaced compile-lane classifier branches named above. Ceiling: 700 production LOC, 7 files, 2 crates; abort on outer-call deletion, DTO change, or cache mutation. Run host-backed virtual/batch, runtime-render, and audit suites plus `targeted-domain`. CCA1N4 consumes the bound request for execution cutover.
