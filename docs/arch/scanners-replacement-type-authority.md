# Component-meta type publication authority

This document defines the component-meta type publication boundary for props,
slot bindings, accepted props, and fallthrough props.

## Ownership

- `verter_type_expr::publication` owns the closed authority, evidence, policy,
  proof, and publication-result types plus the pure
  `select_type_publication(authority, evidence, policy)` function.
- `verter_semantic::analysis::component_meta` owns production and atomic merge
  of authority and authored evidence. It does not render terminal display.
- `verter_session::component_meta_resolution_policy::type_publication` owns
  structural classification for the two authored-selection exceptions.
- `verter_session::meta_resolve::projectors::output_sink` is the only
  production mint for `TerminalTypeDisplay` and the only place that
  materializes a selected semantic source into output `TypeExpr`.
- `verter_ffi`, `verter_protocol`, and `@verter/component-meta` transport and
  decode the already-selected result. They do not select again.

The macro admission tokens in
`meta_resolve::projectors::publication_authority` are a separate capability
boundary. They do not represent type publication authority.

## Three separate facts

`ResolvedTypeAuthority` is immutable semantic truth:

- outcome: `Present`, `Absent`, or `Failed`;
- exactness for a present outcome: `ExactConcrete`, `ExactSymbolic`, or
  `Incomplete`;
- typed producer provenance and diagnostics.

`AuthoredTypeEvidence` is an inseparable locator, authored text, and authored
provenance row. Its source is constructible only from an authored-body or macro
payload locator. Text or an arbitrary `SemanticTypeSource` cannot mint it.

`TerminalTypeDisplay` is output text. It is neither authority nor evidence and
is never an input to selection, policy, merge, cache identity, or inheritance.

## Pure selector

| Authority | Required structural input | Selected source | Result |
| --- | --- | --- | --- |
| `Failed` | none | none | same typed `Failed` |
| `Absent` | none | none | same typed `Absent` |
| `Present/ExactConcrete` | none | resolved | `Published/ExactConcrete` |
| `Present/ExactSymbolic` | matching typed equivalence proof | authored evidence | `Published/ExactSymbolic` |
| `Present/ExactSymbolic` | no matching proof | resolved | `Published/ExactSymbolic` |
| `Present/Incomplete` | typed incomplete permit | authored evidence | `Published/Incomplete` |
| `Present/Incomplete` | no permit | resolved | `Published/Incomplete` |

Evidence never changes the authority object. A selected authored representation
records that choice in `PublicationReason` and `PublicationProvenance`;
publication never upgrades incomplete authority to an exact result.

The only structural authored-selection classes are imported,
macro-participating compound types and imported indexed access. Local, bare,
and non-participating types do not mint a permit or equivalence proof.

## Carrier and merge rules

The full `TypePublication` carrier travels through semantic props, slot
bindings, accepted props, fallthrough branches, session projectors, and the
output envelope.

- Authority and evidence are merged as whole rows.
- Locator, text, and provenance from different evidence rows are never
  spliced.
- `Failed` is absorbing through merge and fallthrough inheritance.
- `Failed` produces neither a materialized output type nor terminal display.
  `Absent` may carry the established schema-absence placeholder for mechanical
  compatibility, but remains structurally `Absent`, never `Published`.
- Output materialization failure remains a typed output error; it is not
  replaced with `unknown`.

## Output and compatibility boundary

For `Published`, the terminal sink materializes the selected source and may
render a display. Authored evidence text may be used as display only when that
same authored source was selected. Otherwise display is rendered from the
materialized type.

The native target rows carry three distinct fields:

- optional materialized type;
- structured publication outcome;
- branded terminal display.

The current compat `rawType` property is allowed only as the mechanical
projection of `TerminalTypeDisplay.text`. It cannot feed semantic selection or
policy.

## Wire contract

Component-meta schema version 4 adds structured `TypePublication` and
`TerminalTypeDisplay` messages to all four target lanes. The retired target
`raw_type_id` fields remain reserved:

- `PropMeta`: tag 4;
- `SlotBindingMeta`: tag 4;
- `AcceptedPropMeta`: tag 3;
- `FallthroughPropEntry`: tag 3.

Those tags and names must never be reused. Decoders reject missing publication,
published rows without a type, and failed rows carrying a type or display.

## Enforcement

- Unit tests exhaust the selector table, authority immutability, display
  independence, atomic evidence merge, and absorbing failure.
- Session policy tests cover positive and negative structural classifiers.
- The existing compile-fail harness proves authored sources cannot be minted
  from text, display cannot enter the selector, and terminal display cannot be
  minted outside the sink.
- FFI, protocol, and TypeScript tests round-trip `Published`, `Absent`, and
  `Failed` separately from display.
- Production tripwires keep the deleted restoration and partial-merge helpers
  absent.
