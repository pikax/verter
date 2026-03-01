# Phase 4: Host Layer Review

## Overall: PRODUCTION-QUALITY — Sound Caching & Invalidation

~7,500 lines. Tiered cache invalidation, TOCTOU-aware locking, panic-safe analysis. No critical bugs found.

---

## Critical Issues

**None found.** Core correctness of caching, invalidation, and compilation pipeline is sound.

---

## High Severity

### H1. Wasted-Work Window in `compile_entry`
`get_virtual_file` releases read lock (cache miss detection), then calls `compile_entry`, then acquires write lock (result storage). If another thread upserts the same file in this window, compilation proceeds with stale data but stores keyed to `captured_semantic_hash`. Hash comparison on next request catches mismatch → recompile. **Not incorrect, but wastes CPU.**

### H2. Two Read Locks Held Simultaneously in `compute_cross_file_optimizations`
`files` read lock held while acquiring `alias_to_canonical` read lock inside loop. Safe with current lock ordering (no writer needs both), but **no documented lock ordering invariant**. Future change could introduce deadlock.

### H3. Compile Profile Hash Uses `DefaultHasher`
`DefaultHasher` is `SipHash-1-3`, deterministic within a single build. Currently safe since all `CompileProfile` fields have deterministic `Hash` impls. Fragile if future fields depend on HashMap iteration order.

---

## Medium Severity

### M1. `canonicalize_id` Does Not Handle Windows Drive Letter Casing
`C:\foo\bar.vue` and `c:\foo\bar.vue` produce different canonical IDs. Also doesn't resolve `.`/`..` segments in absolute paths. Could cause cache misses with inconsistent callers.

### M2. Non-SFC `semantic_hash` Uses `whole_hash`
For `.ts` dependency files, any change (comments, whitespace) triggers dependent SFC recompilation. Correct but conservative. Mitigated by Tier 2/3 smart invalidation checking specific export changes.

### M3. Style Override Re-Analysis Finds `</style` by String Search
`source[content_start..].find("</style")` could match inside CSS comments (e.g., `/* </style might break */`), truncating content prematurely. Rare but possible with SCSS.

### M4. Duplicate Compilations Under Concurrency
Multiple threads seeing cache miss for same file+profile each compile independently. Last writer wins. Deterministic so not incorrect, but wastes CPU. Acceptable for LSP (single-client), noticeable for bundler parallel transforms.

### M5. `style_override_hash` Uses `0` for "no overrides"
If `DefaultHasher` produces `0` for actual override set, indistinguishable from "no overrides." Probability `1/2^64` — negligible.

---

## Low Severity

### L1. Panic Recovery Swallows Backtrace Context
`catch_analysis_panic` converts OXC panics to warning diagnostics but loses backtrace. Helpful to log full backtrace in debug mode.

### L2. `generation` Field Incremented but Never Read
`FileEntry.generation` counter unused by any code. Reserved for future use or dead code.

### L3. No Global Memory Cap Across All Files
Each `FileEntry` holds source + compile slots + analyses. With `max_profiles_per_file = 8` and 1000+ files, could use hundreds of MB. LRU eviction per-file helps but no global cap.

### L4. `import_resolves_to_dep` Basename Matching False Positives
If two deps share the same filename (`/a/Button.vue` and `/b/Button.vue`), first found wins. Could produce incorrect cross-file optimization results.

---

## Strengths

### Tiered Invalidation Design (Crown Jewel)
- **Tier 1**: No export signatures → full invalidation (conservative)
- **Tier 2**: Export-level → only invalidate when macro-consumed exports changed
- **Tier 3**: Type shape → skip invalidation when resolved type shape unchanged

### TOCTOU-Aware Locking in `upsert()`
Change detection + state update atomic under single write lock. Explicit comments explaining rationale.

### Panic-Safe Analysis
OXC panics caught → warning diagnostic + default analysis. Critical for LSP resilience.

### Semantic Hash Separation
`whole_hash` (byte-identical) vs `semantic_hash` (content + descriptor fingerprint). Whitespace-only changes skip recompilation.

### `Arc<str>` Zero-Copy Source Sharing
Source shared between host file entry and compilation via reference count.

### Comprehensive Test Coverage (~120+ tests)
Parse snapshots, upsert change detection, cache invalidation, LRU, ID normalization, cross-file optimization, source map remapping.

### Clean Module Decomposition
Pure logic modules (`parse.rs`, `hash.rs`, `upsert.rs`, `cache.rs`) separate from stateful `impl VerterHost` methods organized by lifecycle.

---

## Recommendations
1. **Medium**: Normalize Windows drive letter casing in `canonicalize_id`
2. **Medium**: Use proper `</style` boundary detection (not string search)
3. **Low**: Document lock ordering invariant in `shared.rs`
4. **Low**: Remove or use `generation` field
5. **Low**: Consider per-file compilation lock to prevent duplicate work under concurrency
