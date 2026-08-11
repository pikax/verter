## Verdict

The **28-byte zero-growth invariant is correct**. The `32` assertion is a specification error. A2C remains implementable with **0 retained bytes and 0 completion-owned allocations**.

### 1. Exact discriminator correction

In [A2C-SPEC-REVISION.md](<EVIDENCE>/A2C/A2C-SPEC-REVISION.md:115), replace the locked-layout block with:

```rust
#[cfg(target_pointer_width = "64")]
{
    assert_eq!(std::mem::size_of::<SkeletonRegionFlags>(), 1);
    assert_eq!(std::mem::size_of::<SkeletonRegion>(), 28);
    assert_eq!(std::mem::size_of::<FunctionBodySkeleton>(), 96);
}
```

The test `a2c_completion_storage_has_zero_retained_growth` must use exactly those assertions.

No other size statement in the revision requires correction:

- `FunctionBodySkeleton == 96`: correct.
- `SkeletonRegionFlags == 1`: correct.
- Both zero-growth statements in §7: correct.
- The `48`-byte rejected statement fact and measured `10,616`-byte candidate references are historical measurements, not target layouts; leave them unchanged.
- Do not rewrite the stop finding or command proof; they correctly document the contradiction as found.

### 2. The fact fits with zero growth

The real [SkeletonRegion](<REPO>-wt-a2c/crates/verter_semantic/src/analysis/flow/mod.rs:175) contains a standalone one-byte `has_return: bool`. Replace that field—not the struct—with:

```rust
flags: SkeletonRegionFlags
```

A `SkeletonRegionFlags(u8)` has the same size and alignment as the replaced storage slot, so `SkeletonRegion` remains 28 bytes.

The combined state needs only five bits: one `has_return` bit and ten possible endpoint values.

```text
bit 0      HAS_RETURN

bits 1–4  endpoint code:
          0      Exact(DoesNotContribute)
          1      Exact(Contributes)
          2..=9  Unknown(CompletionUnknown(code - 2))

bits 5–7  reserved; must remain zero
```

Use:

```rust
const HAS_RETURN: u8 = 1 << 0;
const ENDPOINT_SHIFT: u8 = 1;
const ENDPOINT_MASK: u8 = 0b0001_1110;
```

Encode with:

```rust
flags = (flags & !ENDPOINT_MASK) | (endpoint_code << ENDPOINT_SHIFT);
```

Decode with:

```rust
let endpoint_code = (flags & ENDPOINT_MASK) >> ENDPOINT_SHIFT;
```

Initialize every region with `SkeletonRegionFlags(0)`. Replace direct `has_return = true` writes with `mark_has_return()`. Only root region zero may receive/read endpoint bits; non-root regions retain zero in bits 1–7.

Strictly, Rust `bool` itself has no reusable invalid bit patterns. The zero-growth solution works by replacing that one-byte field with a private `u8` bit carrier, not by placing extra values inside a `bool`.

### 3. Cost and gate ruling

Zero growth is attainable, so no positive-cost fallback is authorized. The correct retained A2C cost remains:

```text
FunctionBodySkeleton growth: 0 bytes
SkeletonRegion growth: 0 bytes
statement facts: 0 bytes
target facts: 0 bytes
completion-owned allocations: 0
completion-owned reallocations: 0
```

This satisfies the static layout and allocation requirements. It does **not** by itself prove the latency gate. The new candidate must still pass the required 30-sample interleaved benchmark:

```text
upper slowdown = max(3%, 2 × predeclared measured noise floor)
```

The previous 10,616-byte/157-allocation candidate neither condemns nor validates the zero-cost encoding. If the compact candidate fails the measured latency gate, stop under the existing recalibration rule; do not change the 28-byte invariant.

__DONE__
