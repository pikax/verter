# TCM2 — Content-mapper projection plane

**Status:** DRAFT, pending DAG amendment + authorization record.
**Predecessors:** TCM0, TCM1. **Downstream:** TCM4.
**Dormant: unregistered and unreachable from production routing until TCM4.**

## Scope

A separately versioned package — provisionally
`@verter/typescript-content-mapper`. Do **not** overload
`@verter/typescript-plugin`.

Ship **exactly one** current codec, the one TCM0 selected. Not both versioned and
versionless init codecs, not a `V1 | V2` runtime enum, not the TypeScript-Go
historical protocol, not semver-selected wire formats, not a fallback codec, not a
compatibility parser spanning protocol generations. Unknown future contracts fail
closed. When upstream changes the contract, the superseded codec is replaced in
the same release rather than accumulated.

The mapper must: use the TCM0-measured topology; speak the current JSON-RPC over
stdio; write **only** protocol frames to stdout with logs on a separate channel;
call the canonical Verter compiler/cache for its process; isolate state by
TypeScript's opaque project handle; serve several handles in arbitrary order;
release state on `closeProject`; bound message sizes, queues, outstanding work,
handles and caches; support cancellation where the protocol permits; implement
stable config and watched-file identities; support primary and supplemental
outputs; validate `TypeScriptSingleInputProjection`; emit an explicit feature mask
on every span; implement mapper diagnostics and directives; validate signed
`int32` bounds before serialisation; preserve exact project/compiler options; and
conform across project references, monorepos, build, watch, incremental,
declarations and declaration maps.

**Purity.** The mapper must never call the TypeScript semantic API, send LSP
requests, wait on a TypeScript project snapshot, or touch the old provider/relay.
Add a hard test that FAILS if any transform path can reach any of them — this is
the discriminating deadlock/reentrancy proof TCM0 specifies.

**Single-input projection.** Verter may hold several source units; a transform
maps output back to exactly one input file and revision. The
`TypeScriptSingleInputProjection` constructor must prove every serialised original
range belongs to that exact input. Never encode source-unit IDs into offsets, map
foreign-file offsets onto the component, silently take the first unit, blindly
drop foreign spans, or serialise `PlacementMap` as a span map.

Do not treat TypeScript-created virtual filenames as stable Verter identities.
