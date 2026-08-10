# ADR-019 — Authority Publication Is Reproducible and Single-Source

**Status:** Accepted  
**Decision owner:** architecture package distribution.  
**Reopen only if:** a stronger content-addressed publication system replaces the bundled builder and validators.

## Context

Revision 10 was represented by divergent source, consolidated, and ZIP artifacts. A valid architecture cannot be safely implemented when its distributed authority is not singular.

## Decision

- one canonical unpacked source tree owns package content;
- `MANIFEST.json` and `VALIDATION.json` are generated from it;
- one bundled release builder produces the consolidated document, deterministic ZIP, validation report, and checksums;
- the builder re-extracts and revalidates the ZIP;
- generated artifacts are never edited independently;
- A0 rejects a digest or content mismatch.

## Consequences

The package digest has a precise meaning and can be bound into every baseline, charter, stack snapshot, and review record.

## Rejected alternatives

- manually zipping a working directory;
- publishing consolidated and split artifacts from separate trees;
- trusting filename/revision labels without content validation.
