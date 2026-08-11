# A0 landing equivalence

The accepted identity for block A0 diverges from the reviewed candidate identity.
This artifact records the divergence and proves it carries no content change.

## Cause

The maintainer instructed that no commit landed on `main` reference the architecture
program or any of its blocks. The landed commit messages named both. Each commit was
replayed onto the same parent with a neutralized message; no file, mode or path changed.

## Identity

| Field | Value |
|---|---|
| Reviewed candidate commit | `b7ea2dc88bda86473de81de3438b7f88ef30adc7` |
| Accepted (landed) commit | `8a11cecf4f141f6a0254787e2fb51bd91b1d926b` |
| Tree (both) | `47645406a9246e600af995c62608b709347e13a4` |
| Parent, reviewed | `9af553dd262f82ac2f66e4ebf0a0faa70bc7aec0` |
| Parent, accepted | `9af553dd262f82ac2f66e4ebf0a0faa70bc7aec0` |

## Proof

The candidate and accepted commits name the same tree object, so the accepted content
is byte-identical to the reviewed content:

```
$ git rev-parse b7ea2dc88bda86473de81de3438b7f88ef30adc7^{tree}
47645406a9246e600af995c62608b709347e13a4
$ git rev-parse 8a11cecf4f141f6a0254787e2fb51bd91b1d926b^{tree}
47645406a9246e600af995c62608b709347e13a4
```

The full diff between the two commits is empty:

```
$ git diff b7ea2dc88bda86473de81de3438b7f88ef30adc7 8a11cecf4f141f6a0254787e2fb51bd91b1d926b

```

The reviewed candidate object remains reachable from the retained review branches and
from this record; it is not required for execution, which proceeds from the accepted
identity.
