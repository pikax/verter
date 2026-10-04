## Typed-IR-Only Resolver Rule (CRITICAL)

The native component-meta / typeinfo resolver pipeline drives every semantic decision from the typed IR (`TypeExpr` on the Rust side, `TypeDescriptor` from `@verter/type-ir` on the TS side). Source slicing, regex against type text, hand-rolled type-text splitters, `starts_with("Pick<")` shape sniffing, the synthesise-then-reparse pattern (`format!(...).parse_type_annotation(...)`), and `path.contains("/node_modules/")` classification are all forbidden inside that pipeline.

- OXC AST lowers to `TypeExpr` via `lower_ts_type(ts_type, source)` (in `verter_semantic::analysis::type_expr_lower`) at exactly two boundary classes: macro / JSDoc PRODUCER fields lower eagerly at their producer boundary (stored on `Analyzed*Field`, `ResolvedLocalType.type_expr`, surviving every cache); top-level DECLARATION BODIES lower LAZILY on first semantic demand through the scheduler-retained parse snapshot (`DeclBodyMemo` → `DeclLoweringService`), NOT eagerly during shallow analysis — an `IndexedReady` publish lowers ZERO declaration bodies. Either way the analyzer/lowering arm takes an OXC node it already holds; downstream stages walk the resulting `TypeExpr`, never re-parse or re-lower at query time.
- `parse_type_annotation` is reserved for JSDoc tag-type payloads (`{Type}` text inside `@type`/`@param`/`@returns`). Calling it from the resolver / projector / registry / policy / materialiser / compat pipeline is the bug.
- Raw display strings (`Analyzed*Field.type_annotation`, `ExpandedField.raw_type`, `ResolvedLocalType.expanded`, `PropMeta.rawType`) are display passthroughs only. Resolver and compat consumers MUST NOT parse them back into `TypeExpr` / `TypeDescriptor`.
- Workspace classification uses `RouteLookup::{workspace_is_package_backed, workspace_is_workspace_owned}` — structural predicates selected by the request source adapter. Substring checks on canonical paths (`"/node_modules/"`, `"\\node_modules\\"`) are banned. The classification API is path-agnostic and handles symlinked / pnpm-hoisted / Windows-backslash / workspace-linked-package cases.
- Hand-rolled type-text parsers must not exist inside the resolver. Walk `TypeExpr` nodes — `IndexedAccess`, `Ref { name: "Pick", type_arguments }`, `Union`, `Intersection`, etc. — directly via Rust pattern matching.
- The JS compat layer reads `prop.type` (`TypeDescriptor`) for every semantic decision. `prop.rawType` is display passthrough only. Operator splits use union/intersection tag matching on `TypeDescriptor`, not hand-rolled string operator parsers.

If a new requirement appears to need text manipulation inside the resolver, fix the producer (lower the right OXC node, store the right typed field, extend `@verter/type-ir` with a missing variant) rather than reparsing or pattern-matching on text. Architecture-guard tests in `crates/verter_session/tests/cases/architecture_guards.rs` and equivalents in `packages/component-meta` lock down this contract.

See `/component-meta` skill for the full producer-side schema (typed `*_expr` fields on `Analyzed*Field`, `ResolvedLocalType.type_expr` "always populated" invariant) and the post-cutover delete list.

### Typed Value Domain + Demand-Lattice Resolution (CRITICAL)

The U2 query-value-domain design (`.claude/skills/type-resolution/SKILL.md`) locks the typed value
domain and the demand-lattice that decides cache satisfaction/backfill. Resolution is typed end to
end; display is a projection; error and absorbing types ride existing carriers.

- **One key → one value arm.** Every `SemanticQueryKey` maps to exactly one `SemanticQueryValue` arm.
  No non-type value is smuggled into `GraphTypeNode`; the wire taxonomy stays a closed type taxonomy.
- **Demand lattice (presets, not enum order).** The five mode names (`Identity` / `Navigate` /
  `Shallow` / `Expanded` / `Skeleton`) are PRESETS over `(ProjectionDemand, EvalPolicy)`. Cache
  satisfaction and backfill are decided by lattice DOMINANCE over a RECORDED materialised
  `(path, point)` set — NOT by enum order, and NOT by a meet-derived nominal demand. `Skeleton` =
  `TypeParamShells` + carrier-stop; it is INCOMPARABLE to the expansion presets, a regime of its own.
- **Display is a projection.** Canonical display is computed at publish from the cached typed value,
  never a stored or re-parsed string. `display_needs` is display-only: it is masked OUT of every
  typed-value family key and never drives resolution. Two queries differing only in `display_needs`
  hit the SAME typed-value slot.
- **Error tolerance.** A result computed over torn / broken / mid-edit input is `ReturnOnly` and is
  never warm-admitted. A fact-rooted error (a recorded missing-dep fact) IS cacheable. `admit_decision`
  gates on the ROOTING FACT's presence in the `ReadSetSignature`, not on the taint class.
- **Error / any / never / unknown.** `unknown` = ⊤, `never` = ⊥; `any` is off-order (relates
  bidirectionally); `error` taints and rides the EXISTING `SemanticNodeData::Opaque(QueryError)`. NO
  new `GraphTypeNode::ErrorType` wire arm may be introduced — the wire-purity closure forbids it.
- **Planned STAGE-B guards (gap tracked here per the architecture-guard rule).** The discriminating
  behavioural guards land with STAGE-B: `cache_satisfaction_is_demand_lattice_not_enum_order` (U10),
  `cache_satisfaction_is_materialized_point_not_nominal_demand`,
  `display_needs_is_display_only_never_drives_resolution`,
  `error_tolerance_broken_input_is_returnonly_fact_rooted_error_is_cacheable`, and
  `error_any_never_propagation_lattice`. The design-gate guards landed NOW are
  `error_rides_opaque_no_new_error_type_wire_arm` and `u2_value_domain_design_doc_locks_invariants`
  (both in `crates/verter_session/tests/cases/g_block/u2_value_domain_design_guards.rs`).

