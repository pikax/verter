# Canonical Identity Encoding Contract

**Status:** Normative identity/fingerprint encoding contract.  
**Binding ADRs:** ADR-002, ADR-004, ADR-012, ADR-016.

# 1. Identity authority versus digest

Typed descriptors define identity. A digest is an index/fingerprint of the canonical descriptor encoding and never replaces equality material where collision would be correctness-sensitive.

Each digest is domain-separated by a stable namespace and compatibility epoch.

# 2. Canonical encoding

Unless an accepted external protocol requires another encoding, identity descriptors use a tagged, length-delimited byte form:

```text
u32 domain_tag_length little-endian
bytes domain_tag UTF-8
u32 field_count little-endian
repeat fields in schema order:
    u16 field_tag little-endian
    u64 payload_length little-endian
    payload
```

Rules:

- fixed explicitly assigned enum discriminants;
- fixed-width integer encoding;
- booleans `0`/`1`;
- explicit present/absent tag for optionals;
- sets sorted by canonical element bytes;
- maps sorted by canonical key bytes and duplicate canonical keys rejected;
- strings use exact UTF-8 bytes; no implicit Unicode normalization;
- paths are already normalized through the captured project/source authority before encoding;
- schema changes bump the compatibility epoch or create a new domain;
- ad hoc delimiter concatenation, debug formatting, unordered JSON, and declaration-order enum hashing are prohibited.

# 3. Stable IDs

`StableEntityId` derives from documented domain-separated canonical identity material and is independent of allocation, traversal schedule, worker, cache history, or interner insertion.

Collision-sensitive use performs full descriptor equality or carries a deterministic disambiguator. Silent aliasing is prohibited.

`SessionHandle` is not a stable ID. It includes/validates owner cohort and generation and cannot be serialized as a stable reference unless an explicit protocol translates it to stable identity.

# 4. One-time normalization

Authoritative owners compute source, options, project, and profile descriptors once per revision/basis. Hot keys pass compact typed IDs rather than repeatedly canonicalizing paths, hashing unchanged bytes, or normalizing options.

# 5. Tests

- golden canonical bytes and digests per domain;
- permutation invariance for sets/maps;
- enum/schema evolution tests;
- path/source normalization basis tests;
- randomized hash seed/schedule/worker equality;
- collision injection/full-equality behavior;
- stable ID versus session handle misuse compile tests;
- native/WASM canonical encoding equality.
