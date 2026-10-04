## Shallow File State and Frontier Engine

Cross-file type resolution for macros (`defineProps<T>()`, component-meta, etc.) is built on two shared primitives in `verter_session::resolver_core`:

**ShallowFileState** (`shallow_file_state.rs`) is the authoritative shallow symbol/export surface for one imported type file. Keyed by `(canonical_id, whole_hash)`. Contains:
- `exports` map (exported name -> `ExportTarget`: Local or Reexport)
- `wildcard_reexports` (`export * from` sources, in declaration order)
- `symbols` (slim locally-declared type headers and content-free member/type-parameter facts)
- `import_locals` / `import_targets` (import classification for closure)

Populated once through the shared host ensure-path and cached in `FileArtifactStore`. Invalidated when the file's whole-hash changes. The backend state retains its lazy body/dependency memo privately. Engine consumers receive `ShallowInputRecord` header projections through `IndexedInputs` and request demanded bodies and dependency edges through `OwnedLowering`.

**ExternalTypeFrontier** (`external_type_frontier.rs`) is the single BFS engine for all cross-file type deepening. Level-by-level traversal:
1. Seed with initial `(canonical_id, exported_name)` pairs
2. For each pending symbol: load `ShallowFileState` via `FrontierHost` trait, route the export (direct > alias > wildcard in declared order), run local closure
3. Collect `ExternalSymbolRef` entries from unresolved external deps into the next level
4. Dedup on `(canonical_id, exported_name)` across the entire request via `seen` set
5. Repeat until frontier is empty or budget is exceeded

**Assigned module values (`export = X`).** A module that assigns `export = X` exports X's members through the same route: the requested module's own `default` names X (TypeScript 7 always applies the interop), and any other name the member `X.name` a namespace merged into X declares; `export *` of such a module is the checker's TS2498 error and routes nothing. An import assignment (`import x = require("m")`) is an import route of its own form (`RouteImportForm::ImportEquals`, an `ImportTarget` whose `imported_name` is `export=`): it binds X itself, while a namespace import reads X's members and names X by `default` only when X is a function or class. A declaration-file module without an export declaration exports every top-level declaration, written `export` or not, and an ambient namespace body without one exports every member.

**Barrel BFS contract:** when a pending file is a barrel and the requested symbol is not found in that file's shallow surface, process that file's wildcard barrel children as one BFS layer. Shallow every child in the layer before choosing any deeper barrel grandchildren. A barrel child that does not expose the requested symbol at its own shallow surface may contribute its wildcard children to the next layer, but must not trigger immediate depth-first descent.

**Local closure** (`ShallowFileState::local_closure()`) resolves same-file transitive deps iteratively. Uses a visited set for cycle handling (revisited nodes silently skipped). Never crosses import boundaries -- external deps become `ExternalSymbolRef` for the frontier.

**Budget contract** -- three domains with high ceilings (safety rails, not normal control flow):
- `local_closure_steps`: 500 (same-file symbols per closure)
- `frontier_symbol_visits`: 2000 (cross-file `(canonical_id, exported_name)` pairs)
- `builder_expansion_steps`: 5000 (symbolic expansion steps)

When a budget trips, the system returns a structured `BudgetExceededFailure` with domain, limit, actual count, and context -- never silently normalizes.

**Host integration:** production route resolution enters through the
request-bound `RouteLookup` port. The private source request driver owns the
routed-target walk; passive `ImportedRootDb` and `RouteDb` slots retain its
answers. Current indexed shallow facts resolve direct, named-reexport and
wildcard-barrel hops. Terminal semantic projection starts
from that routed declaration and executes through `ProjectSemanticDispatch`.
There is no host adapter that expands parser elements and no second frontier
after `ImportedRootDb` selects the target.

**Key files:**

| File | Purpose |
| --- | --- |
| `crates/verter_session/src/resolver_core/shallow_file_state.rs` | ShallowFileState, ExportTarget, ShallowTypeSymbol, ExternalSymbolRef, ResolutionBudgets, local_closure() |
| `crates/verter_session/src/resolver_core/external_type_frontier.rs` | ExternalTypeFrontier, FrontierHost trait, PendingExternalSymbol, ResolvedSymbol, RouteKind |
| `crates/verter_session/src/host_resolve/external_type_resolution.rs` | Routed component-meta declaration and native projection entry points |
| `crates/verter_session/src/host_resolve/frontier_engine.rs` | Named-export routing and route/index fact production |
| `crates/verter_session/src/frontier_tests.rs` | Behavioral invariant tests (diamond dedup, barrel ordering, cycle termination, budget enforcement, etc.) |

