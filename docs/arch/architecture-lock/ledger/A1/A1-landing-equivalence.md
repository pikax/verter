# A1 landing equivalence

The accepted identity for block A1 diverges from the reviewed candidate identity.
This artifact records the divergence and proves it carries no content change.

## Cause

The maintainer instructed that no commit landed on `main` reference the architecture
program or any of its blocks. The landed commit messages named both. Each commit was
replayed onto the same parent with a neutralized message; no file, mode or path changed.

## Identity

| Field | Value |
|---|---|
| Reviewed candidate commit | `13cedd6fc1315bfb6fec0c4cacb0eacdb02c6c83` |
| Accepted (landed) commit | `fdad3da1375473ccf1375b48b2e13fffaba62d79` |
| Tree (both) | `a992bb87382e58d6ec846c7be37cbb941ee0b1b2` |
| Parent, reviewed | `b7ea2dc88bda86473de81de3438b7f88ef30adc7` |
| Parent, accepted | `8a11cecf4f141f6a0254787e2fb51bd91b1d926b` |

## Proof

The candidate and accepted commits name the same tree object, so the accepted content
is byte-identical to the reviewed content:

```
$ git rev-parse 13cedd6fc1315bfb6fec0c4cacb0eacdb02c6c83^{tree}
a992bb87382e58d6ec846c7be37cbb941ee0b1b2
$ git rev-parse fdad3da1375473ccf1375b48b2e13fffaba62d79^{tree}
a992bb87382e58d6ec846c7be37cbb941ee0b1b2
```

The full diff between the two commits is empty:

```
$ git diff 13cedd6fc1315bfb6fec0c4cacb0eacdb02c6c83 fdad3da1375473ccf1375b48b2e13fffaba62d79

```

The reviewed candidate object remains reachable from the retained review branches and
from this record; it is not required for execution, which proceeds from the accepted
identity.
