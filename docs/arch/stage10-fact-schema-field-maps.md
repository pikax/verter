# Stage 10 — exhaustive field-to-fact schema maps

This is the committed [P2] enumeration for the Stage-10 closed fact substrate
(`crates/verter_type_expr/src/facts.rs`, `locators.rs`, `span_origins.rs`). For
each narrowed source struct it maps EVERY current field to exactly one of:

- **fact field** — a required stored fact field (identity-participating,
  `#[serde]`-persisted, publication/projection filter-read, or reconstruction
  EMIT-required metadata);
- **origin locator** — an identity-participating span recovered via a
  producer-emitted origin locator (`span_origins.rs`), never a stored `Span`;
- **body locator** — a `TypeExpr`-bearing body replaced by a content-free
  locator (`locators.rs`), the single graph-engine-routed escape;
- **carve-out (display-only)** — a human-readable passthrough with no semantic
  authority, never a fact.

This table is the obligation the B3–B7 producers must satisfy. Producers
destructure their source struct EXHAUSTIVELY (no `..`), so a struct that grows a
live field fails compilation until it appears in one of the four columns here
(see the demo producer `verter_semantic/src/analysis/fact_projection.rs`).

The B1 substrate defines these fact types and their witnesses; consumer flip and
`TypeExpr` field removal land later (atomic squash). No fact stores `Span`,
`MemberSpans`, `FunctionSpans`, `IndexSignatureSpans`, `TypeExpr`,
`SemanticNodeId`, or `HotTypeRef`; each is `Eq + Hash + NoTypeExpr +
NoStoredSpan` (asserted in `verter_type_expr/src/fact_witnesses.rs`).

---

## A. Analyzed* (`verter_semantic/src/analysis/types.rs`)

### `AnalyzedPropField` → `facts::AnalyzedPropFieldFact`

| source field | disposition |
|---|---|
| `name: String` | fact field `name` |
| `is_optional: bool` | fact field `is_optional` |
| `span: Span` | origin locator `name_span_origin: MemberSpansOrigin` (prop-name span) |
| `type_annotation: Option<String>` | carve-out (display-only) |
| `type_expr: Option<TypeExpr>` | body locator `payload: Option<MacroPayloadLocator>` |
| `type_expr_scope: Option<TypeExprScope>` | fact field `type_expr_scope` (scope pairing) |
| `description: Option<String>` | carve-out (JSDoc display) |
| `tags: Vec<JsdocTag>` | carve-out (JSDoc display) |
| `resolution_source: TypeResolutionSource` | carve-out (display/diagnostic) |
| `resolution_error: Option<String>` | carve-out (display/diagnostic) |
| `declared_in_macro_type_arg: bool` | fact field `declared_in_macro_type_arg` (policy-consumed) |

### `AnalyzedEmitField` → `facts::AnalyzedEmitFieldFact`

| source field | disposition |
|---|---|
| `name: String` | fact field `name` |
| `span: Span` | origin locator `name_span_origin: MemberSpansOrigin` |
| `payload_type: Option<String>` | carve-out (display-only) |
| `payload_expr: Option<TypeExpr>` | body locator `payload: Option<MacroPayloadLocator>` |
| `payload_expr_scope: Option<TypeExprScope>` | fact field `type_expr_scope` |
| `description: Option<String>` | carve-out |
| `tags: Vec<JsdocTag>` | carve-out |

### `AnalyzedSlotField` → `facts::AnalyzedSlotFieldFact`

| source field | disposition |
|---|---|
| `name: String` | fact field `name` |
| `is_required: bool` | fact field `is_required` |
| `span: Span` | origin locator `name_span_origin` |
| `bindings: Vec<AnalyzedSlotFieldBinding>` | fact field `bindings: Arc<[AnalyzedSlotFieldBindingFact]>` |
| `return_type: Option<String>` | carve-out |
| `return_expr: Option<TypeExpr>` | body locator `payload: Option<MacroPayloadLocator>` |
| `return_expr_scope: Option<TypeExprScope>` | fact field `type_expr_scope` |
| `description: Option<String>` | carve-out |
| `tags: Vec<JsdocTag>` | carve-out |

### `AnalyzedSlotFieldBinding` → `facts::AnalyzedSlotFieldBindingFact`

| source field | disposition |
|---|---|
| `name: String` | fact field `name` |
| `type_annotation: Option<String>` | carve-out |
| `binding_expr: Option<TypeExpr>` | body locator `payload: Option<MacroPayloadLocator>` |
| `binding_expr_scope: Option<TypeExprScope>` | fact field `type_expr_scope` |
| `span: Span` | origin locator `name_span_origin` |

### `AnalyzedOptionsProp` → `facts::AnalyzedOptionsPropFact`

| source field | disposition |
|---|---|
| `name: String` | fact field `name` |
| `span: Span` | origin locator `name_span_origin` |
| `type_constructor: Option<String>` | fact field `type_constructor` (Vue ctor name) |
| `is_required: bool` | fact field `is_required` |
| `has_default: bool` | fact field `has_default` |
| `default_value: Option<String>` | carve-out (runtime expr source) |
| `type_annotation: Option<String>` | carve-out |
| `type_expr: Option<TypeExpr>` | body locator `payload: Option<MacroPayloadLocator>` |
| `type_expr_scope: Option<TypeExprScope>` | fact field `type_expr_scope` |
| `description: Option<String>` | carve-out |
| `tags: Vec<JsdocTag>` | carve-out |

### `AnalyzedExposeField` → `facts::AnalyzedExposeFieldFact`

| source field | disposition |
|---|---|
| `name: String` | fact field `name` |
| `span: Option<Span>` | origin locator `name_span_origin` (Synthetic for type-arg-surface members with no authored object-literal span) |
| `type_expr: Option<TypeExpr>` | body locator `payload: Option<MacroPayloadLocator>` |
| `type_expr_scope: Option<TypeExprScope>` | fact field `type_expr_scope` |
| `description: Option<String>` | carve-out |
| `tags: Vec<JsdocTag>` | carve-out |

### `AnalyzedMacro` → `facts::AnalyzedMacroFact`

| source field | disposition |
|---|---|
| `kind: AnalyzedMacroKind` | fact field `kind: AnalyzedMacroKindFact` (structural role authority) |
| `is_type_based: bool` | fact field `is_type_based` |
| `type_references: Vec<String>` | fact field `type_references` |
| `binding_name: Option<String>` | carve-out (binding identifier; not a semantic authority — recoverable from the AnalyzedMacro payload site) |
| `model_name: Option<String>` | fact field `model_name` |
| `has_inherit_attrs_false: bool` | fact field `has_inherit_attrs_false` |
| `prop_fields: Vec<AnalyzedPropField>` | projected to `AnalyzedPropFieldFact` (per A above; carried on the owning surface, not duplicated on the macro fact) |
| `emit_fields: Vec<AnalyzedEmitField>` | projected to `AnalyzedEmitFieldFact` |
| `slot_fields: Vec<AnalyzedSlotField>` | projected to `AnalyzedSlotFieldFact` |
| `default_keys: Vec<String>` | carve-out (withDefaults keys; runtime, display-only) |
| `default_values: Vec<AnalyzedDefaultValue>` | carve-out (runtime default expr text) |
| `expose_fields: Vec<AnalyzedExposeField>` | projected to `AnalyzedExposeFieldFact` |
| `resolved_local_types: Vec<ResolvedLocalType>` | projected to `ResolvedLocalTypeFact` (see D) |
| `parsed_type_argument: Option<Arc<TypeExpr>>` | body locator `parsed_type_argument: Option<MacroPayloadLocator>` |
| `parsed_type_argument_scope: Option<TypeExprScope>` | fact field `parsed_type_argument_scope` |
| `span: Span` | origin locator (recovered via the macro-call `MacroPayloadLocator`; no stored macro-call span field) |

`AnalyzedDefaultValue { key, value, span }` — `key` carve-out, `value` carve-out
(runtime expr text), `span` recovered-via-locator; the whole struct is a
display-only default-value passthrough, not a semantic fact.

---

## B. Projected* (`verter_semantic/src/analysis/type_solver/query_engine.rs`)

### `ProjectedMember` → `facts::ProjectedMemberFact`

| source field | disposition |
|---|---|
| `name: String` | fact field `name` |
| `ty: TypeExpr` | body locator `ty: TypeBodySlot` |
| `optional: bool` | fact field `optional` |
| `readonly: bool` | fact field `readonly` |
| `is_method: bool` | fact field `is_method` |
| `visibility: MemberVisibility` | fact field `visibility` |
| `declared_in_macro_type_arg: bool` | fact field `declared_in_macro_type_arg` |
| `spans: MemberSpans` | origin locator `span_origin: MemberSpansOrigin` |
| `declaration_origin: Option<Arc<str>>` | fact field `declaration_origin: DeclarationOrigin` |

### `ProjectedIndexSignature` → `facts::ProjectedIndexSignatureFact`

| source field | disposition |
|---|---|
| `key_name: String` | fact field `key_name` |
| `key_type: TypeExpr` | fact field `key_type: KeyTypeShape` (declared key SHAPE, `[k:string]≠[k:number]`) |
| `value_type: TypeExpr` | body locator `value_type: TypeBodySlot` |
| `readonly: bool` | fact field `readonly` |
| `spans: IndexSignatureSpans` | origin locator `span_origin: IndexSignatureSpansOrigin` |
| `declaration_origin: Option<Arc<str>>` | fact field `declaration_origin: DeclarationOrigin` |

### `ProjectedSurface` → `facts::ProjectedSurfaceFact`

| source field | disposition |
|---|---|
| `members: Vec<ProjectedMember>` | fact field `members: Arc<[ProjectedMemberFact]>` |
| `call_signatures: Vec<TypeExpr>` | fact field `call_signatures: Arc<[FunctionSignatureFact]>` |
| `construct_signatures: Vec<TypeExpr>` | fact field `construct_signatures: Arc<[FunctionSignatureFact]>` |
| `index_signatures: Vec<ProjectedIndexSignature>` | fact field `index_signatures: Arc<[ProjectedIndexSignatureFact]>` |
| `has_index_signature: bool` | fact field `has_index_signature` |

---

## C. Prepared* (`verter_semantic/src/analysis/type_solver/prepared.rs`)

### `PreparedTypeDecl` → `facts::PreparedTypeBodyFacts` (+ siblings)

| source field | disposition |
|---|---|
| `root_identity: ResolvedRootIdentity` | carried as the fact's slot anchor (`ResolvedDeclSlotIdentity` at the session key) |
| `exported_name: Option<String>` | fact field (export-name metadata; retained on the prep-fact bundle) |
| `kind: TypeDeclKind` | fact field `PreparedTypeBodyFacts.classification: TypeBodyClass` |
| `type_parameters: Vec<TypeParam>` | `TypeParamDeclFact` / `NarrowTypeParam` (constraint/default → locators) |
| `body: TypeExpr` | body locator `PreparedTypeBodyFacts.body_slot: TypeBodySlot` |
| `merged_contributors: Vec<TypeExpr>` | body locators `PreparedTypeBodyFacts.merged_contributor_slots: Arc<[TypeBodySlot]>` |
| `member_index: FxHashMap<String, PreparedMember>` | `PreparedMemberFact` per member (see below) |
| `local_deps: Vec<String>` | fact field (same-file dependency names) |
| `external_deps: Vec<PreparedExternalDep>` | fact field (cross-file dependency edges — already `NoTypeExpr`) |
| `name_resolution: FxHashMap<String, ResolvedRootIdentity>` | fact field (pre-resolved name context; slot identities) |
| `provenance: DeclProvenance` | fact field (already `NoTypeExpr`) |
| `cache_deps: PreparedCacheDeps` | fact field (already `NoTypeExpr`) |
| `wrapper_shape: PreparedWrapperShape` | narrowed wrapper facts (see C wrapper table) |
| `projection_class: PreparedProjectionClass` | narrowed projection facts (`PreparedForwardPayloadFact`) |

### `PreparedMember` → `facts::PreparedMemberFact`

| source field | disposition |
|---|---|
| `ty: TypeExpr` | body locator `ty: TypeBodySlot` |
| `optional: bool` | fact field `optional` |
| `readonly: bool` | fact field `readonly` |
| `is_method: bool` | fact field `is_method` |
| `visibility: MemberVisibility` | fact field `visibility` |
| `spans: MemberSpans` | origin locator `span_origin: MemberSpansOrigin` |
| `declaration_origin: String` | fact field `declaration_origin: DeclarationOrigin` |

### `PreparedValueDecl` → `facts::ValueTypeAnnotationFact` / `FunctionSignatureFact` / `ObjectShapeFact` / `EnumMemberFact`

| source field | disposition |
|---|---|
| `root_identity: ResolvedRootIdentity` | fact slot anchor |
| `exported_name: Option<String>` | fact field |
| `kind: ValueDeclKind` | fact field (Const/Let/Var/Function/AsyncFunction/Class/Enum) |
| `type_annotation: Option<TypeExpr>` | `ValueTypeAnnotationFact` (+ `typeof_alias_target: Option<ValueDeclIdentityPart>` + annotation locator) |
| `signatures: Vec<FunctionSignature>` | `FunctionSignatureFact` (ordered overload group) |
| `object_shape: Option<ObjectExpr>` | `ObjectShapeFact` |
| `member_index: FxHashMap<String, PreparedValueMember>` | `PreparedValueMemberFact` per member |
| `enum_members: Option<Vec<(String, EnumMemberValue)>>` | `EnumMemberFact` (ordered name → closed scalar) |
| `external_deps: Vec<PreparedExternalDep>` | fact field |
| `name_resolution: FxHashMap<String, ResolvedRootIdentity>` | fact field |
| `cache_deps: PreparedCacheDeps` | fact field (already `NoTypeExpr`) |

### `PreparedValueMember` → `facts::PreparedValueMemberFact`

| source field | disposition |
|---|---|
| `ty: TypeExpr` | body locator `ty: TypeBodySlot` |
| `is_method: bool` | fact field `is_method` |

### `FunctionSignature` (`type_eval.rs`) → `facts::FunctionSignatureFact`

| source field | disposition |
|---|---|
| `parameters: Vec<FunctionParam>` | fact field `parameters: Arc<[FunctionParamFact]>` (each: name/optional/rest/has_ts_annotation facts + `ty` locator + `FunctionParamSpanOrigin`) |
| `return_type: Option<TypeExpr>` | body locator `return_ty: Option<TypeBodySlot>` |
| `type_parameters: Vec<TypeParam>` | fact field `type_parameters: Arc<[NarrowTypeParam]>` |
| `has_implementation_body: bool` | fact field `has_implementation_body` |
| (`FunctionExpr.spans: FunctionSpans`) | origin locator `spans_origin: FunctionSpansOrigin` |

### Wrapper / forward payloads

| source | disposition |
|---|---|
| `PreparedKeyFilterShape::Opaque(TypeExpr)` | `PreparedKeyFilterShapeFact::Opaque(TypeBodySlot)` (other arms 1:1) |
| `PreparedKeyRemapShape::Opaque(TypeExpr)` | `PreparedKeyRemapShapeFact::Opaque(TypeBodySlot)` (other arms 1:1, incl. `PreparedCaseTransformKind`) |
| `PreparedValueRuleShape::Transform(TypeExpr)` | `PreparedValueRuleShapeFact::Transform(TypeBodySlot)` (`PassThrough` 1:1) |
| `PreparedForwardPayload.target_name: String` | `PreparedForwardPayloadFact.target_name` |
| `PreparedForwardPayload.target_args: Vec<TypeExpr>` | `PreparedForwardPayloadFact.target_args: Arc<[TypeArgLocator]>` |
| `PreparedForwardPayload.forwarding_kind: PreparedForwardingKind` | `PreparedForwardPayloadFact.forwarding_kind` |

### `ObjectMember` variants (`verter_type_expr`) → `facts::ObjectMemberFact`

| variant | disposition |
|---|---|
| `Property(ObjectProperty)` | `ObjectMemberFact::Property(ObjectPropertyFact)` — name/optional/readonly/visibility facts + `ty` locator + `MemberSpansOrigin` |
| `Method(MethodSignature)` | `ObjectMemberFact::Method(ObjectMethodFact)` — name/optional/visibility facts + `FunctionSignatureFact` + `MemberSpansOrigin` |
| `CallSignature(FunctionExpr)` | `ObjectMemberFact::CallSignature(FunctionSignatureFact)` |
| `ConstructSignature(FunctionExpr)` | `ObjectMemberFact::ConstructSignature(FunctionSignatureFact)` |
| `IndexSignature(IndexSignature)` | `ObjectMemberFact::IndexSignature(IndexSignatureFact)` — key SHAPE + value locator + `key_name`/`readonly` facts + `IndexSignatureSpansOrigin` |

---

## D. Synthesized (`ResolvedLocalType`) → `facts::ResolvedLocalTypeFact`

Demo producer: `verter_semantic/src/analysis/fact_projection.rs`.

| source field | disposition |
|---|---|
| `name: String` | fact field `name` |
| `expanded: String` | carve-out (expanded type TEXT, display-only) |
| `type_expr: Option<TypeExpr>` | fact field `shape: ResolvedLocalShape` (object → `SynthesizedMemberFact` per member with `MemberSpansOrigin`; tuple → `TuplePayloadFact`/`TupleElementFact`; indexed access → `IndexedAccessFact`; leaf → `LeafTypeFact`; else shallow `Ref` locator) |
| `span: Span` | origin locator (the reference span is addressed by the enclosing macro payload locator; no top-level fact span field) |

Synthesized-(d) producer constants (documented, not stored): each synthesized
object member is `readonly = false`, `visibility = Public`.

---

## E. Svelte (`verter_semantic/src/analysis/framework_facts/svelte.rs`)

### `SvelteScriptFacts` → `facts::SvelteScriptFactsFact`

| source field | disposition |
|---|---|
| `props_type: Option<TypeExpr>` | body locator `props_type: Option<SymbolBodyLocator>` (shallow-by-default; bare `Ref` preserved) + `props_type_scope` |
| `bindable_members: Vec<String>` | fact field `bindable_members` |
| `prop_defaults: Vec<AnalyzedDefaultValue>` | carve-out (verbatim runtime default text) |
| `validated_snippet_members: Vec<String>` | fact field `validated_snippet_members` |
| `legacy_props: Vec<SvelteLegacyProp>` | fact field `legacy_props: Arc<[SvelteLegacyPropFact]>` |
| `dispatcher_events: Option<TypeExpr>` | body locator `dispatcher_events: Option<SymbolBodyLocator>` + `dispatcher_events_scope` |
| `instance_exports: Vec<String>` | fact field `instance_exports` |

### `SvelteLegacyProp` → `facts::SvelteLegacyPropFact`

| source field | disposition |
|---|---|
| `name: String` | fact field `name` |
| `has_default: bool` | fact field `has_default` |

### `SvelteScriptCandidates` / `SveltePropsCandidate` (parse-domain capture)

These are the content-addressed capture candidates (parse-domain). Their
persisted `TypeExpr` positions (`SveltePropsCandidate.props_type`,
`SvelteScriptCandidates.dispatcher_events`) narrow to the same body-locator +
scope-pairing form as `SvelteScriptFactsFact`; the remaining scalar fields
(`from_generic_argument`, `bindable_members`, `snippet_candidates`,
`instance_exports`, `module_exports`, `dispatcher_import_source`) are 1:1 fact
fields or provenance metadata. `prop_defaults` stays a display-only carve-out.
