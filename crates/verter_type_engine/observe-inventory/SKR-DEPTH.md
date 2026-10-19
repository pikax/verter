# Iterative openness and carrier-presentation inventory

Classification follows [the observation policy](../../../docs/arch/semantic-observe.md).

## verter_type_engine

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| `Openness.frames` (`Frame`: memo settle, disjunction over remaining children, walk end, in-flight release, escape classification, conditional / mapped / instantiation-argument / carrier-head / recipe / indexed-access-index / default-verification continuations) | `Openness::run` resumes the frame below with each answer, so a key-domain or closedness evaluation runs at any depth on a constant native stack; `Drop` releases the in-flight entries an abandoned evaluation still holds | REQUIRED | One evaluation; empty when it answers | `src/project_semantic_dispatch/raise/openness.rs` | always |
| `Openness.walks` (`OpenWalk`: role, bound binders, role/position-keyed `memo` and `in_flight`) | `Openness::visit` answers revisits and back edges from the innermost walk; a nested mapped operand or recipe escape opens its own walk | REQUIRED | One walk scope, opened by a predicate, a mapped operand or an escape and ended with its answer | `src/project_semantic_dispatch/raise/openness.rs` | always |
| `Openness.budget` (replaces `OpenWalk.budget` and the per-call `&mut u32` threading) | Each visit, proof, recipe and escape charges it; exhaustion answers undecidable (open key domain, closed outer-generic question, `Unavailable` proof) | REQUIRED-budget | One evaluation; written back to a direct proof's caller | `src/project_semantic_dispatch/raise/openness.rs` | always |
| `RecipeEnv` (prepared declaration and `ParamBindings`, one slot per declared type parameter) | The recipe arms read the live binding of `ParamRef` / `FollowRefByName`; escapes lower under it; default verification fills it in declaration order | REQUIRED | One declaration or instantiation proof | `src/project_semantic_dispatch/raise/openness.rs` | always |
| `ProjectSemanticDispatch.closedness_active` (now an insertion-ordered `IndexSet` instead of a scanned `SmallVec`) | `push_closedness_active` refuses a declaration already being proven on the chain at constant cost per hop; `pop_closedness_active` releases the latest entry | REQUIRED | Each entry from its proof's start to its answer; empty when no proof runs | `src/project_semantic_dispatch/mod.rs` | always |
| Carrier-presentation stack (`Presenting`: surface node, view, member values presented so far) and its once-per-terminal visited set (replaces the depth-8 recursive presenter) | `present_terminal_member_carriers` presents a component member as its instance-identity carrier at every level of an `Expanded` terminal's inline structure | REQUIRED | One terminal presentation | `src/project_semantic_dispatch/walk.rs` | always |

The `OPENNESS_NESTING` thread-local and its 256-level re-entry guard are removed: the explicit work stack replaces the native re-entry they bounded. No optional counter, trace or history is added; the depth tests read verdicts and published surfaces, not visit or depth telemetry.
