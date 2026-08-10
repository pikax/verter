# ADR-007 — Prove the Direct Core Before Managed Runtime Convergence

**Status:** Accepted

## Context

Generalizing query/executor/cache/session infrastructure before the direct operation and artifact boundaries are final risks preserving legacy managed assumptions under new names.

## Decision

The critical dependency direction is:

1. Gate 0 evidence, semantic safety, identity/compatibility/performance lock;
2. typed compositional requests, shared syntax frontend, direct compiler, prepared/resumable transaction, source units/mappings;
3. one semantic kernel and sealed compile projections;
4. sole effective-flow solver and public semantic cutovers;
5. coherent InputStore, QueryRuntime, flights, executor, retention, incrementality, providers, and host decomposition.

CSS and bounded framework-contract work may proceed only through explicit DAG edges.

## Consequences

Managed execution becomes reuse/orchestration around the smallest proven computation rather than defining it.
