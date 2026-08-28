# LSP-branch-pending items (release-clean full-green gate)

The release-clean tree is green for the non-LSP surfaces. Its **full-green gate additionally
awaits the external LSP branch** — the items below are LSP-owned and were **not** in scope for the
release-clean review (docs pass); they are recorded here so the dependency is explicit, not silent.
None is a non-LSP blocker. Resolve each when the LSP branch lands.

## Pending items

1. **Architecture/foundation findings remain on LSP-owned files.** The checks cover direct
   `std::fs` usage and the VFS-boundary (source-read authority); separately, the informational
   source-size advisory reports one non-exempt LSP source. These are LSP-branch cleanups, not
   non-LSP regressions.
2. **C5 — provider-surface-store legacy naming / half-wiring.** The provider-surface / carrier-store
   layer carries legacy naming and a partially-wired path awaiting the LSP branch's completion.
3. **Relay-shim signal tests (×3) need a bare-CI runner.** They pass on a bare CI runner but fail
   under local process-supervisor harnesses (signal delivery differs). Untouched by this review;
   they gate on runner environment, not on product code.

## Disposition

LSP-branch turf. Not resolved here; coordinate landing with that branch. Cross-reference: the C3
cleanup debt ([`deferred-cleanup-debt.md`](deferred-cleanup-debt.md)) also cascades into `verter_lsp`
and must be coordinated with the same branch.
