# ADR-009 — IDE and Build Share One Error-Tolerant Frontend per Language Domain

**Status:** Accepted  
**Decision owner:** syntax frontend and parse lifecycle  
**Reopen only if:** an accepted ADR proves two genuinely incompatible syntax products are both necessary and cannot share one frontend without material product loss.

## Context

Including consumer role or vague parse flavor in identity can legitimize permanent IDE/build parser duplication. Interpreting “one parse identity” process-globally would force independent direct calls through global synchronization.

## Decision

For equal language bytes and `SyntaxProfileId`, IDE, build, TypeInfo, lint, formatter, and framework consumers use one Verter-owned error-tolerant frontend and parse product.

Identity is split:

```text
ParseKey = exact syntax construction identity
ParseOwnerDomainId = direct invocation/batch | PreparedCarrier | managed owner/shard
ParseInstanceId = (ParseOwnerDomainId, ParseKey)
```

Consumer role is not a key dimension. `ParseProductKind` is used only for a genuinely incompatible syntax product and requires a separate accepted ADR.

One live parse instance has one owner/result. Independent direct owner domains may parse independently. Retaining domains may pressure-evict and later perform one visible same-key reparse flight. Authored locators are revalidated after reparse. Graph/index retention does not implicitly pin the parse arena.

## Consequences

- no permanent build-fast versus IDE-tolerant dual parser;
- direct compilation stays free of process-global cache/synchronization;
- managed reuse remains explicit and bounded.

## Rejected alternatives

- **Role/flavor in identity:** hides duplicated parsing.
- **Process-global direct cache:** violates direct ownership and can add contention/retention.
