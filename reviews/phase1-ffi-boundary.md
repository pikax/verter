# Phase 1: verter_ffi Boundary Review

## Overall Assessment: GOOD — Production Ready

No critical bugs. Well-engineered Rust-JavaScript boundary with sound error handling and comprehensive testing.

---

## Issues Found

### P1 (High)

**1. UTF-16 Clamping Undocumented (MEDIUM)**
`byte_offset_to_utf16()` + `clamp_to_char_boundary()` silently adjusts offsets that land mid-character. If JS passes offset `5` but that's mid-emoji, it gets clamped to `4`. Missing test for this case — test only covers valid boundaries.

**2. `unwrap_or(0)` for Style/Custom Indices (MEDIUM)**
`ffi_node_kind_to_host()` defaults missing index to 0 for `style`/`custom` nodes. Should arguably be an error — ambiguity between explicit 0 and inferred 0.

### P2 (Medium)

**3. No Size Validation for Index Conversions**
Multiple `u32 → usize` and `usize → u32` casts without overflow checks. Safe in practice (Vue SFCs don't have millions of blocks) but semantically lossy.

**4. No Exhaustiveness Checking in Match Arms**
Catch-all `other =>` arms in FFI conversion mean new host enum variants won't trigger compile errors — must be caught by code review.

### P3 (Low)

**5. Missing `#[serde(skip_serializing_if)]`**
Optional output fields serialize as `null` instead of being omitted. Micro-optimization for WASM message size.

**6. Missing Edge Case Tests**
- Mid-character UTF-16 clamping
- Empty source
- Out-of-bounds offsets
- Very large indices

---

## Strengths

- Comprehensive error enum (`FfiConversionError`) with clear `Display` messages
- All FFI types flat + `#[serde(rename_all = "camelCase")]` — consistent
- Framework-agnostic conversion functions shared by NAPI and WASM
- 50+ tests covering main paths and round-trips
- No unwraps in public code — all Results or Option defaults
- Clean separation of input types (JS→Rust) vs output types (Rust→JS)

## Scorecard

| Category | Status |
|----------|--------|
| Correctness | GOOD — no data loss or panics |
| Error Handling | GOOD — comprehensive error enum |
| Performance | MINOR — negligible issues |
| Test Coverage | GOOD — missing some edge cases |
| Documentation | MINOR — needs UTF-16 clamping docs |
| API Design | GOOD — clear input/output separation |
