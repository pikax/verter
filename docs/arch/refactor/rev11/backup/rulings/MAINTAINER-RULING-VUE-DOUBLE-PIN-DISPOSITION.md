---
ruling_id: "VUE-DOUBLE-PIN-DISPOSITION"
type: "maintainer-ruling"
date: "2026-08-21"
date_source: "stated"
binds: ["Vue oracle pinning", "conformance infrastructure"]
source_file: "MAINTAINER-RULING-VUE-DOUBLE-PIN-DISPOSITION.md"
summary: "The Vue version split is DELIBERATE and temporary, not stale pinning: 3.5.x is the VDOM oracle, while 3.6.0-rc is used for Vapor because that is the RC's focus. It is not a defect and must not be collapsed opportunistically. Its disposition is fixed: once Vue 3.6 is FULLY RELEASED, Verter upgrades to 3.6 across the board and the double pin is removed entirely. Before then, a spike may measure whether moving the VDOM oracle to the latest RC changes VDOM output at all — a no-difference result would allow collapsing early, a difference needs the maintainer's decision because moving the VDOM oracle onto a prerelease that changes VDOM behaviour is a different risk from one that does not."
supersedes: []
superseded_by: []
contradicts: []
notes: "Recorded because the split reads as an inconsistency to anyone who has not been told otherwise — the program orchestrator initially classified `@vue/compiler-sfc` being pinned at both 3.5.34 and 3.6.0-rc.3 as a defect, and briefed an unconditional bump that would have moved the VDOM oracle onto a prerelease without measuring the consequence. The no-legacy rule still applies and is not in tension with this: Verter supports the latest RELEASED Vue, and the RC pin exists to develop against where Vue itself is heading, not to keep an old behaviour alive."
---

# Maintainer Ruling — the Vue double pin is deliberate, and ends at 3.6 release

**Status:** RATIFIED by the maintainer, 2026-08-21.

Recorded verbatim:

> 3.5.x is vdom oracle but maybe we can spike change to 3.6 to see if there's
> differences in vdom, if there's none we should update vue to be rc.5
>
> altho once vue 3.6 is fully released we will upgrade to 3.6 across the board
> and remove the double pin all together

## Current state, and why

```
@vue/compiler-core   3.5.34        VDOM oracle
@vue/compiler-sfc    3.5.34  AND  3.6.0-rc.3
@vue/compiler-dom    3.6.0-rc.3
@vue/compiler-vapor  3.6.0-rc.3    Vapor, where the RC's work is
```

3.5.x is the VDOM oracle deliberately. 3.6-rc carries Vapor, which is what that
release line is focused on. The split is a considered position, not drift, and
**must not be collapsed opportunistically** by anyone who reads it as an
inconsistency.

## Disposition

When Vue 3.6 is **fully released**, Verter upgrades to 3.6 across the board and
the double pin is removed entirely. One version, no split.

Before then, a spike may measure whether moving the VDOM oracle to the latest RC
changes VDOM output at all:
- **no difference** → the split can collapse early to the RC;
- **any difference** → the maintainer decides, because moving the VDOM oracle
  onto a prerelease that changes VDOM behaviour is a different risk from moving
  onto one that does not.

## Not in tension with the no-legacy rule

Verter supports the latest RELEASED version. The RC pin exists to develop
against where Vue is heading, not to keep old behaviour alive for users. Neither
pin is a compatibility shim, and neither survives 3.6's release.
