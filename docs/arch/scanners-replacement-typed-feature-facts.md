# Scanner Replacement: Typed Feature Facts

This mirror records the typed semantic facts that replace display-text and
shape sniffers. Facts are producer-owned, serde-safe, fail closed on partial
analysis, and remain separate from presentation strings.

## A1 — Resolved type authority

`ResolvedTypeAuthority` and `TypePublication` own the selected semantic source,
exactness, provenance, diagnostics, and authored evidence. Display annotations
are evidence or UI detail only; they never select semantic behavior. See
[`scanners-replacement-type-authority.md`](scanners-replacement-type-authority.md).

## A2 — Compatibility descriptors

Native type descriptors are the compatibility classifier's authority.
Unsupported or partial structure stays typed degradation and is never recovered
from terminal display text. See
[`scanners-replacement-compat-descriptor.md`](scanners-replacement-compat-descriptor.md).

## A3 — Public component contract

The public component contract carries ordered, producer-owned props, emits,
slots, exposes, and model surfaces. Occurrence identity and provenance preserve
authored ordering without parallel-lane reconstruction. See
[`scanners-replacement-public-contract.md`](scanners-replacement-public-contract.md).

## A4 — Prop callable role

`PropCallableRole` is an analysis-side fact on every resolved prop:

- `SvelteSnippet` carries the exact package-backed declaration identity plus
  `ResolutionExactness` and `ResolutionProvenance`.
- `Other` means identity resolution completed and did not match Svelte's
  exported `Snippet` declaration.
- `Unresolved` carries a closed reason such as missing dependency, cycle,
  budget/work limit, unsupported carrier, or fault. Its default is
  `AnalysisUnavailable`; absence never means `Other`.

Svelte analysis captures a named `Snippet` import independently of prop names,
then validates its package route to the defining `svelte` declaration.
`ProjectSemanticDispatch` resolves prop carriers through the shared
`ResolveDecl`/`Instantiate` machinery, including imported aliases and
multi-hop local aliases. The demand preserves dependency signatures, fixed-view
reads, cycle/budget fuses, and cache suppression. A partial demand returns no
node or speculative identity and taints cold-compute completeness so it cannot
be admitted as an exact warm result.

The role is preserved through `ResolvedPropField`, component analysis,
fallthrough/accepted surfaces, `AnalyzedPropDefinition`, serialization, and LSP
snapshots. Svelte slot synthesis and both snippet completion forms require an
explicit `SvelteSnippet` pattern match. `typeAnnotation` remains display-only.

The former `Snippet` display classifier, prop/member-name validation,
parse-candidate `$slots` synthesis, and member-name filtering are deleted.
No callable-role field is added to the public component-contract wire format.

## A5 — Template class semantic facts

Dynamic template-class domains are selected only after `RawTemplateData` exists.
The session joins each requested bare binding or `props.member` to its exact
same-revision declaration or macro-payload locator, then classifies the graph
through `ProjectSemanticDispatch`. `TemplateClassSemanticFacts` carries the
canonical owner, exact `whole_hash`, dependency `ReadSetSignature`, requested
subjects, closed literal domains, and exact reactive-wrapper proof.

Closed unions are all-or-nothing: every arm must resolve to a string literal.
Broad, mixed, nullable, cyclic, budget-limited, or missing domains fail closed;
no partial literal subset is projected. Vue wrapper roles require a
producer-authored reference-head fact whose shared import/export and local
alias route ends at a package-backed terminal with final import edge `vue`.
Terminal-instantiation demand supplies fully substituted wrapper arguments;
local and non-Vue package same-name wrappers never match.
Qualified namespace heads and `import("...").Member` heads retain their exact
authored root/specifier and member path while routing through the same shared
import/export authority. Requested macro-prop heads are projected graph-free
beside the existing lazy macro hot-mirror handle from the same typed-IR borrow;
the class consumer never re-reads a payload body.

The closed wrapper vocabulary is `Ref`, `ShallowRef`, `ComputedRef`
(`WritableComputedRef` normalizes onto it), `ModelRef`, `Reactive`, and
`ShallowReactive`. `ModelRef` keeps its own role rather than collapsing into
`Ref`, because `defineModel` is the one wrapper source the compiler itself
synthesises and a consumer must be able to tell a model binding from a plain ref.

Recorded boundary: the vocabulary only decides the role once a route already
exists. An UNANNOTATED binding — `const m = defineModel<'a' | 'b'>()` — has no
authored annotation, so the producer mints no authored reference head, so there
is no route candidate and no wrapper peel. The same holds for any inferred
wrapper binding. This is fail-closed and at exact parity with the deleted
scanner; it belongs to the deferred inferred/named-head class, not to the wrapper
vocabulary. It is pinned by a negative assertion in
`template_class_model_ref_is_in_the_wrapper_vocabulary`.

Route provenance is value-side EVIDENCE, never query or cache identity. The
authored head and the widened prepared external-dependency edge carry authored
local aliases and authored specifiers; they ride on the prepared/lowered
declaration VALUE (which derives neither `PartialEq` nor `Hash`) and are keyed by
the owner's content-addressed identity. No semantic query key, family slot, graph
node identity, or cache key includes them — `Ref as A` and `Ref as B` are the
same instantiation and hash-cons to one node, while the artifact still reports
the two distinct local bindings. The authored `args` locators are published route
evidence only: exact classification uses the TERMINAL SUBSTITUTED argument, never
the outer authored argument, so a transforming or reordering alias cannot be
classified from its outer arguments.

Template conversion consumes only an opaque revision-checked
`TemplateClassDomainIndex`. The normal compile, content-override, and lazy
raw-template lanes all build the same facts, and the builder stamps the revision
it actually resolved against — the owner shallow state the selected resolver
context serves — so the converter's owner-revision gate is a real cross-check
rather than an echo of the caller's argument. Publication requires complete
facts and a validated dependency signature; overlays and dependency-derived
Content results stay return-only.

A `ReturnOnly` fact set declines exactly two rails — the raw-template semantic
slot (no complete dependency signature) and the pure-content publish (not
owner-only). It must never emit a transitive non-cacheable mark: that
propagates to every active tracer on the thread, so the ENCLOSING compile /
component-meta signature would become non-cacheable for an ordinary valid SFC
whose only unresolved class subject is a `v-for` alias, a slot-scope alias, an
options-API binding, a typo, or a merely missing dependency. The inner tracer's
observations already fan out to the enclosing signature, so that signature still
invalidates on every dependency the class facts read.

Each row populates only the namespace it was requested through: a bare
identifier resolves in the bare namespace and `root.member` in the rooted one, so
a prop row never answers for a different same-named subject. A row the facts did
not admit records its label as refused, and a refused label resolves to nothing —
an explicit negative decision can never be overridden by another subject.

Bare identifiers fall back to a unique matching `defineProps` field only when no
unique local binding exists. `template_converter_inputs` now owns runtime
component linkage only. The former type-annotation union and wrapper string
scanners are deleted.
