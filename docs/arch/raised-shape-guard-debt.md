# Raised-Shape Readiness Guard — Accepted Scanner-Debt Residuals

**Status**: ACCEPTED defense-in-depth scanner-debt. The owner-local node-domain
raised-shape readiness surface is dormant (zero production callers) and fenced by
ONE scanner — the zero-production-caller fence
(`node_domain_readiness_primitives_have_zero_production_callers` in
`crates/verter_session/tests/cases/raised_shape_core_guards.rs`). It is NOT the
production correctness boundary. The PROTECTED END-PROPERTY is held by INDEPENDENT
rails — part structural-by-construction, part an independent TEST pin:

- **the raiser's output cannot silently drift** — anti-drift is STRUCTURAL: there
  is exactly ONE `SemanticNodeData -> TypeExpr` traversal
  (`raise_node_to_type_expr_core_impl`), private to
  `project_semantic_dispatch::raise`; the production raiser
  (`raise_node_to_type_expr_inner`) calls it directly, and the node-domain
  facts/key are a pure VIEW of that one traversal's output (the legacy `TypeExpr`
  predicates / the `TypeExpr`-wrapping `RaisedShapeKey`). The raiser's ABSOLUTE
  output is pinned independently by the materialize-parity / raise-materialization
  SUITE in `raised_shape_tests.rs` — an independent absolute-output TEST rail;
- **the readiness primitives have zero production callers** — the Kind-B sites
  stay on `legacy_semantic_type_expr_bridge`; the cap mint backing the node-bearing
  expansion facade is compiler-confined
  (`pub(in crate::host_manage::component_meta_methods)`, E0624 for a non-owner),
  and the bridge reference set is pinned by the two
  `output_projector_residual_guards` pinning guards.

Be precise about what each rail guarantees. The END-PROPERTY — the readiness
facts/key equal the legacy predicate on the SAME raised value, and the readiness
primitives have ZERO production callers — is what is guaranteed. The
"raiser-cannot-drift-its-output" property is structural (one private traversal;
the facts are `legacy_predicate ∘ raise(node)`) and is additionally pinned by the
materialize-parity / raise-materialization SUITE in `raised_shape_tests.rs`. The
zero-production-caller fence's scanner gaps below (unexpanded macro-generated
references, `syn::Verbatim` token forms `syn` does not structurally interpret,
semantic aliases/re-exports, complex `cfg`/`cfg_attr` behavior beyond the explicit
skip policy) are real RESIDUAL REVIEW/TEST risks — they cannot, however, silently
wire a live unfenced production caller in a way that escapes the compiler-confined
cap mint + the bridge no-new-reference pins. Per the 8-A4 hardening-bound
architecture ruling (structural-confinement-first; a scanner is capped at TWO
hardening rounds of add/broaden, and the per-piece impl scanner was REPLACED — not
broadened — by a whole-impl-scan-minus-sanctioned-spans mechanism + a claim
narrowing, then the codex terminal-lock added two cheap correctness fixes
(raw-identifier `r#` normalisation; the cfg(test) impl-item skip hoisted to all
scan paths) + a further claim narrowing, all of which keep `hardening_rounds: 2`),
these residuals are recorded here and deliberately NOT chased (no alias-resolution
/ macro-expansion / name-resolution treadmill).

## Accepted scanner-debt residuals

### `node_domain_readiness_primitives_have_zero_production_callers` (`hardening_rounds: 2`, bound reached)

**Claim (honest, narrowed):** this guard is a dormant, defense-in-depth lexical
tripwire. It rejects common direct lexical references to the readiness symbols in
the guarded production files, using a shared whole-item `syn::Visit` scanner plus
precise subtraction for sanctioned definition spans and documented test-only
impl-items. Identifier tokens are normalised for the raw-identifier (`r#`) spelling
before matching, so a raw-spelled reference is the SAME identifier as its bare name
and is caught. It is not a Rust name resolver, macro-expansion proof, cfg
satisfiability engine, or semantic alias analysis. Known residuals include
unexpanded macro-generated references, semantic aliases/re-exports, complex cfg
behavior beyond the explicit skip policy, and `syn::Verbatim` token forms that
`syn` does not structurally interpret. These residuals are bounded and acceptable
for the interim dormant fence because production reachability is also constrained
by independent compiler/ownership rails and the later cutover must re-establish
structural guards.

Scans `verter_session/src/**` (non-test) for production references to the seven
readiness symbols: whole-identifier tokens in item bodies/signatures/fields. The
defining files (`raise.rs` for the classifiers/equality,
`component_meta_methods.rs` for the facade) are scanned with their bodies,
subtracting ONLY the exact sanctioned definition SPANS. Every item — INCLUDING
every `Item::Impl` — runs through the SAME whole-item identifier walk (no
special-casing of which impl sub-parts to scan): the walk descends attrs,
generics, where-clauses, the trait path, the self_ty, item bodies, and nested
items by construction (so a readiness reference in an impl HEADER type-argument, a
WHERE-clause bound, an ATTRIBUTE argument list, or a method BODY is caught by
construction). The ONLY spans subtracted, by SPAN (not by symbol globally): a
top-level `fn`/`struct`/`type`/`enum` whose OWN NAME is a readiness symbol (its
definition + internal wiring); an impl-item whose OWN NAME is a readiness symbol
(the readiness fn relocated as an associated def — its own subtree); for an
INHERENT `impl <readiness-type>` (no trait), ONLY the HEAD occurrence of the
self_ty — the bare outer type-name ident that names the artifact — while the
self_ty's GENERIC ARGUMENTS / nested types are STILL scanned (so a readiness
symbol used as a generic argument on the self-type,
`impl NodeBearingExpansion<NodeBearingExpansion<()>>`, the INNER occurrence, IS
reported; a TRAIT impl with a readiness self-type is production wiring and is NOT
exempt); and EXACT-`#[cfg(test)]`-gated items / impl-items. Item cfg-gating uses
the strict `cfg_is_exactly_test_or_test_support` classifier INCLUDING `Item::Use`.
All three scan paths (whole-file, per-item, whole-impl) route through ONE shared
identifier visitor, so macro invocation AND attribute (`Meta::List`) token trees
are scanned recursively for the readiness idents CONSISTENTLY on every path — no
scan path is special.

This MECHANISM REPLACED the prior hand-rolled per-impl-piece scanner (per-impl-ITEM
bodies + a separate impl-HEADER token scan), which kept MISSING impl sub-constructs
round after round (items, then header type-args, and it would still have missed
where-clauses + attributes). The whole-impl-scan-minus-sanctioned-spans mechanism
covers where-clauses + attributes BY CONSTRUCTION, so they are NO LONGER residual
debt.

NOT detected (bounded debt — TRUE residual non-claims only):

- **Proc-macro EXPANSION** — only the pre-expansion LITERAL tokens inside a macro
  invocation are scanned; a readiness symbol that a proc-macro SYNTHESISES in its
  expansion (absent from the call-site tokens) is not seen.
- **`syn::Verbatim` token forms that `syn` does not structurally interpret** — a
  readiness ident inside a `syn` `Verbatim` node (the GENERAL family — `Item` /
  `ImplItem` / `Type` / `Expr::Verbatim` and any other `syn` does not structurally
  interpret) routes to the no-op `visit_token_stream` and is NOT scanned. This is a
  lexical-visibility residual of the SAME family as proc-macro EXPANSION. It is a
  bounded disclosure gap, acceptable for the interim dormant fence because
  production reachability is also constrained by the independent compiler/ownership
  rails above and the later cutover must re-establish structural guards. Scanning
  the Verbatim arms is OPTIONAL hardening, not required; honest disclosure is the
  standing position (the codex terminal-lock ruling explicitly chose disclosure
  over scanning the Verbatim forms).
- **Raw-identifier spelling beyond the `r#` normalisation** — identifier tokens are
  normalised for the leading raw-identifier `r#` escape before matching (a
  raw-spelled `r#node_can_shell_raise` reference IS caught under the bare name).
  Any residual lexical spelling beyond that normalisation is part of the disclosed
  bounded residual, not chased.
- **Semantic ALIASING** — a readiness fn re-bound under a different name
  (`use X as Y`, a local `let f = X;`) and then called through that alias is not
  name-resolved.
- **Deliberately-unsupported `cfg` / `cfg_attr` complexity** — the test/production
  split recognises only the EXACT canonical test/test-support gate; an exotic
  `cfg` / `cfg_attr` conjunction that gates an item out of production by
  entailment (but is not the canonical shape) is treated as production and
  scanned (intentionally conservative — a production caller cannot hide behind a
  non-canonical gate), and a readiness symbol synthesised only by a `cfg_attr`
  expansion is not chased.

## REAL structural debt owed at the 8-A4 CUTOVER (NOT now)

The readiness surface is zero-drift-now via materialize-then-predicate (it builds
the full `TypeExpr` to read one bool / compare one key), NOT the node-domain /
perf end-state. The 8-A4 Kind-B graph-native conversion owes:

- re-implement the facts/key as a TRUE BOTTOM-UP node-domain projection that
  computes them from the graph nodes DIRECTLY (without building the full
  `TypeExpr`), encoding the raiser's transforms EXACTLY (the Intersection arm-drop
  + 0/1/many collapse, the Object empty-vs-sentinel split with a fresh per-member
  cycle set, the exact `?` / `filter_map` / `.unwrap_or(Unknown { "<raise miss>" })`
  positions, and the legacy NON-recursion through `Ref` / `ImportType` / `TypeOf`
  carrier args, `TypeParam` constraints/defaults, and template expressions);
- switch the Kind-B raise-then-decide sites to the node-domain classifiers /
  equality primitive + the expansion facade;
- DELETE `legacy_semantic_type_expr_bridge` and retire the temporary
  zero-production-caller fence (and this scanner-debt ledger with it).

Owners: the 8-A4 Kind-B conversion block. Tracking row:
`docs/arch/parselower-design.md` (the 8-A4 DEBT row).
