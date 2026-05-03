# Tier 1B implementation feedback

This file replaces the user-instructions feedback log location (which is
not writable inside this worktree). Entries follow the standard CLAUDE.md
format.

- [issue] `crates/verter_session/src/meta.rs:730` `get_component_meta_payload`
  signature differs from plan §3.3.2.3 wording. Plan says
  `encode: impl FnOnce(&ComponentMetaPayload) -> Vec<u8>`; existing impl uses
  `encode_fn: impl FnOnce(ComponentMetaAnalysis, &ResolvedComponentMetaState) -> Vec<u8>`.
  Resolution: kept the existing signature (callers depend on it) and threaded
  the BFS bridge into the body around the existing analysis pipeline. Brief's
  D90 BFS bridge body is exposed as a private `get_component_meta_payload_bridge`
  helper so the BFS path is testable without disturbing the public surface.

- [improvement] `MAX_BRIDGE_DEPTH` placed in
  `crates/verter_session/src/component_meta_payload.rs` per D125.

- [debt] At 1B-close, `OwnedTypeResolutionContext::declaration_fingerprints`
  is still empty (Step 1A introduced the field; 1C-α populates it). Surface
  envelope therefore hands out empty `TypeHandle`s and the BFS terminates at
  depth 0 on the first call. The contract (eager fields populated, lazy
  fields shaped as `Vec<NamedTypeHandle>`, BFS bridge with depth-exceeded
  envelope) is exercised by 1B tests. Deep-traversal bytes-equiv tests
  against real corpus components land in 1C-α once fingerprint population is
  wired.
