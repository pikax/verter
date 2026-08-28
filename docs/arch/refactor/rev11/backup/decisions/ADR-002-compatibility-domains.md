# ADR-002 — Compatibility Domains Are Explicit and Monotonic

**Status:** Accepted

## Context

Internal progress counters should disappear, but Verter also has real serialized/public/persistent boundaries. Resetting an existing published epoch in place can make old and new bytes ambiguous.

## Decision

- a version-like value exists only for a real compatibility domain;
- one domain has one owner and a monotonic epoch sequence;
- zero is a valid first epoch and never an uninitialized sentinel;
- an incompatible clean replacement creates a new domain/namespace whose first epoch may be zero;
- disposable private caches may be invalidated by a new namespace/build fingerprint;
- ordinary in-memory DTOs remain versionless;
- duplicate counters that must “stay equal” are collapsed or separated into genuinely independent domains;
- package semver, source revisions, provider epochs, and external tool versions are not compatibility epochs.

## Consequences

Breaking pre-1.0 changes remain possible without rewriting chronology or preserving accidental counters.

## Rejected alternatives

- preserve every historical counter;
- reset every retained counter in the same namespace;
- add version fields to ordinary cross-module values.
