<!-- unified-charter-v2
id=CCA1GT
name=Template-fact semantic consumer cutover
phase=compiler
train=compiler.compiler-bridge
product=compiler_bridge
kind=migration
semantic_role=delivery
class=compiler
predecessors=CCA1E,CCA1F
owner=compiler.compiler-bridge:template-fact authority route and combined-method deletion
conflict_domains=semantic_authority,compiler_execution
resource_class=rust-mixed
review_profile=architecture-3
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
charter=charters/compiler-compiler-bridge/CCA1GT.md
max_production_loc=1200
max_production_files=8
max_related_packages=2
rescope_loc=1500
rescope_files=16
rescope_unrelated_packages=3
-->

# CCA1GT — Template-fact semantic consumer cutover

## Independently acceptable outcome, role, and owners

Route `compile_template_data` through the registered framework's `FrameworkSemanticAuthority`, then delete the displaced `CarrierCompiler::template_data` declaration, `TemplateFacts` wrapper, and Vue/Svelte implementations. Current ownership is the combined compiler method; final ownership is the framework semantic backend selected by immutable catalog identity. This lands and rolls back independently of eval source.

## Exact production population and APIs

No dual producer may survive. Every file below is in-scope; a 9th production file is a rescope.

**Catalog lookup and backend facts**

- `crates/verter_compiler/src/framework_common/registered_carrier_projection.rs` — type-erased semantic catalog lookup; `InstalledSemanticAuthority` is an eval-source fn plus a template-facts fn payload keyed adapter × artifact epoch × Semantic, not a Vue/Svelte match dispatcher. Template-facts lookup binds `compile_source` to `artifact.parse_key()`. `TemplateFactsBasis` makes the effective template explicit: `AdmittedArtifact` uses the registered parse; `SelectedTemplate(bytes)` binds only when those bytes equal the unique admitted `SectionRole::TemplateHost` region's `syntax.content_span` sliced through the artifact inventory. Missing, duplicate, unsliceable, or differing bytes return `None` before catalog lookup or semantic extraction. `content_artifact_token` is not parse admission. On equality the catalog still receives the original carrier source/artifact so spans stay SFC-absolute.
- `crates/verter_compiler/src/framework_common/vue_semantic_authority.rs` — Vue backend template facts over a registered parse artifact. Producer failure is typed `None`, never `RawTemplateData::default()` as success. A valid template-free SFC is `Some` empty facts.
- `crates/verter_compiler/src/svelte/semantic_authority.rs` — Svelte backend template facts over a registered parse artifact. Same refusal versus template-free contract.

**Combined-method deletion and compile-bundle consumer**

- `crates/verter_compiler/src/framework_common/carrier_compiler.rs` — delete the combined method, wrapper, and method-only harness.
- `crates/verter_compiler/src/framework_common/mod.rs` — drop the displaced `TemplateFacts` re-export.
- `crates/verter_compiler/src/framework_common/vue_bridge.rs` — delete the Vue combined-method implementation after route equivalence. `compile_bundle` must not independently extract; when `want_template_data` it fills `template_data` from catalog template facts, passing `SelectedTemplate` whenever `block_content.template` is `Some`.
- `crates/verter_compiler/src/svelte/carrier.rs` — delete the Svelte combined-method implementation after route equivalence. `compile_bundle` must not independently extract; consume catalog template facts, passing `SelectedTemplate` whenever `block_content.template` is `Some`.

**Dispatch**

- `crates/verter_session/src/parse.rs` — replace `file_language_has_template_data_compiler`/`compile_template_data` combined-compiler lookup with semantic catalog selection. Reuse binds adapter, language, and `artifact.parse_key()` to `compile_source`. Direct artifact queries pass `AdmittedArtifact`.

Existing template-data, template-class, host-manage, and Svelte conformance tests are evidence surfaces and do not enlarge the production-file budget.

The boundary is source plus immutable framework/parse identity to complete raw template facts with SFC-absolute byte spans. Selected-template facts bind only when the selected bytes equal the unique admitted TemplateHost region; otherwise the request is `None` before extraction. Serialization keeps its owning UTF-16 conversion. Parse publication, eval source, projection, runtime, assembly, and host routing are excluded. Do not mutate eval-source producer behavior except a second catalog fn that does not couple identity.

## Exact predecessor contracts and binding laws

- **CCA1E:** the Vue semantic backend produces complete template facts from its registered parse artifact.
- **CCA1F:** the Svelte semantic backend produces complete template facts from its registered parse artifact.
- Capability availability replaces filename/framework guessing; unsupported/inapplicable requests return the existing typed absence/refusal and never a fabricated empty success.
- Facts bind source revision, parse identity, provenance, deterministic order, and complete-only admission. Cancelled, stale, partial, or source-mismatched facts publish and warm nothing.
- Selected-template facts bind only when the selected bytes equal the unique admitted TemplateHost region of the registered artifact. Missing, duplicate, unsliceable, or differing bytes refuse before catalog lookup. `content_artifact_token` is not parse admission. Native/no-override queries remain `AdmittedArtifact`.

## Internal subblocks, migration, and deletions

1. Characterize Vue/Svelte fact values, span geometry, fresh/incremental equivalence, and unsupported outcomes.
2. Switch both availability and production dispatch atomically to semantic capability selection.
3. Delete the combined method, `TemplateFacts` compatibility wrapper when unreferenced, both framework implementations, and method-only tests. `compile_bundle` must consume catalog template facts instead of independently extracting.

No shadow/dual producer may survive. Delete no eval-source, IDE, runtime, host, registry, trait, option, or staged-artifact authority.

## Acceptance, performance, aborts, and verification

- **CCA1GT-AC1:** structural evidence finds no combined-trait template-fact declaration, dispatch, implementation, or wrapper residue.
- **CCA1GT-AC2:** Vue/Svelte facts, spans, provenance, deterministic order, and absence/refusal outcomes remain equivalent; a planted hardcoded framework or empty-success fallback fails.
- **CCA1GT-AC3:** fresh, preloaded, incremental, and edit-revert agree; stale/cancelled/partial facts cannot publish or warm.
- **CCA1GT-AC4:** one request performs one fact extraction with no duplicate parse/semantic pass, source copy, or retained candidate; absent/inapplicable demand is zero-work.

Ceiling: 1200 production LOC, 8 production files, 2 crates. Abort on another production caller, semantic divergence, a second resolver, or any eval-source mutation. Run focused compiler/session template-fact, map/span, and Svelte conformance evidence plus `targeted-domain`; CCA1G joins this result with CCA1GE.
