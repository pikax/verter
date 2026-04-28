# TS spec §4.5 — Indexed access types (two-level chain)

Indexed access types resolve `T[K]` by treating `T` as an object
type and `K` as a single key (or a union of keys). For chained
access `T['a']['b']`, TS evaluates left-to-right.

Source:

```ts
interface ButtonStyles {
  variants: {
    size: 'sm' | 'md' | 'lg';
    color: 'red' | 'blue';
  };
}
```

`ButtonStyles['variants']` →
`{ size: 'sm' | 'md' | 'lg'; color: 'red' | 'blue' }`. Then
`...['size']` → `'sm' | 'md' | 'lg'`.

Component-meta surface: one required prop `size` whose
`type_signature` is the union literal `"sm" | "md" | "lg"`.

Discriminating-test linkage (§0p.A.5):
- Drift in either intermediate hop would either yield a different
  union (sibling `color`'s value) or the entire object literal.
  Caught by byte-equality.
- Verter rule (CLAUDE.md §"Macro Type Traversal Rule"): "type
  navigation must stay narrower than expansion: walking
  `A['c']['full']['bar']` should navigate intermediate hops and
  expand only the terminal requested projection." This fixture is
  the smallest test of that rule.
