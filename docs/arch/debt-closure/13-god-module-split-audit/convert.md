# convert — Tier 0 Step 0.3 god-module split audit

**File:** `crates\verter_ffi\src\convert.rs`  
**LOC:** 3783  
**Function definitions:** 163  
**Intra-file call edges:** 150  
**Method:** automated extraction via `tmp/audit_extract.py` (regex-based function-and-call enumeration; Tarjan SCC). The plan's §2.1.0 "Default tool" is a `syn`-AST extension to the architecture-guards scanner; that extension is deferred — this document was produced by the lighter-weight extractor in the same time window. The Tier 2 worker assigned to this module should re-derive any sections that look noisy with the syn-AST tool when it lands.

## 1. Intra-file strongly-connected components

No non-trivial SCCs and no self-recursive functions detected. The file's call graph is acyclic intra-file; recursion (if any) is handled cross-module.
## 2. Recursion-budget edges

No recursion-budget edges detected in this file.

Recursion in this file (if any) does not consult an explicit pinned budget constant or named depth counter. Cross-module budgets (e.g. `assertions::WALKER_DEPTH_CAP`) may still bound callers from outside.

## 3. Cache-identity edges

No `*Db` cache reads or writes detected in this file.

## 4. Public-surface edges

`pub fn` count: 27.

- `pub fn component_meta_analysis_to_ffi` — line 16 (span 16-20)
- `pub fn component_meta_analysis_to_ffi_with_resolution` — line 24 (span 24-332)
- `pub fn component_meta_resolution_to_ffi` — line 907 (span 907-911)
- `pub fn ffi_config_to_host` — line 1256 (span 1256-1303)
- `pub fn ffi_profile_to_host` — line 1306 (span 1306-1363)
- `pub fn ffi_file_kind_to_host` — line 1378 (span 1378-1384)
- `pub fn ffi_node_kind_to_host` — line 1387 (span 1387-1402)
- `pub fn ffi_upsert_to_host` — line 1405 (span 1405-1415)
- `pub fn ffi_block_override_to_host` — line 1438 (span 1438-1455)
- `pub fn host_preprocessor_request_to_ffi` — line 1458 (span 1458-1465)
- `pub fn ffi_virtual_query_to_host` — line 1512 (span 1512-1522)
- `pub fn host_node_kind_to_ffi` — line 1529 (span 1529-1552)
- `pub fn byte_offset_to_utf16` — line 1563 (span 1563-1566)
- `pub fn utf16_to_byte_offset` — line 1568 (span 1568-1578)
- `pub fn lint_diagnostics_to_utf16` — line 1589 (span 1589-1604)
- `pub fn host_diagnostics_to_ffi` — line 1606 (span 1606-1630)
- `pub fn host_update_to_ffi` — line 1633 (span 1633-1717)
- `pub fn host_virtual_file_to_ffi` — line 1720 (span 1720-1738)
- `pub fn host_resolved_id_to_ffi` — line 1741 (span 1741-1749)
- `pub fn host_remove_to_ffi` — line 1752 (span 1752-1756)
- `pub fn host_cross_file_result_to_ffi` — line 1759 (span 1759-1779)
- `pub fn host_error_to_string` — line 1785 (span 1785-1807)
- `pub fn code_action_to_ffi` — line 1816 (span 1816-1836)
- `pub fn lint_rule_to_ffi_metadata` — line 1843 (span 1843-1855)
- `pub fn convert_offset` — line 1875 (span 1875-1881)
- `pub fn utf8_to_utf16_offset` — line 1884 (span 1884-1886)
- `pub fn convert_destructured_block_meta` — line 1905 (span 1905-1925)

## 5. Cross-file shared-cache edges

| Target | Function references | Sample line |
|---|---|---|
| `VerterHost` | 3 | `update_result_full_round_trip` (line 2963) |

## 6. Tier 2 split sketch

**Tier 2 W5e candidate split** — 3 sub-modules. This is a SUGGESTION; the W5* worker assigned to this module is free to deviate.

### `from_proto.rs`

Decode helpers — `from_*` functions that transform protobuf wire types into session-side IR. Most leaf functions in convert.rs are decode-side.

### `to_proto.rs`

Encode helpers — `to_*` / `into_proto_*` functions that go IR → protobuf, including ComponentMetaPayload and ComponentMetaSurface assembly.

### `type_handle.rs`

TypeHandle / TypeQueryPath translation between session types and protobuf encoding. Stays small (~200 LOC) but is its own concern.
