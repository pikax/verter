# TS spec §4.6 — Distributive conditional types

When the check type of a conditional is a "naked" type parameter
(i.e., `T extends U ? X : Y` where `T` itself is a type parameter
substituted with a union), the conditional distributes over the
union members of the substitution.

Source:

```ts
type StringsOnly<T> = T extends string ? T : never;
defineProps<{ kind: StringsOnly<'a' | 'b'> }>();
```

For `T = 'a' | 'b'`, the conditional distributes over each member:

1. `'a' extends string ? 'a' : never` → `'a'`
2. `'b' extends string ? 'b' : never` → `'b'`

Result: `'a' | 'b' | never` = `'a' | 'b'` (`never` is absorbed by
union).

Component-meta surface: one required prop `kind` whose
`type_signature` is `"a" | "b"`.

Discriminating-test linkage (§0p.A.5):
- A non-distributive resolver would treat the union as a single
  type and ask `'a' | 'b' extends string ? ('a' | 'b') : never`,
  which yields the same result HERE but would fail on a fixture
  that filters arms (e.g., `T = 'a' | 1`). The
  `template_literal_as_key` and `mapped_exclude/extract` fixtures
  exercise filtering distribution explicitly.
