// @ai-generated - Synthetic owner consuming a single barrel-re-exported
// type. Used by the selected-leaf and unselected-leaf edit scenarios.
//
// NOTE: this fixture is loaded into the Verter VFS via `upsert_ts` under a
// CANONICAL filename (e.g. `/fixtures/cache_invalidation_basic_selected.ts`)
// that does NOT match the physical V1/V2 files on disk
// (`*_selected_v1.ts` / `*_selected_v2.ts`). The Rust test rebinds the
// canonical id between V1 and V2 source bodies to simulate an edit cycle.
// Consequently this fixture group is NOT standalone-typecheckable under
// tsgo — the relative imports resolve only inside Verter's in-memory VFS.
// Static tsgo verification was applied to each V1/V2 leaf body and to the
// owner body in isolation; the multi-file import graph is exercised
// exclusively through the Rust test's upsert sequence.

import type { Selected } from "./cache_invalidation_basic_barrel";

export type Surface = Selected;
