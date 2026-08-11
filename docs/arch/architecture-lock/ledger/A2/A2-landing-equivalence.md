# A2 landing equivalence

The accepted identity for block A2 diverges from the reviewed candidate identity.
This artifact records the divergence and proves it carries no content change.

## Cause

The maintainer instructed that no commit landed on `main` reference the architecture
program or any of its blocks. The landed commit messages named both. Each commit was
replayed onto the same parent with a neutralized message; no file, mode or path changed.

## Identity

| Field | Value |
|---|---|
| Reviewed candidate commit | `80a7d9c328842f1457e866fb8588687e9f1d3118` |
| Accepted (landed) commit | `d6eefef76c515949a7b7f760bbdf4596a5eef77c` |
| Tree (both) | `eaffd3997f140c2c881179e8089ef6bd05b9bc8d` |
| Parent, reviewed | `13cedd6fc1315bfb6fec0c4cacb0eacdb02c6c83` |
| Parent, accepted | `fdad3da1375473ccf1375b48b2e13fffaba62d79` |

## Proof

The candidate and accepted commits name the same tree object, so the accepted content
is byte-identical to the reviewed content:

```
$ git rev-parse 80a7d9c328842f1457e866fb8588687e9f1d3118^{tree}
eaffd3997f140c2c881179e8089ef6bd05b9bc8d
$ git rev-parse d6eefef76c515949a7b7f760bbdf4596a5eef77c^{tree}
eaffd3997f140c2c881179e8089ef6bd05b9bc8d
```

The full diff between the two commits is empty:

```
$ git diff 80a7d9c328842f1457e866fb8588687e9f1d3118 d6eefef76c515949a7b7f760bbdf4596a5eef77c

```

The reviewed candidate object remains reachable from the retained review branches and
from this record; it is not required for execution, which proceeds from the accepted
identity.
