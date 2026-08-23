# TCM4 — Atomic activation and deletion

**Status:** DRAFT, pending DAG amendment + authorization record.
**Predecessors:** TCM0, TCM1, TCM2, TCM3.

## The shape of this block

Activation and deletion are **one atomic accepted transition**. There is no
intermediate production state in which both the new and old paths operate.

## Activation

**Configured projects** use their `tsconfig`-declared `contentMappers`.
Extension registration may assist discovery but must not pretend to inject mapper
config into a configured project.

Before publishing any TypeScript-backed capability, attest: the certified engine
is active; the expected project/config is active; the mapper is loaded; its config
identity matches; the content-mapped source is in the current project; required
semantic capabilities are attached; generations agree. Disagreement fails closed.

**Version is candidate discovery, not activation authority.** `semver >= 7.1.0`
is not the gate — a preview sorts differently from stable and a version string
does not prove the contract is present. Certification requires accepted
executable identity, a passing mapper conformance probe, a passing semantic
capability probe, and satisfied trust requirements.

When the mapper is absent: enter `NeedsMapperConfiguration`. Do **not** run the
old carrier path, do not publish features needing unattested TypeScript data,
retain only independently sound framework-native features, and issue one
actionable recommendation deduplicated by project-config × engine × mapper-config
identity. Never mutate `tsconfig` silently; a JSONC edit preserves comments,
formatting and `extends`, avoids duplicates, refuses overlapping `.vue`/`.svelte`
ownership, and is shown before applying.

**Trust:** respect workspace trust, external-code requirements, package execution
boundaries and local-pipe permissions. Never bypass a TypeScript trust refusal
through a private channel.

## Deletion — the exact closure comes from TCM0

Delete, where applicable: `@verter/typescript-plugin`; carrier injection;
carrier-only generated stores and external sync; provider-only `.verter.ts`
import projection; Native Preview relay interception; global `tsdk` staging; relay
advertisement, attestation, taint filtering and synthesised neutral responses;
duplicate generated/provider/original remapping; duplicate companion compilation;
the old version-selection policy; old tsserver and TSGO carrier providers; private
semantic-query protocols; carrier lifecycle methods on `TypeProvider`; the broad
`TypeProvider` abstraction once no caller needs it; DTOs whose only owner was the
removed route; historical codecs; compatibility flags and fallback branches.

Do not delete a neutral facility with a demonstrated surviving owner. Do not
delete the old query plane before its capability ledger is green. **Do not retain
it after the ledger is green.**

## Acceptance

Performance gates are not waived because the API is new. A performance miss
blocks acceptance. Required: no regression in direct no-projection compilation;
zero projection allocations on that path; no recompilation caused only by terminal
feature policy or position encoding; no duplicate TypeScript compilation; no
unbounded state; no hidden second project graph unless TCM0 proved it necessary;
removed relay/plugin/carrier work absent rather than bypassed.
