# Stage 5+6 Cutover — BLOCK PLAN (working; awaiting codex binding sign-off on Q1-Q4)
Branch base: integration `refactor/semantic-db-overhaul` (re-verify HEAD at each block land; was 63d682f69).
Implements codex BINDING verdict (D:/tmp/stage5-codex-verdict.txt) + reachability (D:/tmp/stage5-reachability.md).
Approach: port verter_compiler macro lowering off parser resolve_type/ OXC resolver → shared dispatch
(SemanticQueryKey::ResolveMacroPayload), delivered as a compiler-OWNED DTO produced by session/host; parser macro
parsing becomes spans-only; THEN delete the legacy rail. Four NEVERS. Each block = commit-first, lands independently.

## GAP ANALYSIS (Plan agent abf3a9d, evidence-cited)
- 5A crux: build_resolve_macro_payload (build.rs:3889) returns a raw resolved SemanticNodeId; normalization to
  props/emits/slots DTOs happens one layer up in vue_macro_dtos_with_ctx (surface.rs:602) / resolve_vue_macro_surface_with_ctx
  (surface.rs:224), using macro_object_surface(Shallow, MacroTypeArgOwnBody) for props, (Shallow, Structural) for emits/slots.
  Type-checking DTO is shape-complete for props(type+required+default)/emits(event+payload)/slots. MISSING for codegen:
  (1) runtime constructors, (2) typed unresolved-macro diagnostic signal.
- Runtime constructors: TWO RuntimeType uses — type-arg path (infer_runtime_type, resolve_type/infer.rs:35 — DOOMED) vs
  object-arg path (extract_runtime_types_from_expr, setup.rs:939, reads author-written JS constructors — SURVIVES). RuntimeType
  enum + format_runtime_types are SHARED → relocate (object-arg copy survives), delete only infer_runtime_type. [Q1 to codex]
- DTO boundary: today host pre-resolves ResolvedElements keyed by type-name → external_types options → parser RE-RESOLVES via
  resolve_type_elements_with_ctx_ref. Target: host resolves+normalizes EVERYTHING through dispatch, hands compiler a finished
  owned DTO; parser does ZERO resolution. New type verter_compiler::compile::ResolvedMacroSurfaces {props/emits/slots/unresolved,
  RuntimeCtorKind} produced by VerterHost::resolve_macro_surfaces_for(canonical) (session/dispatch-backed), passed via a renamed
  options field replacing external_types on VerterCompileOptions/ScriptCodeGenOptions/TscGenOptions.

## BLOCKS (codex 5A→5B→5C; B1-B2 independent land-first)
- **B1 [independent]** delete dead parse_checker_text_to_type_expr (checker_text_adapter.rs + export mod.rs:36 + guard allowance
  architecture_guards.rs:10877 + self-tests/bench). codex §5: no prod caller. +absence guard.
- **B2 [independent]** relocate lowering helpers (lower_call_signature_payload, lower_function_shape, lower_ts_type_from_bytes)
  from resolve_type/elements.rs → verter_type_expr_oxc. [Q2: if no caller outside resolve_type/, collapse into B10]
- **B3 [5A]** add compiler-owned ResolvedMacroSurfaces/MacroPropDto/MacroEmitDto/MacroSlotDto/RuntimeCtorKind in new
  verter_compiler/src/compile/macro_dto.rs (pure types, unwired). +guard: verter_compiler has no verter_session dep.
- **B4 [5A]** session-side runtime_constructors_from_type_expr(&TypeExpr)->Vec<RuntimeCtorKind> (semantic-IR analogue of
  infer_runtime_type) in typeinfo/adapters/vue/runtime_ctor.rs. TDD table-tests per TypeExpr variant. dep B3.
- **B5 [5A GATE]** dispatch-backed ResolvedMacroSurfaces producer: VerterHost::resolve_macro_surfaces_for, assembling from
  VueMacroDtos normalization (surface.rs:668) + B4 constructors + per-macro unresolved flag (Miss/empty-surface + macro span) +
  default/required/optional. Cache via fact-validated result DB. dep B3,B4. Dispatch tests: defineProps<Props> extends/Pick/Omit/
  imported/union/indexed; emits call-sig+tuple; slots; withDefaults; unresolved-import; invalidation on imported-type edit.
- **B6 [5B]** switch runtime codegen (script/macros.rs:152/301/220) to DTO; replace external_types field w/ macro_surfaces on
  ScriptCodeGenOptions+process.rs merge; wire host populate on VDOM/Vapor compile entry. Keep object-arg path. dep B5.
  Regression: external_type_defineprops_generates_runtime_props/_optional_prop/defineemits/withdefaults_uses_key_name/typeof/
  define_props_typed_inline/interface_ref/export_interface_resolves.
- **B7 [5B]** switch TSC consumer (tsc/script.rs:1333/1512) to DTO; TscGenOptions.external_types→macro_surfaces; wire
  virtual_file_pipeline.rs:1442/1461. Keep object-arg branch. dep B5.
- **B8 [5B]** switch compile diagnostics (compile/mod.rs:134/154/326) XInvalidMacroType to DTO unresolved flag. dep B5.
  Tests: test_define_emits_invalid_imported_type_reports_error, test_define_props_unresolvable_type_reports_error.
- **B9 [5B parser flip]** MacroTypeParams spans-only (drop resolved/runtime_types/unresolved_type_ref); extract_type_params
  (setup.rs:782) stops calling resolve_type_elements/infer_runtime_type; relocate object-arg RuntimeType/format_runtime_types/
  runtime_type_to_constructor/runtime_types_to_ts to surviving module; delete infer_runtime_type; bindings.rs:377 prop-names from
  DTO [Q3]. dep B6,B7,B8.
- **B10 [5C]** delete resolve_type/{mod,elements,decl,infer,external}.rs + re-exports + resolve_type_elements*/extract_companion_types/
  TypeResolutionContext/ResolvedElements/ResolvedProp/ResolvedEmit/format_runtime_types(type-arg). Drive RESOLVED_ELEMENTS_FILE_ALLOWLIST
  + RESOLVE_TYPE_PATH_FILE_ALLOWLIST (architecture_guards.rs:13613/13580) to EMPTY. dep B9.
- **B11 [5C]** delete session external-type rails (external_type_resolution.rs, external_macro_collector.rs, external_macro_types.rs
  collect_external_macro_types, frontier ResolvedElements producers, route_owned_shallow.rs:613) + SemanticNodeData::VueMacroElements +
  HostResolvedNamedTypeKey + parser NamedTypeCache adapters + graph named-type indexes. dep B10. [Q4: native_props FFI carrier
  route_owned_shallow.rs:625 must re-source from B5 DTO or be retired FIRST]
- **B12 [5C close]** final absence guards (no prod refs to resolve_type_elements/infer_runtime_type/extract_companion_types/
  ResolvedElements/RuntimeType-type-arg/TypeResolutionContext/VueMacroElements/HostResolvedNamedTypeKey/parse_checker_text_to_type_expr);
  ledgers at empty floor; negative-guard test. dep B10,B11.

## OPEN QUESTIONS (to codex binding sign-off)
- Q1 (B9): RuntimeType/format_runtime_types SHARED w/ surviving object-arg path → plan RELOCATES them, deletes only infer_runtime_type
  (type-annotation walker). codex verdict said "delete RuntimeType outright". CONFIRM refinement or correct.
- Q2 (B2/B10): do lower_call_signature_payload/lower_function_shape have callers outside resolve_type/? If not, collapse B2 into B10.
- Q3 (B9): cleanest spans-only source for template-scope prop NAMES (bindings.rs:377) post-deletion. Cross-file defineProps<Imported>
  has no local names → must come from DTO. Likely: bindings derive from B5 DTO (makes B9 depend on B6 host wiring).
- Q4 (B11): native_props FFI carrier via collect_external_macro_types (route_owned_shallow.rs:625) — re-source from B5 DTO or retire first.
- B5 risk: unresolved flag must distinguish valid-empty-object (defineProps<{}> no diagnostic) from unresolvable-ref (defineProps<Missing>
  diagnostic) — legacy unresolved_type_ref distinction (setup.rs:823). Dispatch signal: Error(Miss) on Ref vs Value(empty Object).

## CRITICAL FILES
surface.rs (DTO producer :602/:224), build.rs:3889 (5A dispatch core), script/macros.rs:152 + tsc/script.rs:1333 (compiler consumers),
setup.rs:782 + resolve_type/{mod,infer}.rs (parser flip + deletion), architecture_guards.rs:13613/12104 (5C ledger).

## ✅ CODEX BINDING SIGN-OFF CORRECTIONS (D:/tmp/stage5-signoff-verdict.txt) — these OVERRIDE the blocks above
- Q1 CONFIRMED: delete `infer_runtime_type` (type-arg walker, resolve_type/infer.rs:35, called setup.rs:841). KEEP/move the
  object-arg runtime model (extract_runtime_types_from_expr setup.rs:939; obj props setup.rs:1009/1052/1056; consumed tsc/script.rs:1391/1439).
  Do NOT keep format_runtime_types for type-arg resolution.
- Q2 CHANGE: **DROP B2.** lower_call_signature_payload/lower_function_shape/lower_ts_type_from_bytes (resolve_type/elements.rs:25/37/652)
  have NO caller outside resolve_type/ (only resolve_type/decl.rs:26/1178/1269). DELETE with B10 + absence guard. verter_type_expr_oxc
  already owns the lowerer (lib.rs:37, call/fn :427/:546).
- Q3 BIND: template prop-names from B5 DTO (VueMacroDtos.props store.rs:96 ← AnalyzedPropField.name types.rs:981 ← vue_macro_dtos_with_ctx
  surface.rs:602 / props_from_typeinfo_surface :669). B9 DEPENDS ON B6 host wiring; bindings.rs:377 consumes macro_surfaces.props[].name
  for type-based props; runtime object/array props stay syntax-only.
- Q4 BIND: native_props is a LIVE public FFI surface (ResolvedMacroMeta.native_props mod.rs:53/75; component_meta.proto:630;
  verter_ffi convert/component_meta.rs:383) built via collect_external_macro_types (external_macro_types.rs:16/46, route_owned_shallow.rs:613/626).
  FOLD native_props into ResolvedMacroSurfaces (or sibling DTO) BEFORE B11; gate B11 deletion on it. **CRITICAL NEW SCOPE: class VISIBILITY
  exists in legacy rail (resolve_type/mod.rs:218, decl.rs:1189) but NOT in typeinfo surfaces (semantic_query.rs:1054, typeinfo/surface.rs:116).
  ADD visibility semantic data to the DTO/typeinfo surface, OR explicitly remove the public API — do NOT silently drop it.**
- B5 SPLIT (binding): (i) structured resolution OUTCOME first (make resolve_vue_macro_surface_with_ctx return structured outcome OR sibling
  probe in resolve_macro_surfaces_for), (ii) DTO normalization second. build_resolve_macro_payload does NOT preserve valid-empty-vs-unresolved
  (no-args Opaque(Miss) build.rs:3942; 1-arg passthrough :3944; vue_macro_dtos collapses failure to empty surface.rs:619/622; projection None on
  query error shallow_surface.rs:153). EXPLICIT check in B5 host DTO builder: defineProps<{}> → unresolved=false; defineProps<Missing> → unresolved=true.
- B3 EXTEND: DTOs carry `unresolved` + native-prop/visibility data (not just props/emits/slots/constructors).
- B9 ordering: AFTER B6 (bindings consume DTO prop names). B11 GATED on native_props fully owned by B5 DTO path.
- REVISED block list: B1 → (B2 dropped) → B3(extended) → B4 → B5(split i+ii) → B6 → B7 → B8 → B9(after B6) → B10(+helper deletes from old B2) → B11(gated on Q4) → B12.
- FIRST BLOCK: **B1** (delete parse_checker_text_to_type_expr: checker_text_adapter.rs:57 def, mod.rs:36 export, bench checker_text_adapter.rs:45, guard architecture_guards.rs:10877).
