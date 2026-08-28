# ADR-010 — Compiler Products Are Compositional and Mapping Kinds Are Distinct

**Status:** Accepted  
**Decision owner:** compiler request and generated-artifact contract  
**Reopen only if:** a product can prove mutual exclusivity or a new mapping class with distinct semantics.

## Context

Real requests can require several products and independent materializations. A single artifact-level enum encourages hidden “full analysis.” Treating all maps as one product conflicts with IDE companions that require projection mappings and runtime outputs whose source maps are optional.

## Decision

A compile request contains:

- a canonical non-empty collection of typed product requests;
- per-product output and terminal materialization requests rather than one global output/materialization bag;
- a typed Vue or Svelte payload before planning;
- one shared semantic profile only when the requested work observes TypeScript-compatible semantics.

Each product request carries only the output, presentation, mapping, provenance, and serialization profiles that can affect that product. Duplicate product kinds and irrelevant profile fields are rejected before expensive work. Equal normalized subrequests may share one private stage/subplan.

Mapping classes are separate:

1. `PlacementMap` — source/unit placement composition used internally where required;
2. `SourceProjectionMap` — required by an IDE/provider companion and published atomically with it;
3. `RuntimeSourceMapData` — optional runtime/build map segments created only when requested;
4. `EncodedSourceMap` — terminal serialization of requested map data.

An operation with no mapping requirement performs zero map construction/encoding. No universal artifact bag is required; typed product results may share one private execution plan.

## Consequences

- runtime plus declarations or IDE plus public API can be requested coherently even when their output/terminal profiles differ;
- required mappings cannot be omitted or mixed with another code generation;
- presentation/serialization changes do not invalidate unrelated semantic/code artifacts;
- map-disabled runtime work remains truly map-free.

## Rejected alternatives

- **Single mutually exclusive artifact enum:** cannot express real product composition.
- **Always build one map type:** wastes work and conflates different validity contracts.
