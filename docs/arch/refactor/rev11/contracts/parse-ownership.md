# Parse Key, Ownership, Affinity, and Reparse Contract

**Status:** Normative cross-regime syntax ownership contract.

# 1. Exact construction identity

```rust
struct ParseKey {
    content: ContentId,
    language: LanguageId,
    syntax_contract: SyntaxCompatibilityId, // named domain + monotonic epoch
    syntax_profile: SyntaxProfileId,        // normalized parse/recovery/source-type options
}
```

`ParseKey` contains only dimensions that can change the constructed syntax result. It does **not** contain consumer names such as runtime, IDE, formatter, TypeInfo, or codegen. A consumer-specific difference is valid only when it changes the normalized syntax profile or the named syntax compatibility contract.

# 2. Owner domain

```rust
enum ParseOwnerDomainId {
    DirectInvocation(DirectInvocationId),
    DirectBatch(DirectBatchId),
    Prepared(PreparedCarrierId),
    Managed(ManagedParseOwnerId),
}

struct ParseInstanceId {
    owner_domain: ParseOwnerDomainId,
    key: ParseKey,
    generation: ParseGeneration,
}
```

The invariant is:

> One live `(ParseOwnerDomainId, ParseKey)` has one owner and one active result.

This is deliberately not a process-global invariant. Independent one-shot direct calls do not acquire a hidden global parse cache. A direct batch may share within its explicit batch owner. `PreparedCarrier` shares only inside the retained value. The managed engine may retain under a bounded owner/shard.

# 3. Shared frontend

Consumers with the same `ParseKey` within one owner domain reuse the same error-tolerant frontend result. A runtime/IDE distinction cannot justify a second parser. Derived indexes or views may differ but must identify the same parse instance and cannot override syntax meaning.

# 4. Affinity

- OXC allocator/AST and local mutable parse state remain on their owner.
- An already-owner-local consumer executes inline.
- A foreign worker sends a compact owned owner-call descriptor; the AST never crosses the boundary.
- No unsafe `Send`/`Sync` implementation hides an ownership mismatch.
- Only OXC-free compact values cross a general CPU executor boundary.

# 5. Retention and reparse

- direct invocation: request-local and dropped at return;
- direct batch: bounded by the explicit batch lifetime;
- prepared: pinned by the caller-retained `PreparedCarrier`, with inspectable retained weight;
- managed: byte-weighted, pressure-evictable, with explicit live pins.

After explicit eviction in a retaining domain, a later demand may start at most one same-key reparse flight. Reparse count and cause are observable. Retaining an index/graph does not implicitly retain the parse arena.

# 6. Locators

A locator carries enough identity to reject stale/wrong parse access:

```text
ParseKey
owner-compatible source/unit identity
node kind
span or canonical local ordinal
optional structural fingerprint
```

Lookup validates key, source/unit identity, node kind, bounds, and generation. Failure is typed; it never reads a same-span node from a different parse.

# 7. Required tests

- runtime and IDE consumers in one domain invoke the parser once;
- genuinely parse-affecting options create distinct keys;
- two independent direct calls do not share hidden state;
- prepared repeat reuses one parse until drop;
- managed pressure eviction produces one visible same-key reparse flight;
- graph/index retention does not pin the arena;
- stale locator and wrong generation fail deterministically;
- native threaded, native single-thread, and WASM/local profiles produce equal declared outputs.
