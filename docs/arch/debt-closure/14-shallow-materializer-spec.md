# 14 — Shallow materializer spec (Tier 1B / D29)

## Purpose

The shallow materializer is the single rule for how a `TypeHandle` is turned
into a single-layer expansion. It is what powers `MetaSession::
get_component_meta_type_expansion` and the BFS bridge that backs the
public `getComponentMeta` API end-to-end. The spec is binding for any code
that produces a `TypeExpansion`.

## Eager-reducing intrinsics

The following TypeScript intrinsics compose into a single one-layer
expansion call. The materializer recognises them by matching on a
parameterised reference whose `target` resolves to a known intrinsic
declaration in the SDK (`lib.es5.d.ts` family).

| Intrinsic     | Rule                                                                                   |
| ------------- | -------------------------------------------------------------------------------------- |
| `Pick<T,K>`   | Materialise `T`'s object shape (one layer), retain only members in `K`.                |
| `Omit<T,K>`   | Materialise `T`'s object shape (one layer), drop members in `K`.                       |
| `Exclude<U,X>`| Materialise `U`'s union (one layer), drop arms assignable to `X`.                      |
| `Extract<U,X>`| Materialise `U`'s union (one layer), keep arms assignable to `X`.                      |
| `Partial<T>`  | Materialise `T`'s object shape (one layer), mark every member optional.                |
| `Required<T>` | Materialise `T`'s object shape (one layer), strip optionality.                         |
| `Readonly<T>` | Materialise `T`'s object shape (one layer), set readonly on every member.              |
| `NonNullable<T>` | Materialise `T`'s union (one layer), drop `null` and `undefined` arms.              |

Composition rule (D38): the eager-reducing intrinsics compose. A type like
`Pick<Omit<MyAlias, "a">, "b">` resolves in **one expand call** end-to-end
because the materializer recognises the outer intrinsic, expands the
inner intrinsic eagerly while still inside the same call, and finally
restricts members. The discriminating test
`intrinsic_pick_omit_compose_in_one_expand_call` enforces this.

## Lazy-yielding kinds

The following type kinds always yield a `LazyChild` in the surface so the
caller can choose whether to expand them.

- User-named aliases (D38 conditional rule). When an alias points at a
  body whose top-level node is itself an eager-reducing intrinsic, the
  alias is **not** treated as lazy on the call where the outer node is the
  intrinsic — that is the "intrinsic through alias" fast-path. In every
  other case the alias is lazy.
- Conditional types (`T extends U ? A : B`).
- Mapped types (`{ [K in Keys]: T[K] }`).
- `infer` placeholders.
- Non-literal indexed-access (`T[K]` where `K` is not a literal type).
- Recursion-visited nodes (already on the BFS frontier path).

## Property types within `Object` are always `LazyChild`

For an `ObjectShape` with N properties, the materializer surfaces the
`property_count` eagerly but produces N `LazyChild` references for the
property values. Materialising the parent costs **one** expand call,
regardless of N. The discriminating test
`shallow_materializer_object_with_n_properties_costs_one_expand_call`
asserts `materialize_structure_calls(Avatar) == 1` for the Avatar
component fixture (Object with N=12 properties).

## Intrinsic-through-alias fast path (D38)

When an outer intrinsic points at a user-named alias, the materializer
peeks the alias's body **inside the same expand call** if and only if the
alias body is itself an eager-reducing intrinsic. The combined operation
is one expand call. The discriminating test
`intrinsic_through_alias_composes_in_one_expand` enforces this for
`Pick<MyAlias, "k">` where `MyAlias = Omit<Base, "drop">`.

## Implementation surface

The materializer produces the protobuf
`verter::v1::ShapeOutline` plus a `Vec<NamedTypeHandle>` for the lazy
children. Both feed into `verter::v1::TypeExpansion`. See
`crates/verter_session/src/component_meta_payload.rs`.
