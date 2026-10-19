## Generic Navigation And Expansion Contract

Generic substitutions are part of semantic meaning and therefore part of semantic query identity. Navigation and expansion operate on instantiated meaning, not on the raw generic declaration body alone.

- Query keys for reusable semantic work must include the relevant type arguments or substitution environment.
- Member projection and indexed access against generic aliases, instantiated types, or mapped helpers must apply substitutions before deciding what member or keyspace is visible.
- Mapped values and name remaps remain parameterized through locator-view projection and reducing lowering. Outer arguments substitute into the shape first; the per-key materializer then binds the mapped parameter before evaluating helpers, conditionals, or template holes. Projecting a recursive helper while its mapped key is still unbound can replace the application with a recursion sentinel and lose its arguments.
- An empty shallow projection of an infer or type-parameter source does not prove an empty homomorphic key domain. Preserve the mapped carrier unless the source is directly empty or its constraint enumerates successfully, so reverse-inference holes retain their binder identity. Optional mapped property reads include `undefined` under `strictNullChecks`, including with `exactOptionalPropertyTypes`; exact optionality governs writes rather than absence on reads. Removing optionality reads the authored member value before absence is added, retaining explicitly written `undefined` in exact mode.
- A mapped index distributes over the selected key domain through the shared indexed-access query. Intersection key domains use the shared key-domain reader: finite arms intersect by typed key identity, and primitive `string`, `number`, or `symbol` arms filter key kinds. Index signatures contribute their broad key kinds beside named members; homomorphic materialization retains a finite-domain proof from the shallow source or constraint before using source members, since a named-member list alone never proves a finite or empty domain. A named member of a homomorphic dictionary can be admitted from its shallow source surface without forcing sibling values. A proven-empty index reduces to `never`, including every following index hop. Admitted nominal and broad keys, and an index equal to a closed template pattern, bind the same per-key mapped value materializer as literal keys. Open template interpolants retain their deferred access. Unique-symbol indices retain their nominal `typeof` form at the wire boundary.
- An open key-domain arm preserves the deferred access. A mapped carrier or intermediate indexed access that stalls before consuming a path retains the full remaining indexed-access structure and alias arguments; the mapping itself is never the answer to its pending index.
- Structural openness checks use an ordered worklist for nested surfaces and operators. Each node retains its role/operand-position memo identity, in-flight cycles remain closed only for that revisit, and the first open child stops sibling descent. Tuple rest elements keep their key-domain position; object index keys remain visible even when member values are not demanded. The shared walk budget and scoped binder rules still govern declaration and generic leaves; the declaration, recipe, mapped and instantiation leaves continue on the same explicit work stack (`raise/openness.rs::Openness`). Four leaves read through a dispatcher query instead — a conditional decision, a carrier head, an escape or default lowering, and the one-hop reduction behind a builtin the registry rule cannot prove closed — and that query may open a nested evaluation; the nested evaluation answers every declaration or instantiation proof this transaction already completed from the transaction's proof table, and the query is itself memoized, so a deep chain nests by question novelty, never one evaluation per hop.
- `keyof` lowers and projects its operand in structural transit with `Navigate`, then reads its key domain under the caller's demand. Mapped key-domain queries do not inspect values. Settling a declaration-backed `keyof` reuses an already settled source surface for names and index signatures; residual carriers use Navigate with the caller's reduction demand instead of expanding member values. Reverse-homomorphic inference selects the key before unwrapping a helper template and registering its indexed inference holes; those holes remain patterns rather than ordinary member reads.
- If two callers reach the same instantiated semantic node through different entry paths, they converge onto the same cache entry.
- If two callers reach the same declaration name with different type arguments, they do not alias to the same cache entry.
- When a declaration body is navigated in `Navigate` or `Shallow` mode with some type parameters unbound, the resulting graph preserves those parameters as first-class type-parameter references. Clients may later instantiate the preserved graph by applying a substitution environment.
- When a declaration is instantiated with a concrete substitution, every substituted occurrence of a type parameter in the projected result has a `SubstituteTypeParam` edge on the origin layer linking the substituted concrete type back to the parameter's declaration site.
- **Navigation-once invariant.** Navigating `type MyType<T> = …` performs at most one full body lowering per `(decl_identity, whole_hash)`. Later instantiations (`MyType<string>`, `MyType<number>`, …) reuse the parameterized lowering and run only substitution + terminal leaf materialization + branch selection. Distinct concrete instantiations never re-lower the body.
- **Relation-check-once invariant.** For a conditional `T extends S ? A : B`, the relation check runs at most once per distinct check-relevant substitution class. Two instantiations whose substituted check types are identical (e.g., `MyType<string>` called 50 times) run the relation check once.
- **Built-in utilities follow the same semantic model.** `Partial<T>`, `Pick<T, K>`, `Omit<T, K>`, `Record<K, V>`, `ReturnType<F>`, `Parameters<F>`, `ThisParameterType<F>`, `OmitThisParameter<F>`, `Awaited<P>`, `Uppercase`/`Lowercase`/`Capitalize`/`Uncapitalize`, `Extract`/`Exclude`/`NonNullable`, `NoInfer`, and every other built-in utility participate in the same `Instantiate` / `SubstituteTypeParam` / `ProjectMember` / `Normalize` / `ConditionalSelect` / `InferBind` / `AliasResolve` origin model as user-defined aliases and helpers. A utility is not a second semantic class.
- **The six signature utilities are one keyed read over the shared signature list.** `ReturnType` / `InstanceType` / `Parameters` / `ConstructorParameters` / `ThisParameterType` / `OmitThisParameter` classify once into the closed `SignatureUtility` key (`project_semantic_dispatch/signature_utility.rs`; `from_builtin_name` is the only spelling boundary — the degenerate-operand table and the origin-edge parameter names read the key, never the spelling). The subject's signatures come from `SignaturesOfType`, and inference reads the LAST candidate (conditional inference against several signatures reads the last one; first-applicable selection belongs to call resolution). A union subject DISTRIBUTES inside the keyed path (`resolve_signature_utility`): each arm settles through the same carrier demand point and answers on its own, the answers re-form a canonical union, and `OmitThisParameter` follows its lib definition over the distributed receiver union (one receiver-less arm returns the subject unchanged) — a union's synthesized signature is a call-site construct and never an inference source. A type parameter infers nothing (the conditional defers; the constraint's signatures are its apparent surface). A consumer that needs a candidate's authored `Signature` node reads it from the `SignaturesOfType` value's per-candidate `authored` list — descriptors are content-interned and outlive the file version that first published them, so the store's descriptor-level source is never a version-correct node.
- **Utility fast-path rule.** Utility-specific fast paths are permissible only when all three clauses hold: (1) measurably faster than the generic dispatch path on a benchmark fixture, (2) observationally equivalent — same `SemanticNodeId` for the same inputs, same mode / cache behaviour, same `SemanticGraphStats` attribution — to the generic path, (3) emit the **same origin edges** the generic path would have emitted. A fast path that skips edge emission is not allowed; revert to the generic dispatch instead. Optimisation happens only after equivalence is test-enforced.

Concrete expectations:

- Navigating `Box<C>['full']['bar']` resolves `"full"` against the instantiated `Box<C>` meaning, not the unsubstituted generic body.
- If `ProjectMember(ResolveDecl(Box), "full", [C])` is already cached, a later path reaching the same instantiated query reuses it.
- Navigating `MyType<T>` with `T` free returns a shape whose body preserves `T` as a parameter reference. Navigating `MyType<string>` returns a shape with `T` substituted, every substituted position carrying a `SubstituteTypeParam` origin edge to the decl's parameter list.

**Binder dependency summaries.** Immutable semantic subtrees carry graph-epoch
summaries independent of the substitution argument. A complete negative proof
returns the original subtree before consulting `(node, parameter, argument)`
memos; binder-presence and pending-substitution guards reuse that proof. Exact
infer declaration/reference identities participate. Sealed or missing children
cannot prove absence. Entries retain their graph roots, reserve the shared
retention budget, and use bounded eviction; graph invalidation clears them.
Infer declarations within nested signatures or conditionals belong to those
scopes and do not make an enclosing argument an infer pattern. An actual infer
pattern keeps its helper application addressable until the relation owner
requests its shape.

**Signature source demand.** Source readers settle reference chains through the
ordinary `Instantiate` owner and prepare arguments through the shared deferred
evaluator and continuation classification. They preserve resolved class
expression identity. Full structural-fact normalization has a different demand:
it may read a class expression's structural surface and is not a substitute for
that source reader. Transit and Skeleton projection retain indexed operators.
A declaration-backed index remains addressable until the shared indexed-access demand consumes it;
intermediate declaration reads use Navigate and reuse that same body demand.
Lowering an indexed reference uses Navigate even under transit demand. The raise
reducer's transit default is Shallow; using it here would defer callable argument
binding and change a generic function's returned class.
A mapped object operand keeps transit Navigate until its index selects K;
graph projection and deferred evaluation share that operand-context policy.
`typeof` indexed operands use the same transit Navigate source context as
`keyof`. An authored `T[keyof T]` reads its source surface once before the
selected-value demand, including when the surface is a module namespace.
Completed deferred-evaluation frames and retained memo witnesses fan their
canonical fact witnesses into the active tracers as verified sorted runs —
down to the first delivering scope, which hands its completed set further
down when it completes — preserving recorder attribution and refusal
propagation without sorting the same child evidence again. A root that
finishes as itself is admitted by its known one-step receipt, with no frame,
tracer or memo entry of its own.
The read-set owner merges batches in balanced rounds so growing prefixes are
not recopied for every following witness. Observation marks cover both local
facts and retained canonical runs, so consuming a completed result replaces
its witness with the receipt without retaining duplicate child evidence.
A named declaration accompanied by null or undefined retains its name.

**Key-domain reuse.** One typed key-domain result supplies enumeration,
membership and emptiness. Immutable key syntax uses a bounded, rooted summary;
live declarations and symbolic patterns retain the fact-backed resolution path.
A broad domain never proves finite emptiness, and unsupported source shapes
cannot produce absence evidence. Finite mapped batches reuse membership instead
of rebuilding a set per selected value. Mapped materializers index source members
once and preserve the first matching member's modifiers and declaration metadata.

Utility intrinsics follow eager alias argument semantics; nominal builtin
interfaces and classes keep deferred argument semantics. A budget recovery in
an `Extract` or `Exclude` operand propagates its diagnostic and resource partial
instead of becoming a semantic miss.
