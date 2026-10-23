**Class surfaces (`ResolveClassSurface` / `ResolveOverloadSet`).** A class resolves through two composed surfaces on the shared dispatch: the INSTANCE side (own members ∪ type-side heritage with dual-space substitution composition, visibility carried, `#private` excluded from the published keyspace) and the STATIC side (the `typeof C` constructor-object: own statics + own construct signature from the producer's `ValueDeclInfo.object_shape` — statics ride INSIDE that ObjectExpr, never a separate field — plus base statics composed via the type-side heritage refs, own-shadows-base precedence; a ctor-less subclass inherits the base ctor params with the derived instance return). the static side carries the `prototype` property every class constructor type has (the instance with `any` for each type parameter, first among its members), so `keyof`, indexed access and relations read it like any other property; a class expression's constructor carries it too. Utility arms select by signature-kind BUCKET: `Parameters`/`ReturnType` read the call bucket, `ConstructorParameters`/`InstanceType` the construct bucket; class-ref payloads obtain the bucket via `ResolveClassSurface(Static)`; a bare-generic `ReturnType` instantiates at `unknown`. `ResolveOverloadSet` is a LIVE producer (`build_resolve_overload_set`): it settles the callee through the ONE shared signature-source carrier rail (`resolve_signature_source_carrier`, the same demand point the signature utilities use; the rails share the FUNCTION, not the CONTEXT — the utilities pass their caller's context while this key, being mode-erased, always settles under the non-published `StructuralTransit` context — and a `DeclRef`/`InstantiationRef` carrier callee still settles to the same signature group on both rails because `Instantiate` of a signature-bearing decl is mode-stable) and produces the ordered VISIBLE signatures of ONE requested bucket (the key's `kind`: call or construct — a call site demands exactly its own bucket, the other bucket is never read, so its settlement never gates the set) as the `OverloadSet(Arc<[SignatureRef]>)` value domain (the boundary `execute` wrap converts the group-bearing node; `execute_type_node` reports `ValueDomainMismatch` instead of leaking it as a type node). Visibility is `build_typeof`'s rule applied where the callee node was produced (a lone signature is visible even if bodied; a multi-signature group hides the trailing implementation), so the set's LAST element is the last visible overload — the signature-utility selection rule. Explicit `type_args` instantiate per candidate (non-accepting candidates drop; all-dropped ⇒ honest `Miss`); a bucket with no signature is an honest `Miss`, never an empty set. `ResolveCall` classifies a missing set through a typed acquisition outcome read from the SAME shared list (`CallCandidates`: complete-empty ⇒ `NotCallable`; candidates present but every one dropped by the explicit type arguments ⇒ `NoApplicableOverload`, the callee stays callable; unsettled ⇒ `Budget`/`Undecidable`) — never by re-reading the raw list's mere settlement. `typeof C.make<string>` instantiation-expression args lower onto `ValueRef.type_args` and instantiate the resolved node at the typeof hop. Decorators never rewrite a surface — projection ignores them. Heritage hops run at the caller's demand (shallow-by-default); the facts rail records one `FileWholeHash` per heritage contributor.


**Per-file class index (`FunctionProgramIndex::classes`).** Every class a file authors — a declaration at any depth or a class expression — is recorded by the function-program discovery walk and sealed into one `ClassIndex` (`verter_session_query::function_program::class_index`) owned by, and released with, the file's `FunctionProgramIndex`. Collection adds no walk of its own: the hash fold that discovery already runs over each served body records the classes it meets (`function_program_hash.rs`), and discovery's own walk of the syntax outside every served function (`function_program_discovery_walk.rs`, `DiscoveryWalk`) records the rest. That walk enters each statement once — at a top-level statement or a namespace member it first discovers the statement's served positions, then walks the statement's statements and expressions, skipping every served function and served field initializer (the hash fold's) — so there is no separate walk of the program for classes (`discovery_enters_each_statement_outside_served_functions_once`): a class declaration, an exported or default-exported class, a class expression a variable, default export, call argument or array holds, and a class in a block, namespace body, static block or field initializer are all indexed. Each record carries the class span, whether it is an expression, whether it has an `extends` clause, and the span of each member its body declares directly (each class element, and each property-declaring constructor parameter). Sealing (`ClassIndex::from_discovery`) sorts the records into source order and keys them: member span → declaring class (`declaring_member`), class span → class (`at`), and each class's nearest lexically enclosing class (`is_enclosed`) — every read is a keyed lookup, never a scan of the classes or their members. The protected-member rule reads a member's declaring class through `declaring_member` (`relation.rs` `member_owner`): a file-scope class by its declaration identity (the class-heritage ancestry authority reads its bases), any other class by its node with the base the index resolved.

**Lexical class bases (`ClassBase`).** When discovery seals, each class's `extends` clause is resolved lexically — in the frame the class is written in, then the frames around it (the same binding inventories and lexical scopes capture resolution reads), then the file. It names a class this file authors (`ClassBase::Class`) only when the binding reached is a class declaration's name or a `const` a class expression initializes. A parameter, `let`/`var`, unmodelled local or any other binding shadowing the name; a namespace member; a class in a block, class body or namespace outside every function (indexed, but its base is left unresolved); a class inside a function no entry serves (a local class declaration's method); a circular chain; and any clause that is not a bare name (a mixin call, a member access) all leave it `Unresolved`. A non-file-scope class derives from another exactly when its resolved base chain reaches it; an unresolved base keeps the pair undecided.

**Function-local class declarations.** A read of a class the frame declares (`class L {}` then `new L()` / `L`) lowers the declaration as the class value a class expression of the same name is (`flow_slice_content.rs` `lower_local_class_declaration`), found by its name span through the same name-pruned descent the local function lookup uses. A class an enclosing frame declares is not read this way (its body names that frame's bindings). A local class declaration's methods are not served by any entry, so a bodied method without a return annotation is an unmodelled member, never its signature with an `any` return. The flow proof still reads a class declaration's capture set as the Capture family's accepted typed gap, so a result carrying a local class value is never admitted warm and a publication read through the proof (a type alias of the return) stays unverified.

**Class-expression instances in relations.** A class expression's instance relates through its surface in every position: the inline element relation (array/tuple elements) routes a `ClassExpressionInstance` pair through the framed relation exactly as it does a declaration carrier (`relation_eval_requires_canonical_frame`).

**Member-granular class-body lowering.** A member read (`this.x`, a class-instance reference's member) never lowers the class body whole. `class_member_source` derefs the class declaration's body through two first-step locators: `TypeBodyPathStep::ClassMembersNamed { name_hash }` (every overload, accessor, field and constructor parameter property of the name, selected from the retained parse by `class_member_name_hash`; a colliding name selects its colliding members too and the reader still matches by key) and `TypeBodyPathStep::ClassHeritage` (the `extends` reference). The deref (`DeclBodyMemo::transient_class_body_selection`, lease-only) lowers only the selected class elements (`lower_class_body_selection`; a skipped method still counts toward its name's overload ordinal), so a demand's lowering work depends on the elements declaring the demanded name, not on the class's other members (`class_owner_tests::a_member_read_through_this_lowers_only_the_elements_declaring_it`, over `DeclLoweringCounters::class_elements_lowered`). Only a class one top-level statement declares serves this route; a merged or namespaced class is no member source. The declaration-level fact lowering (`lower_demanded`, once per class symbol) is unchanged.

**Member-granular overload-group lowering.** A `ValueSignature { ordinal }` deref of a file-scope, value-only declaration (`DeclBodyMemo::transient_value_signature_in`) lowers only the statement declaring that overload: the stored signature fact names its statement (span-origin contributor anchor) and one statement's signatures are contiguous in group order, so the member's in-statement position is the run of same-statement facts before it. Dereferencing every member of an n-overload group therefore lowers n statements, not n² (`overload_scale_tests`, over `DeclLoweringCounters::decl_statements_lowered`). A dual-space name, a synthesized signature or an unsettled fact record falls back to the whole-group re-borrow.

**Inherited member reads (`class_member_source`).** A member read that the class's own body does not declare follows the demanded name up the `extends` chain one class at a time: the `extends` arm lowers to its lazy reference, and when that reference is a class DECLARATION (`DeclRef` / `InstantiationRef`, the clause's type arguments applied) the read follows the base — iteratively, so a chain of any depth is walked without native recursion and never escapes through a whole-class read. The first class on the path declaring the name answers, so an override or an own redeclaration stops the lookup; the answer is that class's own member object, scoped to and stamped with the DECLARING class's file (`declaration_origin`), with the member's visibility, accessor kind and instance/static side as its own body records them (`ClassMembersNamed` selects instance elements only; static reads stay on the value route). No base ever lowers whole: a read's work depends on the classes it walks and the elements declaring the name, never on the bases' other members (`class_inherited_member_tests::an_inherited_member_read_lowers_no_unrelated_base_member`, and past three hundred classes `a_chain_past_three_hundred_classes_lowers_no_unrelated_base_member`). The read falls back to the `extends` arm read whole — the merged declaration — when a class on the path is one another module augments (`declare module "./c" { interface C {…} }`, checked through the augmentation index, whose population facts root the result), is a same-file merge the selective deref cannot serve, has a value-only base (a mixin call, a member access), or closes a circular chain. Identity ownership: the reading class owns the demand; each class on the path owns its member positions and its `extends` clause; the declaring class owns the answer's scope and origin. Invalidation uses the selected-member ingress below when its exact dependency proof is available; unsupported reads retain whole-content evidence. A selected member never permits skipping potentially affecting merged contributors or diagnostics.

**Exact member cutoff boundary.** `precise_class_member_read` stops only the remainder of the member-read lowering after the source-owned narrow port has proved its dependencies: declaration scope payload construction, per-member `LowerLocator` queries and source-file-scoped answer nodes. It requires an ordinary-file, single-statement class, a nonempty selection, supported member types and the exact served content's non-absent `Member` observation. Every declaring element and the header binder frame participate. The existing
Display lane additionally proves graph-preserved names and tuple labels through
the same bounded walker; semantic alpha-normalization stays unchanged. The shared lowerer produces the answer with the same optionality, visibility and member role.

Later calls, affecting writes, heritage/static evaluation effects and their refusals retain their existing execution and evidence. No merged/augmented contributor suffix or class-expression evaluation suffix is dropped solely because an own member is precise. A refusal never certifies an exact member or loses its profile/scope through delivery. `class_member_evidence_tests` compare cold, warm and edited answers and diagnostics to fresh controls, including a later call's TS2345 and an affecting write; `class_evaluation_flow_tests` retain evaluation-order and partial-result controls.

**Const-held class expression bases.** `lower_class_expression_value` reads a class expression's `extends` base as `InstanceType<typeof Base>` inside the instance it embeds in the construct signature's return, so a base that names only a value (another `const`-held class expression) composes its instance beneath the class's own members; `CE2` to `PR` decides `"n"` (`a_class_expression_extending_a_class_expression_is_not_an_unrelated_class`).

**Superseding note for the signature-kernel difference ledger (`SDL-79`).** Its "Open" items have moved: a class expression extending a class expression now decides `CE2`/`CE3` to `CE1` (`"y"`), `CE1` to `CE2` (`"n"`) and `CE2` to `PR` (`"n"`). `class G extends Tagged(TB)` decides (`a_class_extending_a_mixin_application_derives_from_its_base`). Function-local classes evaluate: their probes read through `ReturnType<typeof locals>[i]` and `ReturnType<typeof loc>` equal the checker's answers, while the same probes through `type L = ReturnType<typeof locals>` stay partial until the flow proof enumerates a local class's capture set (`a_function_local_class_read_through_an_alias_declares_its_own_members`, ignored).

**Imported declaration existence.** An authored local export specifier is only
routing evidence. Both the imported-root shortcut and the named export route
walker must establish that the selected lexical owner declares the leaf binding
before reporting a resolved identity. The indexed paths use
`ShallowInputAssembly::declares_local_binding_in` so
namespace blocks, namespace aliases and effective framework symbols retain
their own declaration inventories. The source-header shortcut
requires a unique matching export whose identifier lies inside the selected
declaration's SFC-absolute span; a separate `export { B }` statement must use the
canonical shallow route table to choose its owner. A same-named declaration in
another script region cannot satisfy an unresolved local export. Direct inline declaration exports carry the content-free source-header proof.
Local export lists, aliases and reexports retain whole-content owner/existence
coverage where an equivalent narrow binding proof has not been established.


**Selected member ingress and content ownership.** `OwnedLowering::class_member_input`
serves the existing retained `ClassMembersNamed` selector directly, before a
broad prepared declaration or indexed input escapes. Its required `Member`
observation covers all selected elements, their order and the heritage clause.
Generic class headers frame that hash through the shared semantic signature
hasher, so a new type parameter cannot silently shadow an imported reference.
The source route fact and workspace resolution witnesses cover imported heads.
Only ordinary-file owners are eligible; unresolved, qualified, namespace,
same-file and class-parameter references retain the general content path.
The shared lowerer accepts closed property types and explicitly annotated
monomorphic methods/getters with no body-derived return, own generic clause or
type predicate. Method signatures must be reference-free and form one selected
method; overload groups and getter/setter pairs keep the general path. Closed
reusable signatures omit source coordinates while retaining optionality and
rest flags; imported reference heads are supported only in properties. It preserves method kind, implementation-body flag, optionality,
visibility and declaring origin. Child declaration references keep their normal
target identity and scope. Unsupported accessors and body-derived signatures
continue to read whole content.

Before using a selected own member, the engine observes the module-augmentation
population and retains the general path when another contributor affects the
class. Script-global classes additionally require the exact single contributor
population. Ordinary per-symbol population fingerprints describe membership,
order, owner and symbol space; member headers and bodies are consumed-content
evidence. Target-wide augmentation fingerprints likewise contain ordered
canonical and matching binding membership (specifier, exact owner, name, symbol
space and declaring-file kind), excluding member headers and body versions.
The exact current augmentation-artifact port observes whole served content
before exposing either a contribution or its absence. Session overlay authority
is selected before target matching, including empty overlays; known unrelated
noncontributors are not materialized by target discovery.

Broad indexed/shallow input lease serves and reacquisition, generic declaration
and body reads, and retained flow-demand consumption observe the exact served
snapshot. Prepared bundles retain their producing route witnesses and replay
them with the observed owner hash on every consumption, including committed
slots and memo/shared/follower hits. The narrow file-availability probe exposes
no content and records its exact served syntactic-route fact. Request-bound validation
materializes current parse facts into its existing completion overlay before
capturing the validation epoch; tests and callers must not pre-index edited
dependencies merely to make member evidence validate.

A later nongeneric empty interface contribution is neutral only when the
recorded base proof establishes an ordinary-file nongeneric class and the
retained typed body proves no members, signatures or heritage. The fold omits
that contribution's graph lowering while retaining population, whole-content
and source-environment observations. Captured graph-work controls distinguish
sixteen empty suffixes from sixteen nonempty ones. Every unproved contributor
and class-expression evaluation step retains its existing evaluator, including
conflicting declarations, calls, heritage, static effects, affecting writes
and typed refusal demand/profile/scope.

Source-header direct export proof validates the captured or request-completed
source incarnation and generation as well as its content hash. Completion pairs
that scalar identity with the hash in its existing canonical map; it never
adopts an unrelated later completion's source root. A missing payload or a
session-shadowed canonical fails this shortcut closed. Direct routes carry
header existence proof; content ports own the member or whole-content evidence.

Target-population validation first materializes the current set through the
same augmentation owner as cold evaluation and requires its exact published
lease. Reuse requires agreement with both that current proof and the captured
root. If a differing set is rebuilt after capture, the old view conservatively
refuses admission until a fresh view; this is never counted as reuse or as an
exact performance success.
