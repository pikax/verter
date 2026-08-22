# TCM0 §9 — Deletion closure

Scope: charter item 9. Name every mechanism TCM4 deletes and every generic facility that survives with a
proven owner. Not deferred to TCM4 — the naming happens here.

## Deleted (named now, executed only in TCM4)

| Mechanism | Location | Why it's deleted | Superseded by |
|---|---|---|---|
| The relay half of `open_file`/`load_file`/`update_file`/`close_file` for `TypeScriptLspDirect`-owned features | `crates/verter_type_runtime/src/tsgo/ipc.rs`, `tsserver/ipc.rs` (the LSP-relay call paths) | Once TypeScript talks to the editor directly through the content mapper, Verter's role as a relay proxying these calls to answer plain-TS questions has no reason to exist | Editor's own `didOpen`/`didChange`/`didClose` driving the mapper's `transform()` directly (`feature-ownership-ledger.md` rows #2-5) |
| The TS-diagnostic merge half of `type_provider/merge/diagnostics.rs` (`merge_diagnostics`, `same_mapped_diagnostic` for the compiler-diagnostic class specifically) | `crates/verter_lsp/src/type_provider/merge/diagnostics.rs` | TypeScript reports compiler diagnostics against the mapped file directly, position-mapped via its own `SpanMap`/`DiagnosticDirectives` — no Verter-side merge step for THIS class | `diagnostic-ownership-matrix.md`'s compiler/checker row |
| The relay round-trip for every `TypeScriptLspDirect`-owned interactive feature (hover/completion/definition/type-definition/references/signature-help/document-highlights/inlay-hints, plain-TS subset of code-actions/semantic-tokens) | `crates/verter_lsp/src/server/nav_features*.rs`, `aux_features.rs` (the relay-calling halves only — the merge/attribution logic for Verter-owned overlays survives) | Same reasoning — TypeScript answers these directly against the mapped file | `feature-ownership-ledger.md` rows #6,9,12-14,16,18-20 |
| The `verter-tsc` CLI's independent compiler-diagnostic implementation, IF it converges onto the same oracle-session path the LSP now uses (see open item in `diagnostic-ownership-matrix.md`) | `crates/verter_tsc/src/api_check.rs` | Two independent implementations of "get TS diagnostics" is exactly the kind of duplicate-engine outcome the Shared Optimized Codebase rule forbids, once one path can serve both | The oracle-session (`VerterWithTypeSemanticOracle`) query, used by both the LSP live path and the CLI batch tool |
| The current string-encoded `source_projection_map()`/`PositionMapper::from_json` double-parse path | `crates/verter_compiler/src/code_transform/source_map.rs` (the `_json` variants as the ONLY typed-to-string exit), `crates/verter_lsp/src/documents/position_map.rs:142,200-215` (`PreambleEnvelope` struct + the `from_json` double-parse that consumes it) | Superseded by the typed `SourceProjectionMap`, per `mapping-products-string-surface.md` | TCM1's typed product |
| `get_diagnostics_background` | `crates/verter_type_runtime/src/traits.rs:475-477` and all wrapper forwards | Confirmed zero non-test, non-wrapper call sites — already-dead code, not a capability removal requiring governance ratification | n/a — nothing to supersede, it answers no live consumer today |

## Survives, with a proven owner (not silently kept alive by omission)

| Facility | Owner | Why it survives |
|---|---|---|
| `VerterWithTypeSemanticOracle`-owned methods (component-meta type queries, cross-region rename, carrier-lifecycle notifications, workspace-folder sync) | `feature-ownership-ledger.md` rows #3,7,15,24,27,28 | These need Verter-specific knowledge the mapped file alone cannot express — the content-mapper protocol has no field for them, so the oracle-session client (the SAME `API`/`Snapshot` class inspected live in this investigation) remains the correct mechanism, just no longer wrapped in a relay for the OTHER (TypeScript-native) features |
| `VerterNative`-owned methods and all lint/parse/CSS/directive-syntax diagnostics | `feature-ownership-ledger.md` rows #1,8,10,22,29,30; `diagnostic-ownership-matrix.md`'s directive/framework/custom-block/style rows | Never touched TypeScript; nothing about this program changes that |
| The `Fragment`/`SourceUnit` intra-file assembly system | `crates/verter_compiler/src/assembly/{fragment,source_unit}.rs` | This is the machinery that PRODUCES the content mapper's `transform()` output — it becomes MORE load-bearing, not less |
| `PlacementMap`/`RuntimeSourceMapData`/`EncodedSourceMap` as distinct concepts | TCM1 | Kept distinct per the amendment's explicit rule; only their representation (string → typed) changes |
| `ProjectTypeStore` and the rest of Verter's own type-resolution engine | unchanged | Disjoint concern from the TypeScript-contract program — this investigation finds no reason for TCM1-TCM4 to touch it |
| The External-Source Decision Table's `ExternalSourceRequest`/`ExternalBlockKind` dependency-tracking mechanism | `crates/verter_session/src/types.rs:1643-1678` | Still needed for dependency-edge bookkeeping even where content itself becomes TS-owned (`external-source-decision-table.md` row #6) |

## Named but NOT yet dispositioned (the two governance-pending ledger rows)

`register_carrier_member`/`register_carrier_metadata`/`activate_carrier_member(s)`
(`feature-ownership-ledger.md` rows #25-26) are named here explicitly as candidates whose deletion (if
ratified) would be justified by the content-mapper protocol's own `virtualFileName`/
`canonicalSourceFileName`/`supplementalSourceFileNames` fields already carrying this identity on the
wire — but TCM0 does not have authority to rule on this, per the charter's ban on "an intentional
capability removal without explicit governance approval." TCM4 may only delete these once a maintainer
ruling closes this row; until then they remain live code with a `CANDIDATE` marker, not orphaned and not
deleted by omission.
