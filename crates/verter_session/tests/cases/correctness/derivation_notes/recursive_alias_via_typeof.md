# TS spec §3.7 — Recursive type aliases

A type alias may reference itself, provided the recursion is guarded
by an indirection (an array, a tuple, an object, or a generic
substitution). The TS checker terminates expansion at the recursive
reference and emits the alias's name.

Source:

```ts
interface Tree {
  label: string;
  children?: Tree[];
}
defineProps<{ root: Tree }>();
```

Component-meta surface: one required prop `root` whose
`type_signature` is the named ref `Tree`. The renderer surfaces the
top-level reference rather than the (potentially infinite) expansion.

Verter rule (CLAUDE.md §"Shallow File Processing Core Invariant"):
"processing a file means collecting and indexing its symbols, not
eagerly evaluating them". The `Tree` interface is indexed but not
expanded — the expansion would be triggered on demand by a deeper
projection (e.g., `root.children[0].label`).

Discriminating-test linkage (§0p.A.5):
- An eager-expansion resolver that walked into `Tree.children:
  Tree[]` and recursed would either stack-overflow or surface
  `RecursiveRef`. Either case differs from the canonical `Tree`
  string and is caught by the byte-equality check.
