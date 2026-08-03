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
