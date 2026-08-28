# ADR-005 — Operation DTOs Are Primary; Semantic Graph Export Is Optional

**Status:** Accepted

## Context

Removing general `TypeExpr` must not produce another mandatory general semantic representation at every public boundary.

## Decision

Public TypeInfo operations return operation-specific DTOs with exactness, diagnostics, dependencies, provenance, stable IDs, and rendered text only when requested. Session-local opaque handles may support continuation but are not stable IDs.

Graph export is a separate advanced operation with an explicit consumer inventory, compatibility domain, size/depth/node limits, deterministic IDs/order, and canonical serialization. Internal semantic storage need not mirror the wire graph.

## Consequences

Simple operations remain small and evolvable. Graph consumers are supported intentionally without constraining internal lifetime/storage.

## Rejected alternatives

- general recursive `TypeExpr`/`PortableTypeExpr`;
- mandatory graph payload for every query;
- wire node IDs reused as unchecked internal handles.
