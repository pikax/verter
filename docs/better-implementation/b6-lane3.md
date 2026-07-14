# Better-implementation ledger — B6 / Lane 3

Deferred better-implementation work surfaced by the Lane-3 review of the
emit-payload/native-eval landing. One section per item; each records what
landed, the better approach, why it was not adopted in the landing, and the
owning follow-up. This ledger persists past B8 — remove a section only when
its follow-up lands.

## U14 compat-demand parity — concrete compat display via one native consumer-demand projection

- **Block/Lane:** B6 / Lane 3 (native-eval verification).
- **Landed commit(s):** `4803e2a11` (native structured carriers pinned in
  `packages/component-meta/src/native-eval.spec.ts`), plus the Lane-3 close
  commit that removed the deferred `it.todo` arms in favour of this row.
- **What landed:** the native structured surface (`meta._verter.props[].type`)
  publishes path-precise concrete terminals and shallow structured carriers,
  and the spec pins those exactly; the compat (Volar-interop) DISPLAY string
  (`meta.props[].type`, `meta.slots[].type`) still renders the authored
  symbolic form for shallow carriers.
- **Better approach:** ONE native consumer-demand projection: the compat layer
  requests concrete materialization for a carrier through the existing shared
  dispatch (a demand-scoped raise of the published `SemanticTypeSource` /
  graph handle), then renders the display from the returned typed descriptor —
  never a TS-side re-resolution, never an eager publication-time expansion.
- **Why not adopted now:** publication is shallow by design (Component-Meta
  Shallow-By-Default rule); the consumer-demand projection is a new demand
  surface with its own budget/caching contract, out of scope for the Lane-3
  fix cycle.
- **Source:** native-eval deferral (all three Lane-3 review legs touched it);
  probe evidence 2026-07-11: eight deferred display arms fail standalone —
  compat displays render `Button["variants"]["color"] | undefined`,
  `AvatarProps | undefined`, `{ ui: ComponentConfig<typeof theme>["ui"]; }`,
  `Tabs["variants"]["color"] | undefined` instead of concrete
  unions/objects; the type-registry entries carry no `rawType` source text
  and no resolved declaration metadata (`canonicalSource`/`span`/`kind`).
  The removed deferred arms (former `it.todo`s in `native-eval.spec.ts`):
  1. concrete chained indexed-access unions on the compat display
     (`activeColor` contains `primary`, `ui` contains `root`);
  2. type-registry `rawType` source text + resolved declaration metadata;
  3. concrete mapped-helper surfaces on the compat display;
  4. concrete unions for opaque-sibling registry refs on the compat display;
  5. imported component-config surfaces rendered concretely (`neutral`
     app-config arm included);
  6. transitive imported registry aliases materialized for compat display
     and schema;
  7. imported slot-binding indexed-access helpers materialized for compat
     display;
  8. realistic generic tabs helper routes rendered concretely.
- **Impact if adopted:** vue-component-meta display parity for shallow
  carriers (concrete unions/objects in `prop.type` / `slot.type` and richer
  compat schemas) without weakening shallow publication.
- **Owning follow-up:** U14 (compat-demand parity), `@verter/component-meta`
  compat layer + the native demand entry it consumes.

## defineModel default-VALUE extraction gap

- **CLOSED:** the analyzer now extracts the authored defineModel default
  value (`extract_define_model_default_values` in
  `crates/verter_semantic/src/analysis/macros.rs`, mirroring the
  `withDefaults` value extraction) and threads it through the synthesized
  model prop (`synthesize_model_prop_and_event` derives `has_default` and
  `default_value` from the same lookup, so they cannot diverge); the native
  descriptor publishes `defaultValue` and compat `prop.default` carries the
  authored text, pinned by `native-eval.spec.ts` ("evaluates defineModel
  types") for both typed and untyped defineModel forms.

## LSP true-cross-binding byte-diff test

- **Block/Lane:** B6 / Lane 3 (review finding, claude P3).
- **Landed commit(s):** existing wire-equivalence coverage in
  `crates/verter_lsp/tests/cases/lsp_component_meta_wire_equivalence.rs`
  (LSP JSON vs `verter_ffi` projection at the decoded-DTO level, plus two
  same-process protobuf encodes compared byte-wise).
- **What landed:** the D19 equivalence is asserted between the LSP custom
  method and the FFI projection inside ONE process; the byte-level compare
  covers two drives of the same in-process encoder.
- **Better approach:** a TRUE cross-binding byte diff: produce the encoded
  component-meta payload through the NAPI binding and through the LSP wire
  route for the same fixture and byte-compare the two artifacts (with EOL /
  encoding normalization per the portability rules), so an encoder divergence
  between bindings cannot hide behind the shared in-process code path.
- **Why not adopted now:** requires a harness that runs the built native
  binding and the LSP server side by side; the Lane-3 fix cycle scoped to the
  audited-record and emit-source defects.
- **Source:** claude review leg, P3.
- **Impact if adopted:** catches cross-binding encoder drift (NAPI vs LSP)
  at the byte level rather than the decoded-DTO level.
- **Owning follow-up:** LSP integration harness + `@verter/native` test lane.

## Transitional `dep_signature` / `result_is_partial` fold

- **Block/Lane:** B6 / Lane 3 (review finding, claude P3).
- **What landed:** the component-meta result path still carries the
  transitional pairing of the legacy `dep_signature` alongside the
  fact-signature rail, and folds `result_is_partial` into admission at the
  call sites rather than through one typed completeness carrier.
- **Better approach:** collapse onto the single `ReadSetSignature.facts`
  validity rail end-to-end and carry completeness as one typed
  admission-gating value threaded through the publish path, deleting the
  residual legacy signature plumbing in the same cutover.
- **Why not adopted now:** the fold crosses the warm-admission contract for
  several caches at once; correct as-is, and the cutover belongs to the
  cache-rail owner change, not a fix cycle.
- **Source:** claude review leg, P3.
- **Impact if adopted:** one fewer parallel validity signal (less drift risk
  between the legacy signature and the facts rail), simpler admission logic.
- **Owning follow-up:** `verter_session` cache-rail owner
  (`ComponentMetaResultDb` publish path).
