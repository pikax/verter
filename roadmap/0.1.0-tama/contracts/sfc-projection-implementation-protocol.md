# SFC projection implementation protocol

Companion to `sfc-typescript-projection.md`. STP0 source contract for how later nodes implement projection without creating a second authority.

## One use, one specialization

A `ComponentUseId` identifies one component use. That use has one specialization transaction. Changing inferred use inputs invalidates that specialization. Unchanged checking text does not keep stale source mappings.

`ComponentUseId` is logical. Revision-qualified observations stay on the TCM0R / B4R0 identity plane.

## Three provenance planes

1. **Mapping geometry** — `CodeTransform` chunks and source maps. Not a type owner.
2. **Semantic origin** — Vue facts plus the TypeScript subject. No invented locations.
3. **Edit spelling** — LSO authored-edit transaction. Incomplete rename application is forbidden.

A second mapping owner is rejected (`STP0-authority`).

## Channel participation

Assigned as syntax/framework policy, not as a native TypeInfo variance result:

| Channel | Participation |
| --- | --- |
| Author-written prop/emit/slot/model signatures | `authored-signature` |
| Use-site props, JSX, slot props into a child | `contextual-consumer` |
| Excess-key / fallthrough validation | `validation-only` or `coupled` when the same use specializes |
| Hover/definition/references observation | `observation-only` when not an inference input |
| Generic component use | `coupled` (one transaction) |

## What this protocol does not authorize

Runtime activation, new public helper shapes, a duplicate script-body checker, per-fixture options, or native assignability as the published prop/emit/slot answer. STP1 owns the harness. STP8 owns ABI ratification. Production code stays with later implementation nodes.
