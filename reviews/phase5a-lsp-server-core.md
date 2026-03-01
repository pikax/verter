# Phase 5a: LSP Server Core + Diagnostics Bridge Review

## Overall: WELL-ARCHITECTED — Production-Quality with Notable Gaps

Robust position encoding negotiation, resilient TypeProvider integration, clean separation of concerns. Two critical capability advertisement issues.

---

## Critical Issues

### C1. Capabilities Advertised Without Implementations
`typeDefinitionProvider` and `declarationProvider` advertised in `server_capabilities()` but `goto_type_definition()` and `goto_declaration()` not implemented. Default returns `Ok(None)`. Client shows menu items that never return results and may suppress fallback behavior.

### C2. Workspace File Operation Handlers Not Implemented
Server registers for `workspace/didCreateFiles` and `workspace/didDeleteFiles` with glob `"*"` but no handlers implemented. `$/onFileChanged` custom notification is a no-op stub.

**Impact**: New `.vue` files not indexed until opened. Deleted files remain in host cache with stale diagnostics. `tsconfig.json` changes never picked up after init.

---

## High Issues

### H1. Duplicate Diagnostic Computation (Push + Pull + Actions)
`publish_diagnostics()` (push on change) and `diagnostic()` (pull) both compute full diagnostics pipeline independently. `action_engine_fixes()` re-runs linter a third time. Each edit triggers 2-3 full linter runs.

### H2. Statistics FIFO Eviction O(n)
`Vec::remove(0)` shifts all elements. With max_events=500, every event causes 499 shifts while holding Mutex.
**Fix**: Use `VecDeque::pop_front()`.

### H3. `vue_to_tsx()` in PositionMapper Is O(n) Per Call
Linear scan of all source map tokens. `tsx_to_vue()` rebuilds lookup table on every call. Multiple feature calls per request multiply cost.
**Fix**: Pre-build lookup structures, cache lookup table.

### H4. DashMap Read Guards — Fragile Scope Patterns Near `.await`
Currently safe — guards drop before await points. But pattern relies on subtle scope rules. Any refactoring extending guard across `.await` would deadlock.

---

## Medium Issues

### M1. `percent_decode` Not Fully RFC 3986 Compliant
Decodes `%XX` as single bytes cast to `char`. Multi-byte UTF-8 sequences (CJK, accented chars) mangled. File paths with non-ASCII characters produce incorrect canonical IDs.

### M2. `path_resolver.read().unwrap()` Can Panic on Poisoned RwLock
Multiple locations. If any thread panics while holding write lock, all subsequent reads crash the server.
**Fix**: Use `unwrap_or_else(|e| e.into_inner())` or `parking_lot::RwLock`.

### M3. SFC Scanner `</script>` in String Literal Prematurely Closes Block
`find_close_tag()` uses first `</tagname>` match. `const x = "</script>"` in script block would misidentify boundary.

### M4. `on_did_change_ts_or_js_file` Only Sends Last Change Event
Takes `params.changes` (Vec) but only sends `last.text` to type provider. Other changes silently dropped.

### M5. `LineIndex` Stores Full Source as `Vec<u8>` — Double Memory
`DocumentState` already stores `source: Arc<str>`. Plus `verter_host` has its own copy. ~2x overhead per document.

### M6. Incremental Change Application Doesn't Validate Ordering
Doesn't check non-overlapping or reverse order (LSP spec requirement). Fragile with non-conforming clients.

### M7. `strip_json_comments` Treats Non-ASCII Bytes as Individual Chars
`bytes[i] as char` for bytes >127 corrupts non-ASCII characters in tsconfig.json paths.

---

## Low Issues

- L1: SFC scanner doesn't guard against duplicate/nested same-name blocks
- L2: `extract_attr_value` can match attribute names as substrings
- L3: `on_file_changed` is a no-op stub (node_modules changes ignored)
- L4: `completion_resolve` is a pass-through stub despite advertising resolve support
- L5: `did_save` is a no-op (no opportunity for save-triggered actions)

---

## Strengths

### S1. Robust Position Encoding Negotiation
Correctly implements LSP 3.17 encoding negotiation (UTF-8 > UTF-32 > UTF-16 preference). Flows through to LineIndex, DocumentRegistry, and analysis JSON conversion.

### S2. Resilient TypeProvider Integration
Every TSGO call wrapped in match with error logging. 5-second timeouts on TSX sync/diagnostics. ResilientTypeProvider handles crashes with auto-restart. Validated position mapping prevents queries at synthetic positions.

### S3. Clean Separation of Concerns
DocumentRegistry, PositionMapper, LineIndex, SfcBlock scanner, feature modules, and diagnostics_bridge all cleanly separated.

### S4. Thorough Test Coverage
Line_index UTF-8/16/32 roundtrips, position_map prepend mapping, sfc_scanner edge cases, statistics FIFO, config JSONC stripping, diagnostics_bridge severity/position mapping.

### S5. Source Map Mapper Handles Synthetic Regions Correctly
`tsx_to_vue` returns None for positions inside synthetic regions. Column-level adjustment for Original chunks.

### S6. ActionEngine Integration Well-Structured
Bridge matches diagnostics by rule name + exact range. Converts byte offsets to LSP TextEdits. Handles is_preferred flag.

### S7. Virtual File Routing Clean and Consistent
All feature handlers follow consistent pattern for virtual files: check context, route to TSGO without position mapping, convert via virtual file's own LineIndex.

---

## Priority Fixes
1. **C1**: Remove or implement typeDefinition/declaration providers
2. **C2**: Implement file operation handlers for host state management
3. **H1**: Cache DiagnosticSet per document, share between push/pull/actions
4. **M1**: Fix percent_decode for multi-byte UTF-8
5. **M2**: Handle poisoned RwLock gracefully
6. **M7**: Fix strip_json_comments for non-ASCII
